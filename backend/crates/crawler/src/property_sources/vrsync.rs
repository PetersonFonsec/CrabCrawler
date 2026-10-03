//! Feed XML no padrão VRSync (Grupo OLX: ZAP, Viva Real, OLX).
//!
//! Estrutura conferida em developers.grupozap.com/feeds/vrsync/ em
//! 2026-10-03 (Header, Listing, Details e Exemplos). Detalhes e o que não
//! pôde ser confirmado: `docs/property-sources/VRSYNC.md`.
//!
//! ```text
//! <ListingDataFeed xmlns="http://www.vivareal.com/schemas/1.0/VRSync">
//!   <Header>…</Header>
//!   <Listings><Listing>…</Listing>…</Listings>
//! </ListingDataFeed>
//! ```
//!
//! O provider só mapeia a estrutura para [`RawProperty`]; os valores
//! ("For Sale", "Residential / Apartment", "860000") são interpretados pelo
//! normalizer.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use crab_domain::{PropertySource, RawProperty};
use reqwest::header::{HeaderName, HeaderValue, AUTHORIZATION};
use serde_json::{json, Value};
use uuid::Uuid;

use super::xml::{read_document, Element, XmlLimits};
use super::{FetchBatch, ItemReadError, PropertySourceProvider};
use crate::http::{redact_url, HttpClient};
use crate::CrawlError;

pub const NAMESPACE: &str = "http://www.vivareal.com/schemas/1.0/VRSync";
/// Limite documentado pelo Grupo OLX: até 50 mil anúncios por arquivo.
pub const MAX_LISTINGS: usize = 50_000;
/// Limite de download padrão.
pub const DEFAULT_MAX_BYTES: usize = 200 * 1024 * 1024;

/// De onde vem o feed.
pub enum VrsyncInput {
    File(PathBuf),
    Url {
        http: Arc<HttpClient>,
        url: String,
        /// Valor do cabeçalho `Authorization`, quando o parceiro protege o
        /// feed. Vem de variável de ambiente; nunca é logado.
        authorization: Option<String>,
    },
}

pub struct VrsyncProvider {
    input: VrsyncInput,
    partner_id: Option<Uuid>,
    max_bytes: usize,
    max_listings: usize,
}

impl VrsyncProvider {
    pub fn new(input: VrsyncInput, partner_id: Option<Uuid>) -> Self {
        Self {
            input,
            partner_id,
            max_bytes: DEFAULT_MAX_BYTES,
            max_listings: MAX_LISTINGS,
        }
    }

    pub fn with_limits(mut self, max_bytes: usize, max_listings: usize) -> Self {
        self.max_bytes = max_bytes;
        self.max_listings = max_listings;
        self
    }

    async fn load(&self) -> Result<(Vec<u8>, String), CrawlError> {
        match &self.input {
            VrsyncInput::File(path) => {
                let meta = tokio::fs::metadata(path).await?;
                if meta.len() as usize > self.max_bytes {
                    return Err(CrawlError::TooLarge {
                        max_bytes: self.max_bytes,
                    });
                }
                Ok((tokio::fs::read(path).await?, path.display().to_string()))
            }
            VrsyncInput::Url {
                http,
                url,
                authorization,
            } => {
                validate_feed_url(url)?;
                let mut headers: Vec<(HeaderName, HeaderValue)> = vec![];
                if let Some(auth) = authorization {
                    let mut value = HeaderValue::from_str(auth).map_err(|_| {
                        CrawlError::Config("cabeçalho de autorização inválido".into())
                    })?;
                    value.set_sensitive(true);
                    headers.push((AUTHORIZATION, value));
                }
                let body = http
                    .get_bytes_limited(url, &headers, self.max_bytes)
                    .await?;
                Ok((body, redact_url(url)))
            }
        }
    }

    /// Interpreta um documento VRSync já carregado.
    pub fn parse(&self, bytes: &[u8], origin: &str) -> Result<FetchBatch, CrawlError> {
        parse_feed(bytes, origin, self.partner_id, self.max_listings)
    }
}

#[async_trait]
impl PropertySourceProvider for VrsyncProvider {
    fn source(&self) -> PropertySource {
        PropertySource::Vrsync
    }

    async fn fetch(&self) -> Result<FetchBatch, CrawlError> {
        let (bytes, origin) = self.load().await?;
        self.parse(&bytes, &origin)
    }
}

/// URL de feed aceita: http(s), com domínio, sem credenciais embutidas.
pub fn validate_feed_url(url: &str) -> Result<(), CrawlError> {
    let parsed =
        url::Url::parse(url).map_err(|_| CrawlError::Config("URL do feed inválida".into()))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(CrawlError::Config(
            "URL do feed precisa ser http ou https".into(),
        ));
    }
    if parsed.host_str().is_none_or(|h| h.is_empty()) {
        return Err(CrawlError::Config("URL do feed sem domínio".into()));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(CrawlError::Config(
            "credenciais não podem ir na URL; use a variável de ambiente de autorização".into(),
        ));
    }
    Ok(())
}

pub fn parse_feed(
    bytes: &[u8],
    origin: &str,
    partner_id: Option<Uuid>,
    max_listings: usize,
) -> Result<FetchBatch, CrawlError> {
    let mut header: Option<Element> = None;
    let mut items = Vec::new();
    let mut count = 0usize;
    let (root, namespace) = read_document(
        bytes,
        &[
            &["ListingDataFeed", "Header"],
            &["ListingDataFeed", "Listings", "Listing"],
        ],
        XmlLimits::default(),
        |path, element| {
            if path.last().map(String::as_str) == Some("Header") {
                header = Some(element);
                return Ok(());
            }
            count += 1;
            if count > max_listings {
                return Err(CrawlError::Parse(format!(
                    "feed com mais de {max_listings} anúncios (limite do padrão)"
                )));
            }
            items.push(map_listing(&element, partner_id));
            Ok(())
        },
    )?;
    if root != "ListingDataFeed" {
        return Err(CrawlError::Parse(format!(
            "raiz <{root}> não é um feed VRSync (esperado <ListingDataFeed>)"
        )));
    }
    let header_json = header.as_ref().map(Element::to_json).unwrap_or(Value::Null);
    Ok(FetchBatch {
        items,
        // O feed VRSync é o catálogo do anunciante; ver VRSYNC.md.
        complete: true,
        metadata: json!({
            "format": "VRSync",
            "origin": origin,
            "namespace": namespace,
            "namespace_matches": namespace.as_deref() == Some(NAMESPACE),
            "publish_date": header.as_ref().and_then(|h| h.child_text("PublishDate")),
            "provider": header.as_ref().and_then(|h| h.child_text("Provider")),
            "header": header_json,
        }),
    })
}

fn map_listing(el: &Element, partner_id: Option<Uuid>) -> Result<RawProperty, ItemReadError> {
    let external_id = el.child_text("ListingID");
    let fail = |reason: &str| ItemReadError {
        external_id: external_id.clone(),
        reason: reason.to_string(),
    };
    if external_id.is_none() {
        return Err(fail("<ListingID> ausente"));
    }
    let details = el
        .child("Details")
        .ok_or_else(|| fail("<Details> ausente"))?;
    let location = el
        .child("Location")
        .ok_or_else(|| fail("<Location> ausente"))?;

    // Moeda: pega a do preço principal; o normalizer recusa se não for BRL.
    let price_el = details.child("ListPrice").or(details.child("RentalPrice"));
    let currency = price_el
        .and_then(|p| p.attr("currency"))
        .map(str::to_string);
    let mixed_currency = [
        "ListPrice",
        "RentalPrice",
        "PropertyAdministrationFee",
        "Iptu",
        "YearlyTax",
    ]
    .iter()
    .filter_map(|n| details.child(n))
    .filter_map(|e| e.attr("currency"))
    .collect::<HashSet<_>>()
    .len()
        > 1;
    if mixed_currency {
        return Err(fail("moedas diferentes no mesmo anúncio"));
    }

    // IPTU: a página de Details documenta <Iptu period="Yearly|Monthly">;
    // o exemplo oficial usa <YearlyTax>. Os dois são aceitos.
    let (property_tax, property_tax_period) = match details.child("Iptu") {
        Some(iptu) => (
            iptu.text_trimmed(),
            Some(iptu.attr("period").unwrap_or("Yearly").to_string()),
        ),
        None => (details.child_text("YearlyTax"), Some("Yearly".to_string())),
    };

    let living = details.child("LivingArea");
    let lot = details.child("LotArea");
    let area_unit = living
        .or(lot)
        .and_then(|a| a.attr("unit"))
        .map(str::to_string);
    if let (Some(a), Some(b)) = (
        living.and_then(|e| e.attr("unit")),
        lot.and_then(|e| e.attr("unit")),
    ) {
        if a != b {
            return Err(fail("unidades de área diferentes no mesmo anúncio"));
        }
    }

    // displayAddress: o anunciante escolhe o que pode ser exibido. "All"
    // libera tudo; "Street" esconde número; "Neighborhood" esconde a rua.
    // Coordenada revela o número, então só é usada com "All".
    let display = location.attr("displayAddress").unwrap_or("All");
    let show_street = display != "Neighborhood";
    let show_number = display == "All";

    let state = location.child("State").and_then(|s| {
        s.attr("abbreviation")
            .map(str::to_string)
            .filter(|a| !a.trim().is_empty())
            .or_else(|| s.text_trimmed())
    });

    let mut images: Vec<(bool, String)> = el
        .child("Media")
        .map(|m| {
            m.children_named("Item")
                .filter(|i| i.attr("medium").unwrap_or("image") == "image")
                .filter_map(|i| Some((i.attr("primary") == Some("true"), i.text_trimmed()?)))
                .collect()
        })
        .unwrap_or_default();
    images.sort_by_key(|(primary, _)| !*primary);

    // Payload bruto sem ContactInfo (dados de contato não são necessários).
    let mut payload = el.clone();
    payload.children.retain(|c| c.name != "ContactInfo");

    Ok(RawProperty {
        source: Some(PropertySource::Vrsync),
        partner_id,
        external_id,
        source_url: el.child_text("DetailViewUrl"),
        title: el.child_text("Title"),
        description: details.child_text("Description"),
        notes: None,
        transaction: el.child_text("TransactionType"),
        property_type: details.child_text("PropertyType"),
        usage_type: details.child_text("UsageType"),
        currency,
        sale_price: details.child_text("ListPrice"),
        rent_price: details.child_text("RentalPrice"),
        rent_period: details
            .child("RentalPrice")
            .map(|r| r.attr("period").unwrap_or("Monthly").to_string()),
        condominium_fee: details.child_text("PropertyAdministrationFee"),
        property_tax,
        property_tax_period,
        living_area: living.and_then(Element::text_trimmed),
        lot_area: lot.and_then(Element::text_trimmed),
        area_unit,
        bedrooms: details.child_text("Bedrooms"),
        bathrooms: details.child_text("Bathrooms"),
        suites: details.child_text("Suites"),
        parking_spaces: details.child_text("Garage"),
        street: location.child_text("Address").filter(|_| show_street),
        street_number: location.child_text("StreetNumber").filter(|_| show_number),
        complement: location.child_text("Complement").filter(|_| show_number),
        neighborhood: location.child_text("Neighborhood"),
        municipality: location.child_text("City"),
        state,
        postal_code: location.child_text("PostalCode"),
        country: location.child("Country").and_then(|c| {
            c.attr("abbreviation")
                .map(str::to_string)
                .or_else(|| c.text_trimmed())
        }),
        latitude: location.child_text("Latitude").filter(|_| show_number),
        longitude: location.child_text("Longitude").filter(|_| show_number),
        coordinates_origin: None,
        images: images.into_iter().map(|(_, url)| url).collect(),
        raw_payload: payload.to_json(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Vec<u8> {
        let path = format!(
            "{}/../../fixtures/vrsync/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        std::fs::read(path).unwrap()
    }

    #[test]
    fn parses_official_example_structure() {
        let batch = parse_feed(&fixture("feed_v1.xml"), "test", None, MAX_LISTINGS).unwrap();
        assert!(batch.complete);
        assert_eq!(batch.metadata["namespace_matches"], true);
        let first = batch.items[0].as_ref().unwrap();
        assert_eq!(first.external_id.as_deref(), Some("SBC-001"));
        assert_eq!(first.transaction.as_deref(), Some("For Sale"));
        assert_eq!(
            first.property_type.as_deref(),
            Some("Residential / Apartment")
        );
        assert_eq!(first.sale_price.as_deref(), Some("620000"));
        assert_eq!(first.state.as_deref(), Some("SP"));
        assert_eq!(first.area_unit.as_deref(), Some("square metres"));
        assert_eq!(first.parking_spaces.as_deref(), Some("1"));
        assert!(first.images[0].ends_with("foto01.jpg"), "primária primeiro");
        assert!(first.raw_payload.get("ContactInfo").is_none());
    }

    #[test]
    fn display_address_hides_number_and_coordinates() {
        let batch = parse_feed(&fixture("feed_v1.xml"), "test", None, MAX_LISTINGS).unwrap();
        let hidden = batch
            .items
            .iter()
            .filter_map(|i| i.as_ref().ok())
            .find(|i| i.external_id.as_deref() == Some("SBC-003"))
            .unwrap();
        assert!(hidden.street.is_none());
        assert!(hidden.latitude.is_none());
        assert_eq!(hidden.neighborhood.as_deref(), Some("Centro"));
    }

    #[test]
    fn listing_without_id_is_an_item_error() {
        let batch = parse_feed(&fixture("feed_v1.xml"), "test", None, MAX_LISTINGS).unwrap();
        assert!(batch
            .items
            .iter()
            .any(|i| matches!(i, Err(e) if e.reason.contains("ListingID"))));
    }

    #[test]
    fn rejects_non_vrsync_root_and_limits() {
        assert!(parse_feed(b"<Foo/>", "t", None, 10).is_err());
        assert!(parse_feed(&fixture("feed_v1.xml"), "t", None, 1).is_err());
        assert!(parse_feed(&fixture("feed_malformed.xml"), "t", None, 10).is_err());
    }

    #[test]
    fn feed_url_validation() {
        assert!(validate_feed_url("https://imobiliaria.example.com.br/feed.xml").is_ok());
        assert!(validate_feed_url("file:///etc/passwd").is_err());
        assert!(validate_feed_url("https://u:p@x.com/feed.xml").is_err());
        assert!(validate_feed_url("not a url").is_err());
    }
}

//! Anúncios do arquivo de desenvolvimento (`fixtures/listings.json`)
//! convertidos para o formato comum dos providers.

use crab_domain::{PropertySource, RawListing, RawProperty};
use crab_processing::parse_location_text;

pub fn to_raw_property(raw: RawListing) -> RawProperty {
    let location = parse_location_text(&raw.location_text);
    let raw_payload = serde_json::to_value(&raw).unwrap_or_default();
    let num = |v: Option<f64>| v.map(|n| n.to_string());
    let int = |v: Option<u16>| v.map(|n| n.to_string());
    RawProperty {
        source: Some(PropertySource::Fixture),
        partner_id: None,
        external_id: Some(raw.external_id),
        source_url: raw.url,
        title: Some(raw.title),
        transaction: Some(raw.transaction.as_str().to_string()),
        property_type: Some(raw.kind.as_str().to_string()),
        currency: Some("BRL".into()),
        sale_price: num(raw.price_brl)
            .filter(|_| raw.transaction == crab_domain::TransactionType::Sale),
        rent_price: num(raw.price_brl)
            .filter(|_| raw.transaction == crab_domain::TransactionType::Rent),
        living_area: num(raw.area_m2),
        area_unit: Some("m2".into()),
        bedrooms: int(raw.bedrooms),
        bathrooms: int(raw.bathrooms),
        parking_spaces: int(raw.parking_spots),
        neighborhood: location.neighborhood,
        municipality: location.municipality,
        state: location.state,
        postal_code: raw.postal_code,
        latitude: num(raw.lat),
        longitude: num(raw.lon),
        raw_payload,
        ..Default::default()
    }
}

/// Provider sobre o arquivo de desenvolvimento. Não é um catálogo completo:
/// nada é inativado por ausência.
pub struct FixtureProvider {
    source: crab_crawler::sources::fixture::FixtureListingSource,
}

impl FixtureProvider {
    pub fn new(path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            source: crab_crawler::sources::fixture::FixtureListingSource::new(path),
        }
    }
}

#[crab_crawler::async_trait]
impl crab_crawler::PropertySourceProvider for FixtureProvider {
    fn source(&self) -> PropertySource {
        PropertySource::Fixture
    }

    async fn fetch(&self) -> Result<crab_crawler::FetchBatch, crab_crawler::CrawlError> {
        use crab_crawler::ListingSource;
        let items = self.source.fetch().await?;
        Ok(crab_crawler::FetchBatch {
            items: items.into_iter().map(|r| Ok(to_raw_property(r))).collect(),
            complete: false,
            metadata: serde_json::json!({ "format": "fixture" }),
        })
    }
}

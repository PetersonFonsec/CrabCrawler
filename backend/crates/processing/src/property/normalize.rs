//! `RawProperty` → imóvel + anúncio(s) normalizados.

use crab_domain::{
    Address, CoordinatePrecision, CoordinateSource, Coordinates, ListingDetails, PropertyDetails,
    PropertySource, PropertyType, RawProperty, TransactionType,
};
use serde::Serialize;
use serde_json::{Map, Value};
use uuid::Uuid;

use super::values::{self, limits};
use crate::normalize::{slugify, Gazetteer};

/// Problema num campo. Em `errors` impede o cadastro; em `warnings` o campo
/// é descartado e o resto segue.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FieldIssue {
    pub field: String,
    pub message: String,
}

impl FieldIssue {
    fn new(field: &str, message: impl Into<String>) -> Self {
        Self {
            field: field.to_string(),
            message: message.into(),
        }
    }
}

/// Resultado da normalização de um item válido.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NormalizedProperty {
    pub source: PropertySource,
    pub partner_id: Option<Uuid>,
    /// `None` só no cadastro manual (o id é gerado ao gravar).
    pub external_id: Option<String>,
    pub property: PropertyDetails,
    /// Um anúncio por finalidade ("Sale/Rent" gera dois).
    pub listings: Vec<ListingDetails>,
    /// Valores originais que passaram por conversão.
    pub original: Value,
    pub raw_payload: Value,
    pub warnings: Vec<FieldIssue>,
}

/// Item recusado: o que faltou ou estava errado.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NormalizationFailure {
    pub external_id: Option<String>,
    pub errors: Vec<FieldIssue>,
    pub warnings: Vec<FieldIssue>,
}

impl std::fmt::Display for NormalizationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let parts: Vec<String> = self
            .errors
            .iter()
            .map(|e| format!("{}: {}", e.field, e.message))
            .collect();
        write!(f, "{}", parts.join("; "))
    }
}

/// Converte formatos diferentes de cada fonte num modelo único.
#[derive(Debug, Clone)]
pub struct PropertyNormalizer {
    gazetteer: Gazetteer,
}

impl PropertyNormalizer {
    pub fn new(gazetteer: Gazetteer) -> Self {
        Self { gazetteer }
    }

    pub fn gazetteer(&self) -> &Gazetteer {
        &self.gazetteer
    }

    pub fn normalize(&self, raw: RawProperty) -> Result<NormalizedProperty, NormalizationFailure> {
        let external_id = values::clean_text(raw.external_id.as_deref());
        let source = raw.source.unwrap_or(PropertySource::Manual);
        let mut issues = Issues {
            errors: vec![],
            warnings: vec![],
            manual: source == PropertySource::Manual,
        };
        if raw.source.is_none() {
            issues.error("source", "fonte não informada");
        }
        if source != PropertySource::Manual && external_id.is_none() {
            issues.error("external_id", "id do anúncio na fonte ausente");
        }
        if external_id.as_ref().is_some_and(|id| id.len() > 200) {
            issues.error("external_id", "id do anúncio longo demais");
        }

        // ---- finalidade e preços
        let transactions = match raw.transaction.as_deref().map(str::trim) {
            None | Some("") => {
                issues.error("transaction", "finalidade não informada");
                vec![]
            }
            Some(t) => values::parse_transaction(t).unwrap_or_else(|| {
                issues.error("transaction", format!("finalidade não reconhecida: {t}"));
                vec![]
            }),
        };
        if let Err(e) = values::check_currency(raw.currency.as_deref()) {
            issues.error("currency", e);
        }

        let mut prices: Vec<(TransactionType, f64)> = Vec::new();
        let multiple = transactions.len() > 1;
        for t in &transactions {
            let (field, text, bounds) = match t {
                TransactionType::Sale => {
                    ("sale_price", raw.sale_price.as_deref(), limits::SALE_PRICE)
                }
                TransactionType::Rent => {
                    ("rent_price", raw.rent_price.as_deref(), limits::RENT_PRICE)
                }
            };
            // Com "Sale/Rent", um problema derruba só aquela finalidade.
            let problem = if *t == TransactionType::Rent
                && clean_period(raw.rent_period.as_deref())
                    .is_some_and(|p| p != "monthly" && p != "mensal")
            {
                Some(FieldIssue::new(
                    "rent_period",
                    format!(
                        "só aluguel mensal é suportado (recebido: {})",
                        raw.rent_period.as_deref().unwrap_or_default()
                    ),
                ))
            } else {
                match text.map(str::trim).filter(|v| !v.is_empty()) {
                    None => Some(FieldIssue::new(field, "preço não informado")),
                    Some(v) => match values::parse_money(v, bounds) {
                        Ok(price) => {
                            prices.push((*t, price));
                            None
                        }
                        Err(e) => Some(FieldIssue::new(field, e)),
                    },
                }
            };
            if let Some(p) = problem {
                if multiple {
                    issues.warnings.push(p);
                } else {
                    issues.errors.push(p);
                }
            }
        }
        if multiple && prices.is_empty() {
            issues.error("price", "nenhum preço válido");
        }

        let condominium_fee =
            issues.soft_parse("condominium_fee", raw.condominium_fee.as_deref(), |v| {
                values::parse_money(v, limits::CONDOMINIUM_FEE)
            });
        let mut property_tax =
            issues.soft_parse("property_tax", raw.property_tax.as_deref(), |v| {
                values::parse_money(v, limits::PROPERTY_TAX_YEARLY)
            });
        if clean_period(raw.property_tax_period.as_deref()).as_deref() == Some("monthly") {
            property_tax = property_tax.map(|v| v * 12.0);
        }

        // ---- características
        let unit = raw.area_unit.as_deref();
        let area_m2 = issues.soft_parse("area_m2", raw.living_area.as_deref(), |v| {
            values::parse_area(v, unit, limits::LIVING_AREA_M2)
        });
        let lot_area_m2 = issues.soft_parse("lot_area_m2", raw.lot_area.as_deref(), |v| {
            values::parse_area(v, unit, limits::LOT_AREA_M2)
        });
        let rooms = |v: &str| values::parse_count(v, limits::ROOMS);
        let bedrooms = issues.soft_parse("bedrooms", raw.bedrooms.as_deref(), rooms);
        let bathrooms = issues.soft_parse("bathrooms", raw.bathrooms.as_deref(), rooms);
        let suites = issues.soft_parse("suites", raw.suites.as_deref(), rooms);
        let parking_spaces =
            issues.soft_parse("parking_spaces", raw.parking_spaces.as_deref(), |v| {
                values::parse_count(v, limits::PARKING)
            });

        let property_type = match raw.property_type.as_deref().map(str::trim) {
            None | Some("") => PropertyType::Other,
            Some(text) => values::parse_property_type(text).unwrap_or_else(|| {
                issues.warn(
                    "property_type",
                    format!("tipo não reconhecido, gravado como 'other': {text}"),
                );
                PropertyType::Other
            }),
        };

        // ---- endereço
        let state = match values::clean_text(raw.state.as_deref()) {
            None => None,
            Some(s) => match values::parse_uf(&s) {
                Some(uf) => Some(uf.to_string()),
                None => {
                    issues.error("state", format!("UF inválida: {s}"));
                    None
                }
            },
        };
        let postal_code = match values::clean_text(raw.postal_code.as_deref()) {
            None => None,
            Some(cep) => match values::parse_cep(&cep) {
                Ok(c) => Some(c),
                Err(e) => {
                    issues.error("postal_code", e);
                    None
                }
            },
        };
        let municipality = values::clean_text(raw.municipality.as_deref());
        let neighborhood = values::clean_text(raw.neighborhood.as_deref());
        let municipality_ibge_code = match (&state, &municipality) {
            (Some(uf), Some(city)) => self.gazetteer.lookup(uf, city).map(str::to_string),
            _ => None,
        };
        let address = Address {
            street: raw.street.as_deref().and_then(values::normalize_street),
            number: raw
                .street_number
                .as_deref()
                .and_then(values::normalize_number),
            complement: values::clean_text(raw.complement.as_deref()),
            neighborhood_slug: neighborhood.as_deref().map(slugify),
            neighborhood,
            municipality,
            municipality_ibge_code,
            state,
            postal_code,
        };

        let coordinates = match (raw.latitude.as_deref(), raw.longitude.as_deref()) {
            (Some(lat), Some(lon)) if !lat.trim().is_empty() && !lon.trim().is_empty() => {
                match values::parse_point(lat, lon) {
                    Ok(point) => Some(Coordinates {
                        point,
                        source: raw
                            .coordinates_origin
                            .unwrap_or(CoordinateSource::PropertySource),
                        precision: CoordinatePrecision::Reported,
                    }),
                    Err(e) => {
                        issues.warn("coordinates", e);
                        None
                    }
                }
            }
            _ => None,
        };

        // Sem nenhum ponto de partida não há como analisar a região.
        let has_anchor = address.postal_code.is_some()
            || (address.municipality.is_some() && address.state.is_some())
            || coordinates.is_some();
        if !has_anchor && !issues.errors.iter().any(|e| e.field == "postal_code") {
            issues.error(
                "location",
                "informe um CEP válido, ou município e UF, ou a coordenada",
            );
        }

        // ---- textos e links
        let source_url = issues.soft_parse("source_url", raw.source_url.as_deref(), |v| {
            values::parse_external_url(v)
        });
        let title = issues.text("title", raw.title.as_deref(), limits::TITLE_LEN);
        let description = issues.text(
            "description",
            raw.description.as_deref(),
            limits::DESCRIPTION_LEN,
        );
        let notes = issues.text("notes", raw.notes.as_deref(), limits::NOTES_LEN);
        let mut images = Vec::new();
        for url in &raw.images {
            match values::parse_external_url(url) {
                Ok(u) if images.len() < limits::IMAGES => images.push(u),
                Ok(_) => {}
                Err(e) => issues.warn("images", e),
            }
        }

        if !issues.errors.is_empty() {
            return Err(NormalizationFailure {
                external_id,
                errors: issues.errors,
                warnings: issues.warnings,
            });
        }

        let property = PropertyDetails {
            property_type,
            area_m2,
            lot_area_m2,
            bedrooms,
            bathrooms,
            suites,
            parking_spaces,
            address,
            coordinates,
        };
        let title = title.or_else(|| Some(display_title(&property)));
        let listings = prices
            .into_iter()
            .map(|(transaction, price_brl)| ListingDetails {
                transaction,
                price_brl,
                condominium_fee_brl: condominium_fee,
                property_tax_brl: property_tax,
                title: title.clone(),
                description: description.clone(),
                notes: notes.clone(),
                source_url: source_url.clone(),
                images: images.clone(),
            })
            .collect();

        Ok(NormalizedProperty {
            source,
            partner_id: raw.partner_id,
            external_id,
            property,
            listings,
            original: original_values(&raw),
            raw_payload: raw.raw_payload,
            warnings: issues.warnings,
        })
    }
}

/// Acumula problemas. No manual um erro de digitação volta para o usuário
/// (`soft` vira erro); num feed, um campo acessório inválido é descartado
/// com aviso e não derruba o anúncio.
struct Issues {
    errors: Vec<FieldIssue>,
    warnings: Vec<FieldIssue>,
    manual: bool,
}

impl Issues {
    fn error(&mut self, field: &str, message: impl Into<String>) {
        self.errors.push(FieldIssue::new(field, message));
    }

    fn warn(&mut self, field: &str, message: impl Into<String>) {
        self.warnings.push(FieldIssue::new(field, message));
    }

    fn soft(&mut self, field: &str, message: String) {
        if self.manual {
            self.error(field, message);
        } else {
            self.warn(field, message);
        }
    }

    fn soft_parse<T>(
        &mut self,
        field: &str,
        text: Option<&str>,
        parse: impl Fn(&str) -> Result<T, String>,
    ) -> Option<T> {
        let text = text.map(str::trim).filter(|t| !t.is_empty())?;
        match parse(text) {
            Ok(v) => Some(v),
            Err(e) => {
                self.soft(field, e);
                None
            }
        }
    }

    fn text(&mut self, field: &str, value: Option<&str>, max: usize) -> Option<String> {
        let value = values::clean_text_keep_lines(value)?;
        let (value, cut) = values::truncate(value, max);
        if cut {
            self.warn(field, format!("texto cortado em {max} caracteres"));
        }
        Some(value)
    }
}

fn clean_period(text: Option<&str>) -> Option<String> {
    text.map(slugify).filter(|s| !s.is_empty())
}

/// Título gerado quando a fonte não manda um:
/// `"Apartamento, 2 quartos, Rudge Ramos"`.
pub fn display_title(p: &PropertyDetails) -> String {
    let kind = match p.property_type {
        PropertyType::Apartment => "Apartamento",
        PropertyType::House => "Casa",
        PropertyType::CondoHouse => "Casa em condomínio",
        PropertyType::Penthouse => "Cobertura",
        PropertyType::Studio => "Studio",
        PropertyType::Flat => "Flat",
        PropertyType::Land => "Terreno",
        PropertyType::Commercial => "Imóvel comercial",
        PropertyType::Rural => "Imóvel rural",
        PropertyType::Other => "Imóvel",
    };
    let mut parts = vec![kind.to_string()];
    match p.bedrooms {
        Some(1) => parts.push("1 quarto".into()),
        Some(n) if n > 1 => parts.push(format!("{n} quartos")),
        _ => {}
    }
    if let Some(place) = p
        .address
        .neighborhood
        .as_ref()
        .or(p.address.municipality.as_ref())
    {
        parts.push(place.clone());
    }
    parts.join(", ")
}

fn original_values(raw: &RawProperty) -> Value {
    let mut map = Map::new();
    let mut put = |key: &str, value: &Option<String>| {
        if let Some(v) = value {
            map.insert(key.to_string(), Value::String(v.clone()));
        }
    };
    put("transaction", &raw.transaction);
    put("property_type", &raw.property_type);
    put("usage_type", &raw.usage_type);
    put("currency", &raw.currency);
    put("sale_price", &raw.sale_price);
    put("rent_price", &raw.rent_price);
    put("rent_period", &raw.rent_period);
    put("condominium_fee", &raw.condominium_fee);
    put("property_tax", &raw.property_tax);
    put("property_tax_period", &raw.property_tax_period);
    put("living_area", &raw.living_area);
    put("lot_area", &raw.lot_area);
    put("area_unit", &raw.area_unit);
    put("street", &raw.street);
    put("state", &raw.state);
    put("municipality", &raw.municipality);
    put("postal_code", &raw.postal_code);
    put("latitude", &raw.latitude);
    put("longitude", &raw.longitude);
    Value::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn normalizer() -> PropertyNormalizer {
        PropertyNormalizer::new(Gazetteer::mvp())
    }

    fn feed_item() -> RawProperty {
        RawProperty {
            source: Some(PropertySource::Vrsync),
            external_id: Some("Imovel-01".into()),
            transaction: Some("For Sale".into()),
            property_type: Some("Residential / Apartment".into()),
            currency: Some("BRL".into()),
            sale_price: Some("620000".into()),
            living_area: Some("80".into()),
            area_unit: Some("square metres".into()),
            bedrooms: Some("2".into()),
            state: Some("São Paulo".into()),
            municipality: Some("São Bernardo do Campo".into()),
            neighborhood: Some("Rudge Ramos".into()),
            postal_code: Some("09640-000".into()),
            street: Some("R. Exemplo".into()),
            latitude: Some("-23.656".into()),
            longitude: Some("-46.573".into()),
            ..Default::default()
        }
    }

    #[test]
    fn normalizes_feed_item() {
        let n = normalizer().normalize(feed_item()).unwrap();
        assert_eq!(n.property.property_type, PropertyType::Apartment);
        assert_eq!(n.property.address.state.as_deref(), Some("SP"));
        assert_eq!(n.property.address.postal_code.as_deref(), Some("09640000"));
        assert_eq!(
            n.property.address.municipality_ibge_code.as_deref(),
            Some("3548708")
        );
        assert_eq!(n.property.address.street.as_deref(), Some("Rua Exemplo"));
        assert_eq!(n.property.area_m2, Some(80.0));
        let c = n.property.coordinates.unwrap();
        assert_eq!(c.source, CoordinateSource::PropertySource);
        assert_eq!(n.listings.len(), 1);
        assert_eq!(n.listings[0].price_brl, 620_000.0);
        assert_eq!(n.original["property_type"], "Residential / Apartment");
    }

    #[test]
    fn sale_rent_creates_two_listings() {
        let mut raw = feed_item();
        raw.transaction = Some("Sale/Rent".into());
        raw.rent_price = Some("3200".into());
        let n = normalizer().normalize(raw).unwrap();
        let t: Vec<_> = n.listings.iter().map(|l| l.transaction).collect();
        assert_eq!(t, vec![TransactionType::Sale, TransactionType::Rent]);
    }

    #[test]
    fn sale_rent_with_one_bad_price_keeps_the_other() {
        let mut raw = feed_item();
        raw.transaction = Some("Sale/Rent".into());
        raw.rent_price = Some("abc".into());
        let n = normalizer().normalize(raw).unwrap();
        assert_eq!(n.listings.len(), 1);
        assert!(n.warnings.iter().any(|w| w.field == "rent_price"));
    }

    #[test]
    fn rejects_missing_external_id_for_feeds() {
        let mut raw = feed_item();
        raw.external_id = None;
        let err = normalizer().normalize(raw).unwrap_err();
        assert!(err.errors.iter().any(|e| e.field == "external_id"));
    }

    #[test]
    fn rejects_without_location_anchor() {
        let mut raw = feed_item();
        raw.postal_code = None;
        raw.municipality = None;
        raw.latitude = None;
        let err = normalizer().normalize(raw).unwrap_err();
        assert!(err.errors.iter().any(|e| e.field == "location"));
    }

    #[test]
    fn monthly_iptu_becomes_yearly() {
        let mut raw = feed_item();
        raw.property_tax = Some("150".into());
        raw.property_tax_period = Some("Monthly".into());
        let n = normalizer().normalize(raw).unwrap();
        assert_eq!(n.listings[0].property_tax_brl, Some(1800.0));
    }

    #[test]
    fn unknown_type_is_kept_as_other_with_warning() {
        let mut raw = feed_item();
        raw.property_type = Some("Barco".into());
        let n = normalizer().normalize(raw).unwrap();
        assert_eq!(n.property.property_type, PropertyType::Other);
        assert!(n.warnings.iter().any(|w| w.field == "property_type"));
        assert_eq!(n.original["property_type"], "Barco");
    }

    #[test]
    fn bad_coordinates_are_dropped_not_fatal() {
        let mut raw = feed_item();
        raw.latitude = Some("0".into());
        raw.longitude = Some("0".into());
        let n = normalizer().normalize(raw).unwrap();
        assert!(n.property.coordinates.is_none());
    }

    #[test]
    fn generates_title_when_missing() {
        let n = normalizer().normalize(feed_item()).unwrap();
        assert_eq!(
            n.listings[0].title.as_deref(),
            Some("Apartamento, 2 quartos, Rudge Ramos")
        );
    }
}

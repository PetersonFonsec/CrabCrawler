use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{Location, Provenance};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransactionType {
    Sale,
    Rent,
}

impl TransactionType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Sale => "sale",
            Self::Rent => "rent",
        }
    }
}

/// Tipo do imóvel físico. Valores de cada fonte ("Residential / Apartment",
/// "apto"...) são convertidos pelo normalizer; o texto original fica no
/// anúncio.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PropertyType {
    Apartment,
    House,
    /// Casa em condomínio fechado.
    CondoHouse,
    Penthouse,
    /// Studio, kitnet, loft.
    Studio,
    Flat,
    Land,
    Commercial,
    Rural,
    #[default]
    Other,
}

impl PropertyType {
    pub const ALL: [PropertyType; 10] = [
        Self::Apartment,
        Self::House,
        Self::CondoHouse,
        Self::Penthouse,
        Self::Studio,
        Self::Flat,
        Self::Land,
        Self::Commercial,
        Self::Rural,
        Self::Other,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Apartment => "apartment",
            Self::House => "house",
            Self::CondoHouse => "condo_house",
            Self::Penthouse => "penthouse",
            Self::Studio => "studio",
            Self::Flat => "flat",
            Self::Land => "land",
            Self::Commercial => "commercial",
            Self::Rural => "rural",
            Self::Other => "other",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.as_str() == value)
    }
}

/// Nome antigo de [`PropertyType`], mantido para o fixture de anúncios.
pub type ListingKind = PropertyType;

/// Anúncio como veio da fonte, antes de qualquer normalização.
///
/// `location_text` é o texto livre do portal, por exemplo
/// `"São Bernardo do Campo - SP / Rudge Ramos"`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawListing {
    pub external_id: String,
    pub url: Option<String>,
    pub title: String,
    pub transaction: TransactionType,
    pub kind: ListingKind,
    pub price_brl: Option<f64>,
    pub area_m2: Option<f64>,
    pub bedrooms: Option<u16>,
    pub bathrooms: Option<u16>,
    pub parking_spots: Option<u16>,
    pub location_text: String,
    pub postal_code: Option<String>,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
}

/// Anúncio normalizado, pronto para ser persistido e enriquecido.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Listing {
    pub id: Uuid,
    pub external_id: String,
    pub title: String,
    pub transaction: TransactionType,
    pub kind: ListingKind,
    pub price_brl: Option<f64>,
    pub area_m2: Option<f64>,
    pub bedrooms: Option<u16>,
    pub bathrooms: Option<u16>,
    pub parking_spots: Option<u16>,
    pub location: Location,
    pub provenance: Provenance,
    pub updated_at: DateTime<Utc>,
}

impl Listing {
    /// ID determinístico a partir de (fonte, id externo): coletar o mesmo
    /// anúncio duas vezes gera o mesmo UUID, o que evita duplicidade.
    pub fn stable_id(source: &str, external_id: &str) -> Uuid {
        let key = format!("{source}:{external_id}");
        Uuid::new_v5(&Uuid::NAMESPACE_URL, key.as_bytes())
    }

    pub fn price_per_m2(&self) -> Option<f64> {
        match (self.price_brl, self.area_m2) {
            (Some(price), Some(area)) if area > 0.0 => Some(price / area),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_id_is_deterministic() {
        let a = Listing::stable_id("listing_fixture", "123");
        let b = Listing::stable_id("listing_fixture", "123");
        let c = Listing::stable_id("listing_fixture", "124");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}

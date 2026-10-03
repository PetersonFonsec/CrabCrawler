//! Normalização e enriquecimento.
//!
//! É aqui que fica o diferencial do projeto: transformar dados heterogêneos
//! num modelo comum e cruzá-los pela localização.

pub mod enrich;
pub mod normalize;
pub mod property;
pub mod regional;
pub mod security;

pub use enrich::{compare_price, compare_price_per_m2, PriceComparison};
pub use normalize::{normalize_listing, parse_location_text, slugify, Gazetteer};
pub use property::{NormalizedProperty, PropertyNormalizer};

//! Normalização de imóveis vindos de qualquer fonte, hash de conteúdo e
//! leitura do histórico de preço.

pub mod hash;
pub mod normalize;
pub mod price;
pub mod values;

pub use hash::content_hash;
pub use normalize::{
    display_title, FieldIssue, NormalizationFailure, NormalizedProperty, PropertyNormalizer,
};
pub use price::{price_insight, PriceDirection, PriceInsight};

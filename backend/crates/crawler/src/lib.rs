//! Coleta de dados: anúncios e datasets públicos.
//!
//! Regra do projeto: sempre que existir API ou dataset oficial, use-o antes
//! de recorrer a scraping HTML. Toda requisição passa por [`http::HttpClient`],
//! que aplica rate limiting, retry com backoff e logs.

pub mod error;
pub mod http;
pub mod security;
pub mod sources;

pub use error::CrawlError;
pub use security::{SecurityDataProvider, SecurityScope};

use async_trait::async_trait;
use crab_domain::{RawListing, RegionIndicator};

/// Fonte de anúncios imobiliários.
#[async_trait]
pub trait ListingSource: Send + Sync {
    fn name(&self) -> &'static str;
    async fn fetch(&self) -> Result<Vec<RawListing>, CrawlError>;
}

/// Fonte de indicadores regionais (criminalidade, demografia...).
#[async_trait]
pub trait IndicatorSource: Send + Sync {
    fn name(&self) -> &'static str;
    async fn fetch(&self, municipality_ibge_code: &str)
        -> Result<Vec<RegionIndicator>, CrawlError>;
}

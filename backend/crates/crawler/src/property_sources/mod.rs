//! Fontes de imóveis autorizadas.
//!
//! Cada fonte implementa [`PropertySourceProvider`] e entrega
//! [`RawProperty`]: o provider só lê e mapeia a estrutura da fonte, sem
//! interpretar valores. Normalização, geocoding e persistência são camadas
//! separadas (`crab-processing`, `crab-ingest`), então o domínio não depende
//! do formato de nenhuma fonte.
//!
//! O que **não** existe aqui, de propósito: scraping de portais. Receber a
//! URL de um anúncio não autoriza acessá-la.

pub mod api;
pub mod vrsync;
pub mod xml;

use async_trait::async_trait;
use crab_domain::{PropertySource, ProviderCapability, RawProperty};
use serde::Serialize;
use serde_json::Value;

use crate::CrawlError;

/// Item que veio da fonte mas não pôde ser lido (estrutura inesperada).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ItemReadError {
    /// Id do anúncio, quando foi possível ler.
    pub external_id: Option<String>,
    pub reason: String,
}

/// Resultado de uma busca em lote.
#[derive(Debug, Clone)]
pub struct FetchBatch {
    pub items: Vec<Result<RawProperty, ItemReadError>>,
    /// `true` quando o lote é o catálogo completo do parceiro (a ausência de
    /// um anúncio significa que ele saiu do ar).
    pub complete: bool,
    /// Metadados da fonte (versão, data de publicação do feed...).
    pub metadata: Value,
}

/// Uma fonte externa de imóveis.
///
/// Só `source` e `capabilities` são obrigatórios. Operações que a fonte não
/// oferece devolvem [`CrawlError::Unsupported`] e não aparecem em
/// `capabilities`.
#[async_trait]
pub trait PropertySourceProvider: Send + Sync {
    fn source(&self) -> PropertySource;

    fn capabilities(&self) -> &'static [ProviderCapability] {
        self.source().capabilities()
    }

    fn supports(&self, capability: ProviderCapability) -> bool {
        self.capabilities().contains(&capability)
    }

    /// Catálogo inteiro (`BULK_IMPORT`) ou o que mudou (`INCREMENTAL_SYNC`).
    async fn fetch(&self) -> Result<FetchBatch, CrawlError> {
        Err(CrawlError::Unsupported("fetch"))
    }

    /// Um anúncio pelo id na fonte (`FETCH_BY_ID`).
    async fn fetch_by_id(&self, _external_id: &str) -> Result<Option<RawProperty>, CrawlError> {
        Err(CrawlError::Unsupported("fetch_by_id"))
    }
}

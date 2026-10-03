//! Fontes de dados de segurança pública.
//!
//! Cada fonte implementa [`SecurityDataProvider`] e devolve
//! [`RawCrimeRecord`]s com o rótulo original da fonte. Nenhum provider
//! conhece anúncios: a ligação imóvel → região acontece depois, pela
//! localização normalizada.

pub mod sinesp;
pub mod ssp_sp;

use async_trait::async_trait;
use crab_domain::{DataSource, RawCrimeRecord};

use crate::CrawlError;

/// Recorte geográfico pedido a um provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecurityScope {
    /// UF (sigla). Toda fonte atual é filtrada por estado.
    pub state: String,
    /// Restringe a um município, quando a fonte traz o código IBGE.
    /// Fontes que só trazem o nome deixam o filtro para o normalizador.
    pub municipality_ibge_code: Option<String>,
}

impl SecurityScope {
    pub fn state(uf: &str) -> Self {
        Self {
            state: uf.to_uppercase(),
            municipality_ibge_code: None,
        }
    }

    pub fn municipality(uf: &str, ibge_code: &str) -> Self {
        Self {
            state: uf.to_uppercase(),
            municipality_ibge_code: Some(ibge_code.to_string()),
        }
    }
}

/// Fonte de estatísticas criminais (SSP-SP, Sinesp, futuras SSPs estaduais).
#[async_trait]
pub trait SecurityDataProvider: Send + Sync {
    fn source(&self) -> DataSource;
    async fn fetch(&self, scope: &SecurityScope) -> Result<Vec<RawCrimeRecord>, CrawlError>;
}

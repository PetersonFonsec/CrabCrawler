use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Fontes de dados conhecidas. Cada dado persistido aponta para uma delas,
/// para que a interface possa mostrar "de onde veio este número".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataSource {
    /// Anúncios carregados de um arquivo local (desenvolvimento).
    ListingFixture,
    /// API de Localidades do IBGE.
    IbgeLocalidades,
    /// Estatísticas de criminalidade da Secretaria da Segurança Pública de SP.
    SspSp,
}

impl DataSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ListingFixture => "listing_fixture",
            Self::IbgeLocalidades => "ibge_localidades",
            Self::SspSp => "ssp_sp",
        }
    }

    pub fn reference_url(&self) -> &'static str {
        match self {
            Self::ListingFixture => "fixtures/listings.json",
            Self::IbgeLocalidades => "https://servicodados.ibge.gov.br/api/docs/localidades",
            Self::SspSp => "https://www.ssp.sp.gov.br/estatistica",
        }
    }
}

/// Procedência de um dado: fonte, URL exata e momento da coleta.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Provenance {
    pub source: DataSource,
    pub url: Option<String>,
    pub collected_at: DateTime<Utc>,
}

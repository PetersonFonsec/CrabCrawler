//! Providers do Regional Intelligence.
//!
//! O domínio não conhece IBGE, Seade, GeoSampa ou SGB: conhece as traits
//! abaixo. Cada provider declara cobertura e capacidades
//! ([`ProviderDescriptor`]) e devolve um [`DatasetBatch`] com procedência.
//! Um provider novo (ex.: prefeitura de Santo André) é mais uma
//! implementação destas traits e mais uma linha em
//! `crab_domain::regional::REGIONAL_PROVIDERS`.
//!
//! Fluxo de cada importação: fonte oficial → arquivo bruto em disco →
//! parser (aqui) → normalizador (`crab-processing`) → banco.

pub mod geojson;
pub mod geosampa;
pub mod ibge;
pub mod raw;
pub mod seade;
pub mod sgb;
pub mod table;

use async_trait::async_trait;
use crab_domain::regional::{
    Capability, DatasetBatch, ProviderDescriptor, RawCensusSector, RawRiskArea, RawSectorValue,
    SectorIndicator, UrbanService,
};

use crate::CrawlError;

/// Recorte pedido a um provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegionalScope {
    /// Um município (código IBGE de 7 dígitos).
    Municipality(String),
    /// Uma UF (código IBGE de 2 dígitos, SP = "35").
    State(String),
}

impl RegionalScope {
    /// Valida o código: só dígitos, 2 (UF) ou 7 (município).
    pub fn parse(code: &str) -> Result<Self, CrawlError> {
        let code = code.trim();
        if !code.chars().all(|c| c.is_ascii_digit()) {
            return Err(CrawlError::Parse(format!("código IBGE inválido: {code:?}")));
        }
        match code.len() {
            7 => Ok(Self::Municipality(code.to_string())),
            2 => Ok(Self::State(code.to_string())),
            _ => Err(CrawlError::Parse(format!(
                "use código IBGE de município (7 dígitos) ou UF (2 dígitos): {code:?}"
            ))),
        }
    }

    pub fn prefix(&self) -> &str {
        match self {
            Self::Municipality(c) | Self::State(c) => c,
        }
    }

    pub fn municipality(&self) -> Option<&str> {
        match self {
            Self::Municipality(c) => Some(c),
            Self::State(_) => None,
        }
    }

    /// O código (de setor ou município) está dentro do recorte?
    pub fn contains(&self, code: &str) -> bool {
        code.starts_with(self.prefix())
    }
}

/// Base comum: todo provider informa cobertura, capacidades e política de
/// atualização.
pub trait RegionalDataProvider: Send + Sync {
    fn descriptor(&self) -> &'static ProviderDescriptor;
    /// Nome estável do dataset (não muda entre versões).
    fn dataset_name(&self) -> String;
}

/// Malha de setores censitários.
#[async_trait]
pub trait CensusSectorProvider: RegionalDataProvider {
    async fn sectors(
        &self,
        scope: &RegionalScope,
    ) -> Result<DatasetBatch<RawCensusSector>, CrawlError>;
}

/// Indicadores por setor censitário (demografia, vulnerabilidade).
#[async_trait]
pub trait DemographicDataProvider: RegionalDataProvider {
    /// `Demographics` ou `SocialVulnerability`.
    fn capability(&self) -> Capability;
    /// Qual indicador uma variável da fonte representa.
    fn indicator_for(&self, variable: &str) -> Option<SectorIndicator>;
    async fn sector_values(
        &self,
        scope: &RegionalScope,
    ) -> Result<DatasetBatch<RawSectorValue>, CrawlError>;
}

/// Equipamentos urbanos.
#[async_trait]
pub trait InfrastructureDataProvider: RegionalDataProvider {
    async fn services(
        &self,
        scope: &RegionalScope,
    ) -> Result<DatasetBatch<UrbanService>, CrawlError>;
}

/// Áreas de risco ambiental.
#[async_trait]
pub trait EnvironmentalRiskProvider: RegionalDataProvider {
    async fn risk_areas(
        &self,
        scope: &RegionalScope,
    ) -> Result<DatasetBatch<RawRiskArea>, CrawlError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_validates_codes() {
        assert_eq!(
            RegionalScope::parse("3548708").unwrap(),
            RegionalScope::Municipality("3548708".into())
        );
        assert_eq!(RegionalScope::parse("35").unwrap().prefix(), "35");
        assert!(RegionalScope::parse("35487").is_err());
        assert!(RegionalScope::parse("3548708' OR '1'='1").is_err());
        assert!(RegionalScope::parse("35")
            .unwrap()
            .contains("354870805000012"));
    }
}

//! Fundação Seade — Índice Paulista de Vulnerabilidade Social (IPVS),
//! versão 2022, por setor censitário.
//!
//! Publicado em <https://dadosabertos.sp.gov.br/dataset/seade-ipvs-versao-2022>
//! (CSV) e no repositório do Seade (shapefile zipado). O importador lê o CSV.
//! Os nomes das colunas não puderam ser conferidos no arquivo real: a coluna
//! do setor e a do grupo são detectadas pelo nome (ou informadas na CLI).

use std::path::PathBuf;

use async_trait::async_trait;
use chrono::Utc;
use crab_domain::regional::{
    provider_descriptor, Capability, DatasetBatch, DatasetProvenance, GeographicGranularity,
    ProviderDescriptor, RawSectorValue, SectorIndicator,
};
use crab_domain::DataSource;

use super::ibge::{census_reference_date, sector_values_from_table};
use super::raw::{dataset_version, file_name};
use super::table::{decode_text, parse_delimited, read_maybe_zipped};
use super::{DemographicDataProvider, RegionalDataProvider, RegionalScope};
use crate::CrawlError;

pub const DATASET_URL: &str = "https://dadosabertos.sp.gov.br/dataset/seade-ipvs-versao-2022";

pub struct SeadeIpvsProvider {
    path: PathBuf,
    sector_column: Option<String>,
    group_column: Option<String>,
}

impl SeadeIpvsProvider {
    pub fn new(
        path: impl Into<PathBuf>,
        sector_column: Option<String>,
        group_column: Option<String>,
    ) -> Self {
        Self {
            path: path.into(),
            sector_column,
            group_column,
        }
    }
}

/// Coluna do grupo: a informada, senão a primeira cujo nome contém "ipvs",
/// senão a primeira que contém "grupo".
pub fn detect_group_column(header: &[String], explicit: Option<&str>) -> Option<String> {
    if let Some(name) = explicit {
        return header
            .iter()
            .find(|h| h.eq_ignore_ascii_case(name))
            .cloned();
    }
    let lower = |h: &String| h.to_ascii_lowercase();
    header
        .iter()
        .find(|h| lower(h).contains("ipvs"))
        .or_else(|| header.iter().find(|h| lower(h).contains("grupo")))
        .cloned()
}

impl RegionalDataProvider for SeadeIpvsProvider {
    fn descriptor(&self) -> &'static ProviderDescriptor {
        provider_descriptor(DataSource::SeadeIpvs).expect("Seade registrado")
    }
    fn dataset_name(&self) -> String {
        "ipvs_2022".into()
    }
}

#[async_trait]
impl DemographicDataProvider for SeadeIpvsProvider {
    fn capability(&self) -> Capability {
        Capability::SocialVulnerability
    }

    fn indicator_for(&self, _variable: &str) -> Option<SectorIndicator> {
        // Só a coluna do grupo é lida (ver `sector_values`).
        Some(SectorIndicator::IpvsGroup)
    }

    async fn sector_values(
        &self,
        scope: &RegionalScope,
    ) -> Result<DatasetBatch<RawSectorValue>, CrawlError> {
        if !scope.prefix().starts_with("35") {
            return Err(CrawlError::Parse(
                "o IPVS só cobre o Estado de São Paulo (35)".into(),
            ));
        }
        let path = self.path.clone();
        let (bytes, _) = tokio::task::spawn_blocking(move || read_maybe_zipped(&path, "csv"))
            .await
            .map_err(|e| CrawlError::Parse(e.to_string()))??;
        let text = decode_text(&bytes);
        let header = parse_delimited(text.lines().next().unwrap_or(""), |_| false)?.header;
        let group =
            detect_group_column(&header, self.group_column.as_deref()).ok_or_else(|| {
                CrawlError::Parse(format!(
                    "coluna do grupo IPVS não encontrada (use --group-column); colunas: {header:?}"
                ))
            })?;
        let (records, report) =
            sector_values_from_table(&text, scope, self.sector_column.as_deref(), |h| h == group)?;
        let name = file_name(&self.path);
        Ok(DatasetBatch {
            provenance: DatasetProvenance {
                source: DataSource::SeadeIpvs,
                dataset_name: self.dataset_name(),
                dataset_version: dataset_version(&name, &bytes),
                source_url: Some(DATASET_URL.into()),
                reference_date: Some(census_reference_date()),
                collected_at: Utc::now(),
                granularity: GeographicGranularity::CensusSector,
            },
            coverage: vec![],
            records,
            report,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_group_column() {
        let h = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            detect_group_column(&h(&["cod_setor", "grupo", "IPVS_2022"]), None).as_deref(),
            Some("IPVS_2022")
        );
        assert_eq!(
            detect_group_column(&h(&["cod_setor", "Grupo"]), None).as_deref(),
            Some("Grupo")
        );
        assert_eq!(
            detect_group_column(&h(&["cod_setor", "v1"]), Some("V1")).as_deref(),
            Some("v1")
        );
        assert_eq!(detect_group_column(&h(&["cod_setor", "v1"]), None), None);
    }
}

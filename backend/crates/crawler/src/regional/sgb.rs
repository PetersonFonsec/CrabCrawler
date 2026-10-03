//! Serviço Geológico do Brasil (SGB) — Setorização de Risco.
//!
//! Serviço ArcGIS REST oficial, consultado por município:
//! `geoportal.sgb.gov.br/server/rest/services/gestaoterritorial/risco/MapServer/0/query`.
//! Campos confirmados em 2026-10-03: `cd_geocmu`, `num_setor`, `local`,
//! `tipolo_g1..5`, `tipolo_e1..5`, `cobrade_01..05`, `grau_risco`,
//! `grau_vulne`, `num_edif`, `num_domi`, `num_pess`, `data_setor`.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use crab_domain::regional::{
    provider_descriptor, DatasetBatch, DatasetProvenance, GeographicGranularity, ImportReport,
    ProviderDescriptor, RawGeometry, RawRiskArea, RawRiskTypology,
};
use crab_domain::DataSource;
use serde_json::Value;

use super::geojson::{parse_feature_collection, prop_str};
use super::raw::{dataset_version, file_name, RawStore};
use super::{EnvironmentalRiskProvider, RegionalDataProvider, RegionalScope};
use crate::http::HttpClient;
use crate::CrawlError;

pub const LAYER_URL: &str =
    "https://geoportal.sgb.gov.br/server/rest/services/gestaoterritorial/risco/MapServer/0";
const PAGE_SIZE: usize = 1000;

/// URL de consulta de uma página, em GeoJSON e WGS84.
pub fn query_url(municipality: &str, offset: usize) -> String {
    format!(
        "{LAYER_URL}/query?where=cd_geocmu%3D%27{municipality}%27&outFields=*\
         &returnGeometry=true&outSR=4326&orderByFields=objectid\
         &resultOffset={offset}&resultRecordCount={PAGE_SIZE}&f=geojson"
    )
}

pub enum SgbInput {
    /// GeoJSON já baixado (uma FeatureCollection).
    File(PathBuf),
    /// Consulta a API e grava o bruto.
    Api {
        http: Arc<HttpClient>,
        raw: RawStore,
    },
}

pub struct SgbRiskProvider {
    input: SgbInput,
}

impl SgbRiskProvider {
    pub fn new(input: SgbInput) -> Self {
        Self { input }
    }

    /// Baixa todas as páginas e junta numa FeatureCollection.
    async fn download(http: &HttpClient, municipality: &str) -> Result<Vec<u8>, CrawlError> {
        let mut features: Vec<Value> = Vec::new();
        loop {
            let page: Value = http
                .get_json(&query_url(municipality, features.len()))
                .await?;
            if let Some(err) = page.get("error") {
                return Err(CrawlError::Parse(format!("SGB devolveu erro: {err}")));
            }
            let batch = page
                .get("features")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let n = batch.len();
            features.extend(batch);
            if n < PAGE_SIZE {
                break;
            }
        }
        Ok(serde_json::to_vec(&serde_json::json!({
            "type": "FeatureCollection",
            "features": features,
        }))?)
    }
}

impl RegionalDataProvider for SgbRiskProvider {
    fn descriptor(&self) -> &'static ProviderDescriptor {
        provider_descriptor(DataSource::SgbRiskSectors).expect("SGB registrado")
    }
    fn dataset_name(&self) -> String {
        "setorizacao_risco".into()
    }
}

#[async_trait]
impl EnvironmentalRiskProvider for SgbRiskProvider {
    async fn risk_areas(
        &self,
        scope: &RegionalScope,
    ) -> Result<DatasetBatch<RawRiskArea>, CrawlError> {
        let (bytes, published_name, source_url) = match &self.input {
            SgbInput::File(path) => (
                tokio::fs::read(path).await?,
                file_name(path),
                Some(LAYER_URL.to_string()),
            ),
            SgbInput::Api { http, raw } => {
                let municipality = scope.municipality().ok_or_else(|| {
                    CrawlError::Parse("a API do SGB é consultada por município (7 dígitos)".into())
                })?;
                let bytes = Self::download(http, municipality).await?;
                let name = format!("setorizacao_risco_{municipality}.geojson");
                raw.save("sgb", &name, &bytes).await?;
                (bytes, name, Some(query_url(municipality, 0)))
            }
        };
        let (records, report) = parse_risk_areas(&bytes, scope)?;
        Ok(DatasetBatch {
            provenance: DatasetProvenance {
                source: DataSource::SgbRiskSectors,
                dataset_name: self.dataset_name(),
                dataset_version: dataset_version(&published_name, &bytes),
                source_url,
                reference_date: None,
                collected_at: Utc::now(),
                granularity: GeographicGranularity::Polygon,
            },
            coverage: vec![],
            records,
            report,
        })
    }
}

/// `data_setor` vem como epoch em milissegundos (ArcGIS) ou texto ISO.
fn parse_date(value: Option<&Value>) -> Option<NaiveDate> {
    match value? {
        Value::Number(n) => DateTime::from_timestamp_millis(n.as_i64()?).map(|d| d.date_naive()),
        Value::String(s) => NaiveDate::parse_from_str(s.get(..10)?, "%Y-%m-%d").ok(),
        _ => None,
    }
}

pub fn parse_risk_areas(
    bytes: &[u8],
    scope: &RegionalScope,
) -> Result<(Vec<RawRiskArea>, ImportReport), CrawlError> {
    let fc = parse_feature_collection(bytes)?;
    let mut report = ImportReport::default();
    let mut out = Vec::new();
    for f in fc.features {
        let p = &f.properties;
        let municipality = prop_str(p, "cd_geocmu");
        if !municipality.as_deref().is_some_and(|m| scope.contains(m)) {
            continue;
        }
        report.read += 1;
        let Some(external_id) = prop_str(p, "num_setor")
            .or_else(|| prop_str(p, "objectid"))
            .or(f.id.clone())
        else {
            report.skip("setor sem identificador");
            continue;
        };
        let typologies = (1..=5)
            .map(|i| RawRiskTypology {
                cobrade: prop_str(p, &format!("cobrade_{i:02}")),
                general: prop_str(p, &format!("tipolo_g{i}")),
                specific: prop_str(p, &format!("tipolo_e{i}")),
            })
            .filter(|t| t.cobrade.is_some() || t.general.is_some())
            .collect();
        out.push(RawRiskArea {
            external_id,
            municipality_ibge_code: municipality,
            location_name: prop_str(p, "local"),
            typologies,
            severity: prop_str(p, "grau_risco"),
            mapped_on: parse_date(
                p.iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case("data_setor"))
                    .map(|(_, v)| v),
            ),
            geometry: f.geometry.map(|json| RawGeometry::GeoJson {
                json,
                srid: fc.srid,
            }),
            attributes: Value::Object(f.properties),
        });
    }
    Ok((out, report))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sgb_features() {
        let json = br#"{"type":"FeatureCollection","features":[
          {"type":"Feature","id":1,"geometry":{"type":"Polygon","coordinates":[[[0,0],[1,0],[1,1],[0,0]]]},
           "properties":{"objectid":1,"cd_geocmu":"3548708","num_setor":"SP_SBC_SR_08_CPRM",
             "local":"Vila Baeta Neves","tipolo_g1":"Enxurrada","cobrade_01":"1.2.2.0.0",
             "tipolo_g2":"Deslizamento","tipolo_e2":"Deslizamento planar","cobrade_02":"1.1.3.2.1",
             "tipolo_g3":null,"grau_risco":"Alto","data_setor":1400025600000}},
          {"type":"Feature","id":2,"geometry":null,
           "properties":{"objectid":2,"cd_geocmu":"3547809","num_setor":"OUTRO"}}]}"#;
        let scope = RegionalScope::Municipality("3548708".into());
        let (areas, report) = parse_risk_areas(json, &scope).unwrap();
        assert_eq!(areas.len(), 1);
        assert_eq!(report.read, 1);
        let a = &areas[0];
        assert_eq!(a.external_id, "SP_SBC_SR_08_CPRM");
        assert_eq!(a.typologies.len(), 2);
        assert_eq!(a.severity.as_deref(), Some("Alto"));
        assert_eq!(a.mapped_on, NaiveDate::from_ymd_opt(2014, 5, 14));
        assert!(a.geometry.is_some());
    }

    #[test]
    fn query_url_filters_by_municipality() {
        let url = query_url("3548708", 1000);
        assert!(url.contains("cd_geocmu%3D%273548708%27"));
        assert!(url.contains("resultOffset=1000"));
        assert!(url.contains("f=geojson"));
    }
}

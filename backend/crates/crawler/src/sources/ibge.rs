use std::sync::Arc;

use async_trait::async_trait;
use chrono::{NaiveDate, Utc};
use crab_domain::{
    DataSource, Indicator, IndicatorKind, Provenance, Region, RegionIndicator, RegionLevel,
};
use serde::Deserialize;

use crate::http::HttpClient;
use crate::{CrawlError, IndicatorSource};

const BASE: &str = "https://servicodados.ibge.gov.br/api";

/// Cliente das APIs oficiais do IBGE (Localidades e Agregados/SIDRA).
pub struct IbgeClient {
    http: Arc<HttpClient>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Municipio {
    pub id: u32,
    pub nome: String,
}

impl IbgeClient {
    pub fn new(http: Arc<HttpClient>) -> Self {
        Self { http }
    }

    /// Municípios de uma UF (ex.: "SP"). Usado para resolver nome → código IBGE.
    pub async fn municipalities(&self, uf: &str) -> Result<Vec<Municipio>, CrawlError> {
        let url = format!("{BASE}/v1/localidades/estados/{uf}/municipios");
        self.http.get_json(&url).await
    }

    /// População residente do Censo 2022 (agregado 4709, variável 93).
    pub async fn population_2022(
        &self,
        municipality_code: &str,
    ) -> Result<Option<f64>, CrawlError> {
        let url = population_url(municipality_code);
        let body: Vec<Agregado> = self.http.get_json(&url).await?;
        Ok(parse_population(&body))
    }
}

fn population_url(code: &str) -> String {
    format!("{BASE}/v3/agregados/4709/periodos/2022/variaveis/93?localidades=N6[{code}]")
}

#[derive(Debug, Deserialize)]
struct Agregado {
    resultados: Vec<Resultado>,
}

#[derive(Debug, Deserialize)]
struct Resultado {
    series: Vec<Serie>,
}

#[derive(Debug, Deserialize)]
struct Serie {
    serie: std::collections::HashMap<String, String>,
}

fn parse_population(body: &[Agregado]) -> Option<f64> {
    body.first()?
        .resultados
        .first()?
        .series
        .first()?
        .serie
        .get("2022")?
        .parse()
        .ok()
}

#[async_trait]
impl IndicatorSource for IbgeClient {
    fn name(&self) -> &'static str {
        DataSource::IbgeLocalidades.as_str()
    }

    async fn fetch(
        &self,
        municipality_ibge_code: &str,
    ) -> Result<Vec<RegionIndicator>, CrawlError> {
        let Some(population) = self.population_2022(municipality_ibge_code).await? else {
            return Ok(vec![]);
        };
        let census_day = NaiveDate::from_ymd_opt(2022, 8, 1).expect("data válida");
        Ok(vec![RegionIndicator {
            region: Region {
                level: RegionLevel::Municipality,
                code: municipality_ibge_code.to_string(),
                name: municipality_ibge_code.to_string(),
                municipality_ibge_code: municipality_ibge_code.to_string(),
            },
            indicator: Indicator {
                kind: IndicatorKind::Population,
                value: population,
                period_start: census_day,
                period_end: census_day,
            },
            provenance: Provenance {
                source: DataSource::IbgeLocalidades,
                url: Some(population_url(municipality_ibge_code)),
                collected_at: Utc::now(),
            },
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sidra_population_payload() {
        let json = r#"[{"id":"93","variavel":"População residente","unidade":"Pessoas",
            "resultados":[{"classificacoes":[],"series":[{"localidade":{"id":"3548708",
            "nivel":{"id":"N6","nome":"Município"},"nome":"São Bernardo do Campo - SP"},
            "serie":{"2022":"810729"}}]}]}]"#;
        let body: Vec<Agregado> = serde_json::from_str(json).unwrap();
        assert_eq!(parse_population(&body), Some(810729.0));
    }
}

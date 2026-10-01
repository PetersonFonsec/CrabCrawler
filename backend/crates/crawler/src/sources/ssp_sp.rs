use std::path::PathBuf;

use async_trait::async_trait;
use chrono::{Datelike, NaiveDate, Utc};
use crab_domain::{
    DataSource, Indicator, IndicatorKind, Provenance, Region, RegionIndicator, RegionLevel,
};

use crate::{CrawlError, IndicatorSource};

/// Importador das estatísticas mensais de ocorrências da SSP-SP.
///
/// A SSP publica as ocorrências por delegacia e mês em
/// <https://www.ssp.sp.gov.br/estatistica>. No MVP o arquivo é baixado
/// manualmente e convertido para CSV (`;` como separador) com as colunas:
///
/// `municipio_ibge;delegacia;ano;mes;natureza;quantidade`
///
/// Automatizar o download é um passo seguinte; o parsing e o modelo já ficam
/// prontos aqui.
pub struct SspSpCsvSource {
    path: PathBuf,
}

impl SspSpCsvSource {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

#[async_trait]
impl IndicatorSource for SspSpCsvSource {
    fn name(&self) -> &'static str {
        DataSource::SspSp.as_str()
    }

    async fn fetch(
        &self,
        municipality_ibge_code: &str,
    ) -> Result<Vec<RegionIndicator>, CrawlError> {
        let content = tokio::fs::read_to_string(&self.path).await?;
        let rows = parse_csv(&content)?;
        let url = self.path.display().to_string();
        Ok(rows
            .into_iter()
            .filter(|row| row.municipality_ibge_code == municipality_ibge_code)
            .map(|row| row.into_indicator(&url))
            .collect())
    }
}

#[derive(Debug, PartialEq)]
struct Row {
    municipality_ibge_code: String,
    police_district: String,
    month_start: NaiveDate,
    kind: IndicatorKind,
    count: f64,
}

impl Row {
    fn into_indicator(self, url: &str) -> RegionIndicator {
        let month_end = last_day_of_month(self.month_start);
        RegionIndicator {
            region: Region {
                level: RegionLevel::PoliceDistrict,
                code: self.police_district.clone(),
                name: self.police_district,
                municipality_ibge_code: self.municipality_ibge_code,
            },
            indicator: Indicator {
                kind: self.kind,
                value: self.count,
                period_start: self.month_start,
                period_end: month_end,
            },
            provenance: Provenance {
                source: DataSource::SspSp,
                url: Some(url.to_string()),
                collected_at: Utc::now(),
            },
        }
    }
}

fn parse_csv(content: &str) -> Result<Vec<Row>, CrawlError> {
    let mut rows = Vec::new();
    for (line_no, line) in content.lines().enumerate().skip(1) {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = line.split(';').map(str::trim).collect();
        let err = |msg: &str| CrawlError::Parse(format!("linha {}: {msg}", line_no + 1));
        let [ibge, district, year, month, nature, count] = cols[..] else {
            return Err(err("esperadas 6 colunas"));
        };
        let Some(kind) = map_nature(nature) else {
            tracing::debug!(nature, "natureza ignorada");
            continue;
        };
        let year: i32 = year.parse().map_err(|_| err("ano inválido"))?;
        let month: u32 = month.parse().map_err(|_| err("mês inválido"))?;
        rows.push(Row {
            municipality_ibge_code: ibge.to_string(),
            police_district: district.to_string(),
            month_start: NaiveDate::from_ymd_opt(year, month, 1)
                .ok_or_else(|| err("data inválida"))?,
            kind,
            count: count.parse().map_err(|_| err("quantidade inválida"))?,
        });
    }
    Ok(rows)
}

/// Mapeia a "natureza" da SSP para os indicadores do domínio.
fn map_nature(nature: &str) -> Option<IndicatorKind> {
    match nature.to_uppercase().as_str() {
        "ROUBO - OUTROS" | "ROUBO" => Some(IndicatorKind::Robberies),
        "FURTO - OUTROS" | "FURTO" => Some(IndicatorKind::Thefts),
        "ROUBO DE VEÍCULO" | "ROUBO DE VEICULO" => Some(IndicatorKind::VehicleRobberies),
        "HOMICÍDIO DOLOSO" | "HOMICIDIO DOLOSO" => Some(IndicatorKind::Homicides),
        _ => None,
    }
}

fn last_day_of_month(first: NaiveDate) -> NaiveDate {
    let (y, m) = if first.month() == 12 {
        (first.year() + 1, 1)
    } else {
        (first.year(), first.month() + 1)
    };
    NaiveDate::from_ymd_opt(y, m, 1)
        .expect("data válida")
        .pred_opt()
        .expect("data válida")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rows_and_skips_unknown_natures() {
        let csv = "municipio_ibge;delegacia;ano;mes;natureza;quantidade\n\
                   3548708;01 DP S.BERNARDO DO CAMPO;2026;3;ROUBO - OUTROS;42\n\
                   3548708;01 DP S.BERNARDO DO CAMPO;2026;3;LESÃO CORPORAL;7\n";
        let rows = parse_csv(csv).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, IndicatorKind::Robberies);
        assert_eq!(rows[0].count, 42.0);
    }

    #[test]
    fn computes_month_end() {
        let feb = NaiveDate::from_ymd_opt(2024, 2, 1).unwrap();
        assert_eq!(
            last_day_of_month(feb),
            NaiveDate::from_ymd_opt(2024, 2, 29).unwrap()
        );
    }
}

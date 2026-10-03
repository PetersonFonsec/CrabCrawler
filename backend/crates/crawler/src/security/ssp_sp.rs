use std::path::PathBuf;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use crab_domain::{DataSource, RawCrimeRecord};

use super::{SecurityDataProvider, SecurityScope};
use crate::CrawlError;

/// Importador dos "Dados Mensais" da SSP-SP (Resolução SSP 160/2001).
///
/// A SSP publica ocorrências por natureza e mês, por município e por
/// delegacia, em <https://www.ssp.sp.gov.br/estatistica/dados-mensais>. A
/// página não oferece API pública documentada, então a tabela é exportada
/// manualmente e convertida para CSV (`;` como separador):
///
/// `municipio_ibge;delegacia;ano;mes;natureza;quantidade`
///
/// `delegacia` vazia significa total do município. `natureza` é mantida
/// exatamente como na SSP (ex.: `Nº DE VÍTIMAS EM HOMICÍDIO DOLOSO (3)`); o
/// normalizador decide tipo e unidade.
pub struct SspSpCsvProvider {
    path: PathBuf,
}

impl SspSpCsvProvider {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

#[async_trait]
impl SecurityDataProvider for SspSpCsvProvider {
    fn source(&self) -> DataSource {
        DataSource::SspSp
    }

    async fn fetch(&self, scope: &SecurityScope) -> Result<Vec<RawCrimeRecord>, CrawlError> {
        if scope.state != "SP" {
            return Ok(vec![]);
        }
        let content = tokio::fs::read_to_string(&self.path).await?;
        let version = self
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned());
        let records = parse_csv(
            &content,
            &self.path.display().to_string(),
            version,
            Utc::now(),
        )?;
        let total = records.len();
        let records: Vec<_> = records
            .into_iter()
            .filter(|r| match &scope.municipality_ibge_code {
                Some(code) => r.municipality_ibge_code.as_deref() == Some(code),
                None => true,
            })
            .collect();
        tracing::info!(
            file = %self.path.display(),
            read = total,
            kept = records.len(),
            "SSP-SP: CSV lido"
        );
        Ok(records)
    }
}

const HEADER: [&str; 6] = [
    "municipio_ibge",
    "delegacia",
    "ano",
    "mes",
    "natureza",
    "quantidade",
];

pub(crate) fn parse_csv(
    content: &str,
    source_url: &str,
    dataset_version: Option<String>,
    collected_at: DateTime<Utc>,
) -> Result<Vec<RawCrimeRecord>, CrawlError> {
    let mut lines = content.lines().enumerate();
    let header: Vec<String> = lines
        .next()
        .map(|(_, h)| {
            h.trim_start_matches('\u{feff}')
                .split(';')
                .map(|c| c.trim().to_lowercase())
                .collect()
        })
        .unwrap_or_default();
    if header != HEADER {
        return Err(CrawlError::Parse(format!(
            "cabeçalho inesperado; esperado `{}`",
            HEADER.join(";")
        )));
    }

    let mut records = Vec::new();
    for (line_no, line) in lines {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let err = |msg: &str| CrawlError::Parse(format!("linha {}: {msg}", line_no + 1));
        let cols: Vec<&str> = line.split(';').map(str::trim).collect();
        let [ibge, district, year, month, nature, count] = cols[..] else {
            return Err(err("esperadas 6 colunas"));
        };
        if ibge.len() != 7 || !ibge.bytes().all(|b| b.is_ascii_digit()) {
            return Err(err("código IBGE do município inválido"));
        }
        let month: u32 = month.parse().map_err(|_| err("mês inválido"))?;
        if !(1..=12).contains(&month) {
            return Err(err("mês fora de 1..12"));
        }
        let value: f64 = count
            .replace('.', "")
            .parse()
            .map_err(|_| err("quantidade inválida"))?;
        records.push(RawCrimeRecord {
            source: DataSource::SspSp,
            state: "SP".into(),
            municipality_name: None,
            municipality_ibge_code: Some(ibge.to_string()),
            police_unit: Some(district.to_string()).filter(|d| !d.is_empty()),
            year: year.parse().map_err(|_| err("ano inválido"))?,
            month,
            label: nature.to_string(),
            value,
            counting_unit: None,
            source_url: Some(source_url.to_string()),
            dataset_version: dataset_version.clone(),
            collected_at,
        });
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(csv: &str) -> Result<Vec<RawCrimeRecord>, CrawlError> {
        parse_csv(csv, "fixtures/x.csv", Some("x.csv".into()), Utc::now())
    }

    #[test]
    fn keeps_original_label_and_district() {
        let csv = "municipio_ibge;delegacia;ano;mes;natureza;quantidade\n\
                   3548708;01 DP S.BERNARDO DO CAMPO;2026;3;ROUBO - OUTROS;42\n\
                   3548708;;2026;3;Nº DE VÍTIMAS EM HOMICÍDIO DOLOSO (3);1.204\n";
        let rows = parse(csv).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].label, "ROUBO - OUTROS");
        assert_eq!(
            rows[0].police_unit.as_deref(),
            Some("01 DP S.BERNARDO DO CAMPO")
        );
        assert_eq!(rows[1].police_unit, None);
        assert_eq!(rows[1].value, 1204.0);
        assert_eq!(rows[1].dataset_version.as_deref(), Some("x.csv"));
    }

    #[test]
    fn rejects_bad_header_and_bad_rows() {
        assert!(parse("a;b;c\n").is_err());
        let bad_month = "municipio_ibge;delegacia;ano;mes;natureza;quantidade\n\
                         3548708;;2026;13;FURTO - OUTROS;1\n";
        let err = parse(bad_month).unwrap_err().to_string();
        assert!(err.contains("linha 2"), "{err}");
        let bad_ibge = "municipio_ibge;delegacia;ano;mes;natureza;quantidade\n\
                        SBC;;2026;1;FURTO - OUTROS;1\n";
        assert!(parse(bad_ibge).is_err());
    }
}

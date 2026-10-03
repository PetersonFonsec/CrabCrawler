use std::collections::HashMap;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use calamine::{Data, Reader};
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use crab_domain::{CountingUnit, DataSource, RawCrimeRecord};

use super::{SecurityDataProvider, SecurityScope};
use crate::http::HttpClient;
use crate::CrawlError;

/// URL oficial de download da base anual do Sinesp VDE no gov.br.
///
/// Verificada em 2026-10 para `bancovde-2025.xlsx`. O antigo CKAN
/// (`dados.mj.gov.br`) não responde mais.
pub fn vde_download_url(year: i32) -> String {
    format!(
        "https://www.gov.br/mj/pt-br/assuntos/sua-seguranca/seguranca-publica/estatistica/\
         download/dnsp-base-de-dados/bancovde-{year}.xlsx/@@download/file"
    )
}

/// Onde buscar a planilha.
pub enum VdeInput {
    /// Arquivo já baixado.
    File(PathBuf),
    /// Download direto do gov.br para um ano.
    Download { year: i32, http: Arc<HttpClient> },
}

/// Importador do Sinesp VDE (Dados Nacionais de Segurança Pública, MJSP).
///
/// Colunas esperadas (cabeçalho da primeira aba, sem diferenciar maiúsculas):
/// `uf, municipio, evento, data_referencia, ..., total_vitima, total`.
///
/// - Linhas sem município são agregados estaduais e são ignoradas: o modelo
///   de região exige um município, e dado estadual não é atribuído a cidade.
/// - Quando `total_vitima` está preenchido o registro conta vítimas; senão
///   usa `total` como ocorrências. Linhas sem nenhum dos dois são puladas
///   (vazio ≠ zero na fonte).
/// - A fonte não traz código IBGE: o normalizador casa pelo nome.
pub struct SinespVdeProvider {
    input: VdeInput,
}

impl SinespVdeProvider {
    pub fn new(input: VdeInput) -> Self {
        Self { input }
    }

    async fn load(&self) -> Result<(Vec<u8>, String, String), CrawlError> {
        match &self.input {
            VdeInput::File(path) => {
                let bytes = tokio::fs::read(path).await?;
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string());
                Ok((bytes, path.display().to_string(), name))
            }
            VdeInput::Download { year, http } => {
                let url = vde_download_url(*year);
                tracing::info!(%url, "Sinesp VDE: baixando planilha");
                let bytes = http.get_bytes(&url).await?;
                Ok((bytes, url, format!("bancovde-{year}.xlsx")))
            }
        }
    }
}

#[async_trait]
impl SecurityDataProvider for SinespVdeProvider {
    fn source(&self) -> DataSource {
        DataSource::SinespVde
    }

    async fn fetch(&self, scope: &SecurityScope) -> Result<Vec<RawCrimeRecord>, CrawlError> {
        let (bytes, source_url, version) = self.load().await?;
        let rows = tokio::task::spawn_blocking(move || read_first_sheet(bytes))
            .await
            .map_err(|e| CrawlError::Parse(format!("leitura da planilha: {e}")))??;
        let parsed = parse_rows(rows, &source_url, &version, Utc::now())?;
        let records: Vec<_> = parsed
            .records
            .into_iter()
            .filter(|r| r.state == scope.state)
            .collect();
        tracing::info!(
            %source_url,
            rows = parsed.total_rows,
            state_level_skipped = parsed.state_level_rows,
            empty_skipped = parsed.empty_rows,
            kept = records.len(),
            "Sinesp VDE: planilha lida"
        );
        Ok(records)
    }
}

/// Célula já desacoplada do leitor de planilhas (facilita testes).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Cell {
    Empty,
    Text(String),
    Number(f64),
    #[cfg(test)]
    Date(NaiveDate),
}

impl From<&Data> for Cell {
    fn from(value: &Data) -> Self {
        match value {
            Data::Empty | Data::Error(_) => Cell::Empty,
            Data::String(s) if s.trim().is_empty() => Cell::Empty,
            Data::String(s) | Data::DateTimeIso(s) | Data::DurationIso(s) => {
                Cell::Text(s.trim().to_string())
            }
            Data::Float(f) => Cell::Number(*f),
            Data::Int(i) => Cell::Number(*i as f64),
            Data::Bool(b) => Cell::Text(b.to_string()),
            // Datas do Excel são números de série; convertidos em `date()`.
            Data::DateTime(dt) => Cell::Number(dt.as_f64()),
        }
    }
}

fn read_first_sheet(bytes: Vec<u8>) -> Result<Vec<Vec<Cell>>, CrawlError> {
    let mut workbook = calamine::open_workbook_auto_from_rs(Cursor::new(bytes))
        .map_err(|e| CrawlError::Parse(format!("planilha inválida: {e}")))?;
    let range = workbook
        .worksheet_range_at(0)
        .ok_or_else(|| CrawlError::Parse("planilha sem abas".into()))?
        .map_err(|e| CrawlError::Parse(format!("aba inválida: {e}")))?;
    Ok(range
        .rows()
        .map(|row| row.iter().map(Cell::from).collect())
        .collect())
}

#[derive(Debug)]
pub(crate) struct ParsedVde {
    pub records: Vec<RawCrimeRecord>,
    pub total_rows: usize,
    pub state_level_rows: usize,
    pub empty_rows: usize,
}

pub(crate) fn parse_rows(
    rows: Vec<Vec<Cell>>,
    source_url: &str,
    version: &str,
    collected_at: DateTime<Utc>,
) -> Result<ParsedVde, CrawlError> {
    let mut iter = rows.into_iter().enumerate();
    let (_, header) = iter
        .next()
        .ok_or_else(|| CrawlError::Parse("planilha vazia".into()))?;
    let index: HashMap<String, usize> = header
        .iter()
        .enumerate()
        .filter_map(|(i, c)| match c {
            Cell::Text(t) => Some((t.to_lowercase(), i)),
            _ => None,
        })
        .collect();
    let col = |name: &str| {
        index
            .get(name)
            .copied()
            .ok_or_else(|| CrawlError::Parse(format!("coluna `{name}` ausente")))
    };
    let (c_uf, c_mun, c_event, c_date) = (
        col("uf")?,
        col("municipio")?,
        col("evento")?,
        col("data_referencia")?,
    );
    let c_victims = index.get("total_vitima").copied();
    let c_total = col("total")?;

    let mut parsed = ParsedVde {
        records: Vec::new(),
        total_rows: 0,
        state_level_rows: 0,
        empty_rows: 0,
    };
    for (row_no, row) in iter {
        let get = |i: usize| row.get(i).unwrap_or(&Cell::Empty);
        if row.iter().all(|c| *c == Cell::Empty) {
            continue;
        }
        parsed.total_rows += 1;
        let err = |msg: &str| CrawlError::Parse(format!("linha {}: {msg}", row_no + 1));

        let Some(municipality) = text(get(c_mun)) else {
            parsed.state_level_rows += 1;
            continue;
        };
        let state = text(get(c_uf)).ok_or_else(|| err("UF vazia"))?;
        let event = text(get(c_event)).ok_or_else(|| err("evento vazio"))?;
        let date = date(get(c_date)).ok_or_else(|| err("data_referencia inválida"))?;

        let victims = c_victims.and_then(|i| number(get(i)));
        let (value, unit) = match (victims, number(get(c_total))) {
            (Some(v), _) => (v, CountingUnit::Victims),
            (None, Some(t)) => (t, CountingUnit::Occurrences),
            (None, None) => {
                parsed.empty_rows += 1;
                continue;
            }
        };

        parsed.records.push(RawCrimeRecord {
            source: DataSource::SinespVde,
            state: uf_sigla(&state),
            municipality_name: Some(municipality),
            municipality_ibge_code: None,
            police_unit: None,
            year: date.year(),
            month: date.month(),
            label: event,
            value,
            counting_unit: Some(unit),
            source_url: Some(source_url.to_string()),
            dataset_version: Some(version.to_string()),
            collected_at,
        });
    }
    Ok(parsed)
}

fn text(cell: &Cell) -> Option<String> {
    match cell {
        Cell::Text(t) if !t.is_empty() => Some(t.clone()),
        _ => None,
    }
}

fn number(cell: &Cell) -> Option<f64> {
    match cell {
        Cell::Number(n) => Some(*n),
        Cell::Text(t) => t.replace('.', "").replace(',', ".").parse().ok(),
        _ => None,
    }
}

/// Aceita data nativa da planilha, `AAAA-MM-DD` ou `DD/MM/AAAA`.
fn date(cell: &Cell) -> Option<NaiveDate> {
    match cell {
        #[cfg(test)]
        Cell::Date(d) => Some(*d),
        Cell::Text(t) => {
            let t = t.get(..10).unwrap_or(t);
            NaiveDate::parse_from_str(t, "%Y-%m-%d")
                .or_else(|_| NaiveDate::parse_from_str(t, "%d/%m/%Y"))
                .ok()
        }
        Cell::Number(serial) => excel_serial_to_date(*serial),
        Cell::Empty => None,
    }
}

fn excel_serial_to_date(serial: f64) -> Option<NaiveDate> {
    let base = NaiveDate::from_ymd_opt(1899, 12, 30)?;
    base.checked_add_days(chrono::Days::new(serial.trunc() as u64))
}

/// A planilha usa a sigla da UF; aceitamos também o nome por extenso de SP.
fn uf_sigla(value: &str) -> String {
    let upper = value.trim().to_uppercase();
    match upper.as_str() {
        "SÃO PAULO" | "SAO PAULO" => "SP".into(),
        _ => upper,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> Cell {
        Cell::Text(s.into())
    }

    fn header() -> Vec<Cell> {
        [
            "uf",
            "municipio",
            "evento",
            "data_referencia",
            "agente",
            "arma",
            "faixa_etaria",
            "feminino",
            "masculino",
            "nao_informado",
            "total_vitima",
            "total",
            "total_peso",
            "abrangencia",
            "formulario",
        ]
        .into_iter()
        .map(t)
        .collect()
    }

    fn row(uf: &str, mun: &str, event: &str, date: Cell, victims: Cell, total: Cell) -> Vec<Cell> {
        let mut r = vec![Cell::Empty; 15];
        r[0] = t(uf);
        r[1] = if mun.is_empty() { Cell::Empty } else { t(mun) };
        r[2] = t(event);
        r[3] = date;
        r[10] = victims;
        r[11] = total;
        r
    }

    #[test]
    fn parses_municipal_rows_and_counting_units() {
        let d = Cell::Date(NaiveDate::from_ymd_opt(2025, 3, 1).unwrap());
        let rows = vec![
            header(),
            row(
                "SP",
                "SÃO BERNARDO DO CAMPO",
                "Homicídio doloso",
                d.clone(),
                Cell::Number(3.0),
                Cell::Number(3.0),
            ),
            row(
                "SP",
                "SÃO BERNARDO DO CAMPO",
                "Roubo de veículo",
                t("2025-03-01"),
                Cell::Empty,
                Cell::Number(41.0),
            ),
            row(
                "SP",
                "",
                "Furto de veículo",
                d.clone(),
                Cell::Empty,
                Cell::Number(9000.0),
            ),
            row(
                "SP",
                "DIADEMA",
                "Estupro",
                t("01/03/2025"),
                Cell::Empty,
                Cell::Empty,
            ),
        ];
        let parsed = parse_rows(rows, "u", "bancovde-2025.xlsx", Utc::now()).unwrap();
        assert_eq!(parsed.total_rows, 4);
        assert_eq!(parsed.state_level_rows, 1);
        assert_eq!(parsed.empty_rows, 1);
        assert_eq!(parsed.records.len(), 2);
        let homicide = &parsed.records[0];
        assert_eq!(homicide.counting_unit, Some(CountingUnit::Victims));
        assert_eq!((homicide.year, homicide.month), (2025, 3));
        let vehicle = &parsed.records[1];
        assert_eq!(vehicle.counting_unit, Some(CountingUnit::Occurrences));
        assert_eq!(vehicle.value, 41.0);
        assert_eq!(vehicle.label, "Roubo de veículo");
    }

    #[test]
    fn missing_required_column_is_an_error() {
        let rows = vec![vec![t("uf"), t("municipio")]];
        let err = parse_rows(rows, "u", "v", Utc::now()).unwrap_err();
        assert!(err.to_string().contains("evento"));
    }

    #[test]
    fn download_url_follows_gov_br_pattern() {
        assert!(vde_download_url(2025).ends_with("bancovde-2025.xlsx/@@download/file"));
    }

    #[test]
    fn excel_serial_dates() {
        assert_eq!(
            excel_serial_to_date(45658.0),
            NaiveDate::from_ymd_opt(2025, 1, 1)
        );
    }
}

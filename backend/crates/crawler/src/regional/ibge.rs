//! IBGE — Censo Demográfico 2022 por setor censitário.
//!
//! - Malha: `SP_setores_CD2022.gpkg` (GeoPackage por UF) em
//!   <https://geoftp.ibge.gov.br/organizacao_do_territorio/malhas_territoriais/malhas_de_setores_censitarios__divisoes_intramunicipais/censo_2022/setores/gpkg/UF/>.
//! - Agregados, arquivo Básico: `Agregados_por_setores_basico_BR_<data>.zip`
//!   em <https://ftp.ibge.gov.br/Censos/Censo_Demografico_2022/Agregados_por_Setores_Censitarios/Agregados_por_Setor_csv/>.
//!
//! Os arquivos são grandes (170 MB e 15 MB); o download é manual e o
//! importador lê o arquivo local.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use chrono::{NaiveDate, Utc};
use crab_domain::regional::{
    provider_descriptor, Capability, DatasetBatch, DatasetProvenance, GeographicGranularity,
    ImportReport, ProviderDescriptor, RawCensusSector, RawGeometry, RawSectorValue,
    SectorIndicator,
};
use crab_domain::DataSource;
use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, OpenFlags};

use super::raw::{dataset_version, file_name};
use super::table::{decode_text, parse_delimited, read_maybe_zipped};
use super::{CensusSectorProvider, DemographicDataProvider, RegionalDataProvider, RegionalScope};
use crate::CrawlError;

pub const MESH_URL: &str = "https://geoftp.ibge.gov.br/organizacao_do_territorio/malhas_territoriais/malhas_de_setores_censitarios__divisoes_intramunicipais/censo_2022/setores/gpkg/UF/";
pub const AGGREGATES_URL: &str = "https://ftp.ibge.gov.br/Censos/Censo_Demografico_2022/Agregados_por_Setores_Censitarios/Agregados_por_Setor_csv/";

/// Data de referência do Censo 2022.
pub fn census_reference_date() -> NaiveDate {
    NaiveDate::from_ymd_opt(2022, 8, 1).expect("data válida")
}

fn descriptor() -> &'static ProviderDescriptor {
    provider_descriptor(DataSource::IbgeCensoSetores).expect("IBGE registrado")
}

fn sqlite_err(e: rusqlite::Error) -> CrawlError {
    CrawlError::Parse(format!("GeoPackage: {e}"))
}

/// Malha de setores censitários (GeoPackage).
pub struct IbgeSectorMeshProvider {
    path: PathBuf,
}

impl IbgeSectorMeshProvider {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

impl RegionalDataProvider for IbgeSectorMeshProvider {
    fn descriptor(&self) -> &'static ProviderDescriptor {
        descriptor()
    }
    fn dataset_name(&self) -> String {
        "malha_setores_censitarios_2022".into()
    }
}

#[async_trait]
impl CensusSectorProvider for IbgeSectorMeshProvider {
    async fn sectors(
        &self,
        scope: &RegionalScope,
    ) -> Result<DatasetBatch<RawCensusSector>, CrawlError> {
        let path = self.path.clone();
        let prefix = scope.prefix().to_string();
        let (records, report) =
            tokio::task::spawn_blocking(move || read_gpkg_sectors(&path, &prefix))
                .await
                .map_err(|e| CrawlError::Parse(e.to_string()))??;
        let bytes_for_version = file_fingerprint(&self.path)?;
        let name = file_name(&self.path);
        Ok(DatasetBatch {
            provenance: DatasetProvenance {
                source: DataSource::IbgeCensoSetores,
                dataset_name: self.dataset_name(),
                dataset_version: dataset_version(&name, &bytes_for_version),
                source_url: Some(published_url(MESH_URL, &name, "_setores_CD2022.gpkg")),
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

/// URL do arquivo quando o nome segue o padrão publicado pelo IBGE; senão,
/// só a pasta oficial (um arquivo renomeado não tem URL própria).
fn published_url(base: &str, name: &str, pattern: &str) -> String {
    if name.ends_with(pattern) {
        // Malha: <UF>_setores_CD2022.gpkg fica em <base><UF>/.
        let uf = name.split('_').next().unwrap_or("");
        format!("{base}{uf}/{name}")
    } else if name.starts_with(pattern) {
        format!("{base}{name}")
    } else {
        base.to_string()
    }
}

/// Impressão digital do arquivo para a versão: tamanho + primeiro e último
/// MB. Ler 170 MB inteiros só para o hash não vale a pena; mudanças do IBGE
/// mudam o tamanho ou o conteúdo das pontas (cabeçalho do SQLite).
fn file_fingerprint(path: &Path) -> Result<Vec<u8>, CrawlError> {
    use std::io::{Read, Seek, SeekFrom};
    const CHUNK: u64 = 1 << 20;
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    let mut buf = len.to_le_bytes().to_vec();
    let mut head = Vec::new();
    (&mut file).take(CHUNK).read_to_end(&mut head)?;
    buf.extend(head);
    if len > CHUNK {
        file.seek(SeekFrom::Start(len.saturating_sub(CHUNK)))?;
        file.read_to_end(&mut buf)?;
    }
    Ok(buf)
}

/// Lê a camada de feições do GeoPackage. Tabela, coluna de geometria e SRID
/// vêm das tabelas de metadados do padrão (`gpkg_contents`,
/// `gpkg_geometry_columns`, `gpkg_spatial_ref_sys`); colunas de atributos
/// são procuradas pelo nome do dicionário do IBGE (`CD_SETOR`, `NM_MUN`,
/// `AREA_KM2`), sem diferenciar maiúsculas.
pub fn read_gpkg_sectors(
    path: &Path,
    code_prefix: &str,
) -> Result<(Vec<RawCensusSector>, ImportReport), CrawlError> {
    let conn =
        Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(sqlite_err)?;
    let mut tables: Vec<(String, String, i64)> = conn
        .prepare(
            "SELECT c.table_name, g.column_name, g.srs_id FROM gpkg_contents c \
             JOIN gpkg_geometry_columns g ON g.table_name = c.table_name \
             WHERE c.data_type = 'features'",
        )
        .map_err(sqlite_err)?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .map_err(sqlite_err)?
        .collect::<Result<_, _>>()
        .map_err(sqlite_err)?;
    if tables.len() != 1 {
        return Err(CrawlError::Parse(format!(
            "esperada uma camada de feições no GeoPackage, encontradas {}",
            tables.len()
        )));
    }
    let (table, geom_col, srs_id) = tables.remove(0);
    let srid: i64 = conn
        .query_row(
            "SELECT CASE WHEN upper(organization) = 'EPSG' THEN organization_coordsys_id \
             ELSE srs_id END FROM gpkg_spatial_ref_sys WHERE srs_id = ?1",
            [srs_id],
            |r| r.get(0),
        )
        .unwrap_or(srs_id);

    let columns: Vec<String> = conn
        .prepare(&format!("PRAGMA table_info({})", quote_ident(&table)))
        .map_err(sqlite_err)?
        .query_map([], |r| r.get::<_, String>(1))
        .map_err(sqlite_err)?
        .collect::<Result<_, _>>()
        .map_err(sqlite_err)?;
    let find = |name: &str| {
        columns
            .iter()
            .find(|c| c.eq_ignore_ascii_case(name))
            .cloned()
    };
    let code_col = find("CD_SETOR").ok_or_else(|| {
        CrawlError::Parse(format!(
            "coluna CD_SETOR não encontrada; colunas: {columns:?}"
        ))
    })?;
    let opt = |c: Option<String>| c.map(|c| quote_ident(&c)).unwrap_or_else(|| "NULL".into());
    let sql = format!(
        "SELECT CAST({code} AS TEXT), {name}, {area}, {geom} FROM {table} \
         WHERE CAST({code} AS TEXT) LIKE ?1",
        code = quote_ident(&code_col),
        name = opt(find("NM_MUN")),
        area = opt(find("AREA_KM2")),
        geom = quote_ident(&geom_col),
        table = quote_ident(&table),
    );
    let mut stmt = conn.prepare(&sql).map_err(sqlite_err)?;
    let mut rows = stmt
        .query([format!("{code_prefix}%")])
        .map_err(sqlite_err)?;
    let mut report = ImportReport::default();
    let mut out = Vec::new();
    while let Some(row) = rows.next().map_err(sqlite_err)? {
        report.read += 1;
        let code: String = row
            .get::<_, Option<String>>(0)
            .map_err(sqlite_err)?
            .unwrap_or_default();
        let municipality_name = match row.get::<_, SqlValue>(1).map_err(sqlite_err)? {
            SqlValue::Text(s) => Some(s),
            _ => None,
        };
        let area_km2 = match row.get::<_, SqlValue>(2).map_err(sqlite_err)? {
            SqlValue::Real(v) => Some(v),
            SqlValue::Integer(v) => Some(v as f64),
            SqlValue::Text(s) => s.replace(',', ".").parse().ok(),
            _ => None,
        };
        let blob = match row.get::<_, SqlValue>(3).map_err(sqlite_err)? {
            SqlValue::Blob(b) => b,
            _ => {
                report.skip(format!("setor {code}: sem geometria"));
                continue;
            }
        };
        match gpkg_to_wkb(&blob) {
            Some(wkb) => out.push(RawCensusSector {
                code,
                municipality_name,
                area_km2,
                geometry: RawGeometry::Wkb {
                    bytes: wkb.to_vec(),
                    srid: srid as i32,
                },
            }),
            None => report.skip(format!(
                "setor {code}: geometria GeoPackage inválida ou vazia"
            )),
        }
    }
    Ok((out, report))
}

fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Remove o cabeçalho GeoPackage (`GP`, versão, flags, srs_id, envelope)
/// e devolve o WKB. `None` para geometria vazia ou cabeçalho inválido.
pub fn gpkg_to_wkb(blob: &[u8]) -> Option<&[u8]> {
    if blob.len() < 8 || &blob[..2] != b"GP" {
        return None;
    }
    let flags = blob[3];
    let empty = (flags >> 4) & 1 == 1;
    let envelope = match (flags >> 1) & 0b111 {
        0 => 0,
        1 => 32,
        2 | 3 => 48,
        4 => 64,
        _ => return None,
    };
    let start = 8 + envelope;
    if empty || blob.len() <= start {
        return None;
    }
    Some(&blob[start..])
}

/// Arquivo Básico dos Agregados por Setores (CSV, opcionalmente zipado).
pub struct IbgeBasicAggregatesProvider {
    path: PathBuf,
}

impl IbgeBasicAggregatesProvider {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

impl RegionalDataProvider for IbgeBasicAggregatesProvider {
    fn descriptor(&self) -> &'static ProviderDescriptor {
        descriptor()
    }
    fn dataset_name(&self) -> String {
        "agregados_por_setores_basico".into()
    }
}

#[async_trait]
impl DemographicDataProvider for IbgeBasicAggregatesProvider {
    fn capability(&self) -> Capability {
        Capability::Demographics
    }

    fn indicator_for(&self, variable: &str) -> Option<SectorIndicator> {
        SectorIndicator::from_ibge_basic(variable)
    }

    async fn sector_values(
        &self,
        scope: &RegionalScope,
    ) -> Result<DatasetBatch<RawSectorValue>, CrawlError> {
        let path = self.path.clone();
        let (bytes, _) = tokio::task::spawn_blocking(move || read_maybe_zipped(&path, "csv"))
            .await
            .map_err(|e| CrawlError::Parse(e.to_string()))??;
        let text = decode_text(&bytes);
        let (records, report) = sector_values_from_table(&text, scope, None, |h| {
            h.len() == 5
                && h[..1].eq_ignore_ascii_case("V")
                && h[1..].chars().all(|c| c.is_ascii_digit())
        })?;
        let name = file_name(&self.path);
        Ok(DatasetBatch {
            provenance: DatasetProvenance {
                source: DataSource::IbgeCensoSetores,
                dataset_name: self.dataset_name(),
                dataset_version: dataset_version(&name, &bytes),
                source_url: Some(published_url(
                    AGGREGATES_URL,
                    &name,
                    "Agregados_por_setores_",
                )),
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

/// Transforma uma tabela "um setor por linha" em valores por variável.
/// `sector_column`: nome explícito ou detecção (`CD_SETOR`, depois qualquer
/// coluna com "setor" no nome). `is_value_column` escolhe as colunas de valor.
pub fn sector_values_from_table(
    text: &str,
    scope: &RegionalScope,
    sector_column: Option<&str>,
    is_value_column: impl Fn(&str) -> bool,
) -> Result<(Vec<RawSectorValue>, ImportReport), CrawlError> {
    // Primeiro só o cabeçalho, para descobrir a coluna do setor.
    let header = parse_delimited(text.lines().next().unwrap_or(""), |_| false)?.header;
    let sector_idx = match sector_column {
        Some(name) => header.iter().position(|h| h.eq_ignore_ascii_case(name)),
        None => header
            .iter()
            .position(|h| h.eq_ignore_ascii_case("CD_SETOR"))
            .or_else(|| {
                header
                    .iter()
                    .position(|h| h.to_ascii_lowercase().contains("setor"))
            }),
    }
    .ok_or_else(|| {
        CrawlError::Parse(format!(
            "coluna do setor censitário não encontrada; colunas: {header:?}"
        ))
    })?;
    let value_cols: Vec<usize> = (0..header.len())
        .filter(|&i| i != sector_idx && is_value_column(&header[i]))
        .collect();
    if value_cols.is_empty() {
        return Err(CrawlError::Parse(format!(
            "nenhuma coluna de valor reconhecida; colunas: {header:?}"
        )));
    }
    let mut report = ImportReport::default();
    let table = parse_delimited(text, |row| {
        row.get(sector_idx).is_some_and(|code| {
            let digits: String = code.chars().filter(char::is_ascii_digit).collect();
            scope.contains(&digits)
        })
    })?;
    report.read = table.rows.len();
    let mut out = Vec::with_capacity(table.rows.len() * value_cols.len());
    for row in table.rows {
        if row.len() != header.len() {
            report.skip(format!(
                "linha com {} colunas (esperado {}): setor {:?}",
                row.len(),
                header.len(),
                row.get(sector_idx)
            ));
            continue;
        }
        for &i in &value_cols {
            out.push(RawSectorValue {
                sector_code: row[sector_idx].clone(),
                variable: header[i].clone(),
                value: row[i].clone(),
            });
        }
    }
    Ok((out, report))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn published_urls() {
        assert_eq!(
            published_url(MESH_URL, "SP_setores_CD2022.gpkg", "_setores_CD2022.gpkg"),
            format!("{MESH_URL}SP/SP_setores_CD2022.gpkg")
        );
        assert_eq!(
            published_url(
                AGGREGATES_URL,
                "Agregados_por_setores_basico_BR_20260520.zip",
                "Agregados_por_setores_"
            ),
            format!("{AGGREGATES_URL}Agregados_por_setores_basico_BR_20260520.zip")
        );
        assert_eq!(
            published_url(MESH_URL, "amostra.gpkg", "_setores_CD2022.gpkg"),
            MESH_URL
        );
    }

    #[test]
    fn strips_gpkg_header() {
        // GP, versão 0, flags = little endian + envelope XY (32 bytes), srs_id.
        let mut blob = vec![b'G', b'P', 0, 0b0000_0011];
        blob.extend(4674i32.to_le_bytes());
        blob.extend([0u8; 32]);
        blob.extend([1, 1, 0, 0, 0]); // início de um WKB
        assert_eq!(gpkg_to_wkb(&blob), Some(&[1u8, 1, 0, 0, 0][..]));

        let mut empty = vec![b'G', b'P', 0, 0b0001_0001];
        empty.extend(4674i32.to_le_bytes());
        assert_eq!(gpkg_to_wkb(&empty), None);
        assert_eq!(gpkg_to_wkb(b"XX"), None);
    }

    #[test]
    fn table_to_sector_values_filters_scope_and_flags_broken_rows() {
        let csv = "CD_SETOR;NM_MUN;V0001;V0002;V0005\n\
                   354870805000012;São Bernardo;512;180;2,9\n\
                   355030801000001;São Paulo;900;300;3,0\n\
                   354870805000013;São Bernardo;X\n";
        let scope = RegionalScope::Municipality("3548708".into());
        let (values, report) =
            sector_values_from_table(csv, &scope, None, |h| h.len() == 5 && h.starts_with('V'))
                .unwrap();
        assert_eq!(values.len(), 3);
        assert!(values.iter().all(|v| v.sector_code == "354870805000012"));
        assert_eq!(report.read, 2);
        assert_eq!(report.skipped, 1);
    }

    #[test]
    fn missing_sector_column_is_an_error() {
        let scope = RegionalScope::State("35".into());
        let err = sector_values_from_table("A;B\n1;2\n", &scope, None, |_| true).unwrap_err();
        assert!(err.to_string().contains("setor"));
    }
}

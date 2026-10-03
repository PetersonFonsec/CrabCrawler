use std::collections::HashMap;

use chrono::{DateTime, NaiveDate, Utc};
use crab_domain::{
    CountingUnit, CrimeStatistic, CrimeType, DataSource, Period, PeriodGranularity, Region,
    RegionLevel, SecurityProvenance,
};
use serde::Serialize;
use sqlx::{FromRow, PgPool};

use crate::upsert_region;

/// Uma importação de dataset de segurança (para o log de procedência).
#[derive(Debug, Clone, Serialize)]
pub struct DatasetImport {
    pub source: DataSource,
    pub source_url: Option<String>,
    pub dataset_version: Option<String>,
    /// Recorte pedido, ex.: `"SP"` ou `"SP/3548708"`.
    pub scope: String,
    pub records_read: usize,
    pub records_stored: usize,
    pub records_skipped: usize,
    pub report: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct ImportRow {
    pub source: String,
    pub source_url: Option<String>,
    pub dataset_version: Option<String>,
    pub scope: String,
    pub records_read: i32,
    pub records_stored: i32,
    pub records_skipped: i32,
    pub imported_at: DateTime<Utc>,
}

/// Região que tem estatísticas criminais, com cobertura temporal.
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct SecurityRegionRow {
    pub level: String,
    pub code: String,
    pub name: String,
    pub municipality_ibge_code: String,
    pub first_period: NaiveDate,
    pub last_period: NaiveDate,
}

/// População municipal gravada em `region_indicators`.
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct PopulationRow {
    pub value: f64,
    pub period_start: NaiveDate,
    pub source: String,
    pub source_url: Option<String>,
}

#[derive(FromRow)]
struct StatisticRow {
    level: String,
    code: String,
    name: String,
    municipality_ibge_code: String,
    crime_type: String,
    counting_unit: String,
    count: i64,
    period_start: NaiveDate,
    period_end: NaiveDate,
    period_granularity: String,
    source: String,
    source_label: String,
    source_url: Option<String>,
    dataset_version: Option<String>,
    collected_at: DateTime<Utc>,
}

impl StatisticRow {
    fn into_domain(self) -> Option<CrimeStatistic> {
        let level = match self.level.as_str() {
            "municipality" => RegionLevel::Municipality,
            "neighborhood" => RegionLevel::Neighborhood,
            "census_tract" => RegionLevel::CensusTract,
            "police_district" => RegionLevel::PoliceDistrict,
            _ => return None,
        };
        let granularity = match self.period_granularity.as_str() {
            "month" => PeriodGranularity::Month,
            "year" => PeriodGranularity::Year,
            _ => return None,
        };
        Some(CrimeStatistic {
            region: Region {
                level,
                code: self.code,
                name: self.name,
                municipality_ibge_code: self.municipality_ibge_code,
            },
            crime_type: CrimeType::parse(&self.crime_type)?,
            counting_unit: CountingUnit::parse(&self.counting_unit)?,
            count: u64::try_from(self.count).ok()?,
            period: Period {
                start: self.period_start,
                end: self.period_end,
                granularity,
            },
            source_label: self.source_label,
            provenance: SecurityProvenance {
                source: DataSource::parse(&self.source)?,
                source_url: self.source_url,
                dataset_version: self.dataset_version,
                collected_at: self.collected_at,
            },
        })
    }
}

#[derive(Clone)]
pub struct SecurityRepository {
    pool: PgPool,
}

impl SecurityRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Upsert idempotente, numa única transação. Reimportar o mesmo arquivo
    /// atualiza contagens (a fonte revisa números) sem duplicar linhas.
    pub async fn upsert_statistics(&self, stats: &[CrimeStatistic]) -> Result<usize, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let mut regions: HashMap<(RegionLevel, String, String), i64> = HashMap::new();
        for s in stats {
            let key = (
                s.region.level,
                s.region.code.clone(),
                s.region.municipality_ibge_code.clone(),
            );
            let region_id = match regions.get(&key) {
                Some(id) => *id,
                None => {
                    let id = upsert_region(&mut tx, &s.region).await?;
                    regions.insert(key, id);
                    id
                }
            };
            sqlx::query(
                "INSERT INTO crime_statistics (region_id, crime_type, counting_unit, count, \
                    period_start, period_end, period_granularity, source, source_label, \
                    source_url, dataset_version, collected_at) \
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12) \
                 ON CONFLICT (region_id, crime_type, counting_unit, period_start, period_end, \
                    source, source_label) DO UPDATE SET \
                    count = EXCLUDED.count, period_granularity = EXCLUDED.period_granularity, \
                    source_url = EXCLUDED.source_url, dataset_version = EXCLUDED.dataset_version, \
                    collected_at = EXCLUDED.collected_at",
            )
            .bind(region_id)
            .bind(s.crime_type.as_str())
            .bind(s.counting_unit.as_str())
            .bind(i64::try_from(s.count).unwrap_or(i64::MAX))
            .bind(s.period.start)
            .bind(s.period.end)
            .bind(s.period.granularity.as_str())
            .bind(s.provenance.source.as_str())
            .bind(&s.source_label)
            .bind(&s.provenance.source_url)
            .bind(&s.provenance.dataset_version)
            .bind(s.provenance.collected_at)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(stats.len())
    }

    pub async fn record_import(&self, import: &DatasetImport) -> Result<(), sqlx::Error> {
        let to_i32 = |n: usize| i32::try_from(n).unwrap_or(i32::MAX);
        sqlx::query(
            "INSERT INTO security_dataset_imports (source, source_url, dataset_version, scope, \
                records_read, records_stored, records_skipped, report) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
        )
        .bind(import.source.as_str())
        .bind(&import.source_url)
        .bind(&import.dataset_version)
        .bind(&import.scope)
        .bind(to_i32(import.records_read))
        .bind(to_i32(import.records_stored))
        .bind(to_i32(import.records_skipped))
        .bind(&import.report)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Estatísticas de uma região exata (nível + código + município).
    pub async fn statistics_for_region(
        &self,
        municipality_ibge_code: &str,
        level: RegionLevel,
        code: &str,
        crime_type: Option<CrimeType>,
    ) -> Result<Vec<CrimeStatistic>, sqlx::Error> {
        let rows: Vec<StatisticRow> = sqlx::query_as(
            "SELECT r.level, r.code, r.name, r.municipality_ibge_code, s.crime_type, \
                s.counting_unit, s.count, s.period_start, s.period_end, s.period_granularity, \
                s.source, s.source_label, s.source_url, s.dataset_version, s.collected_at \
             FROM crime_statistics s JOIN regions r ON r.id = s.region_id \
             WHERE r.municipality_ibge_code = $1 AND r.level = $2 AND r.code = $3 \
               AND ($4::text IS NULL OR s.crime_type = $4) \
             ORDER BY s.period_start, s.crime_type",
        )
        .bind(municipality_ibge_code)
        .bind(level.as_str())
        .bind(code)
        .bind(crime_type.map(|t| t.as_str()))
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .filter_map(|row| {
                let id = format!("{}/{}", row.level, row.code);
                let parsed = row.into_domain();
                if parsed.is_none() {
                    tracing::warn!(region = %id, "linha de crime_statistics com valor desconhecido");
                }
                parsed
            })
            .collect())
    }

    /// Regiões com estatísticas num município (o próprio município e
    /// áreas de delegacia), com o intervalo coberto.
    pub async fn regions_in_municipality(
        &self,
        municipality_ibge_code: &str,
    ) -> Result<Vec<SecurityRegionRow>, sqlx::Error> {
        sqlx::query_as(
            "SELECT r.level, r.code, r.name, r.municipality_ibge_code, \
                MIN(s.period_start) AS first_period, MAX(s.period_end) AS last_period \
             FROM crime_statistics s JOIN regions r ON r.id = s.region_id \
             WHERE r.municipality_ibge_code = $1 \
             GROUP BY r.level, r.code, r.name, r.municipality_ibge_code \
             ORDER BY r.level, r.name",
        )
        .bind(municipality_ibge_code)
        .fetch_all(&self.pool)
        .await
    }

    /// População do município (todas as referências disponíveis).
    pub async fn municipal_population(
        &self,
        municipality_ibge_code: &str,
    ) -> Result<Vec<PopulationRow>, sqlx::Error> {
        sqlx::query_as(
            "SELECT i.value, i.period_start, i.source, i.source_url \
             FROM region_indicators i JOIN regions r ON r.id = i.region_id \
             WHERE r.level = 'municipality' AND r.code = $1 AND i.kind = 'population' \
             ORDER BY i.period_start",
        )
        .bind(municipality_ibge_code)
        .fetch_all(&self.pool)
        .await
    }

    /// Última importação de cada dataset (fonte + versão), mais recentes
    /// primeiro: importar 2023 depois de 2025 não esconde o de 2025.
    pub async fn latest_imports(&self) -> Result<Vec<ImportRow>, sqlx::Error> {
        sqlx::query_as(
            "SELECT * FROM ( \
                SELECT DISTINCT ON (source, dataset_version) source, source_url, \
                    dataset_version, scope, records_read, records_stored, records_skipped, \
                    imported_at \
                FROM security_dataset_imports \
                ORDER BY source, dataset_version, imported_at DESC \
             ) latest ORDER BY source, dataset_version DESC NULLS LAST",
        )
        .fetch_all(&self.pool)
        .await
    }
}

use chrono::{DateTime, NaiveDate, Utc};
use crab_domain::RegionIndicator;
use serde::Serialize;
use sqlx::{FromRow, PgPool};

use crate::upsert_region;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct IndicatorRow {
    pub region_level: String,
    pub region_code: String,
    pub region_name: String,
    pub kind: String,
    pub value: f64,
    pub period_start: NaiveDate,
    pub period_end: NaiveDate,
    pub source: String,
    pub source_url: Option<String>,
    pub collected_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct IndicatorRepository {
    pool: PgPool,
}

impl IndicatorRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn upsert(&self, item: &RegionIndicator) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let region_id = upsert_region(&mut tx, &item.region).await?;

        sqlx::query(
            "INSERT INTO region_indicators (region_id, kind, value, period_start, period_end, \
                source, source_url, collected_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8) \
             ON CONFLICT (region_id, kind, period_start, period_end, source) DO UPDATE SET \
                value = EXCLUDED.value, source_url = EXCLUDED.source_url, \
                collected_at = EXCLUDED.collected_at",
        )
        .bind(region_id)
        .bind(item.indicator.kind.as_str())
        .bind(item.indicator.value)
        .bind(item.indicator.period_start)
        .bind(item.indicator.period_end)
        .bind(item.provenance.source.as_str())
        .bind(&item.provenance.url)
        .bind(item.provenance.collected_at)
        .execute(&mut *tx)
        .await?;
        tx.commit().await
    }

    /// Todos os indicadores de um município, em qualquer nível regional.
    pub async fn for_municipality(
        &self,
        ibge_code: &str,
    ) -> Result<Vec<IndicatorRow>, sqlx::Error> {
        sqlx::query_as(
            "SELECT r.level AS region_level, r.code AS region_code, r.name AS region_name, \
                i.kind, i.value, i.period_start, i.period_end, i.source, i.source_url, \
                i.collected_at \
             FROM region_indicators i JOIN regions r ON r.id = i.region_id \
             WHERE r.municipality_ibge_code = $1 \
             ORDER BY i.kind, i.period_start DESC",
        )
        .bind(ibge_code)
        .fetch_all(&self.pool)
        .await
    }
}

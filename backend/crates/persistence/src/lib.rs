//! Persistência em PostgreSQL/PostGIS.
//!
//! As queries usam a API dinâmica do sqlx (sem checagem em tempo de
//! compilação) para o projeto compilar sem banco rodando.

pub mod indicators;
pub mod listings;
pub mod regional;
pub mod security;

pub use indicators::{IndicatorRepository, IndicatorRow};
pub use listings::{ListingFilter, ListingRepository, ListingRow};
pub use regional::{
    DatasetCoverageRow, DatasetRef, RegionalImportRow, RegionalRepository, RiskAreaRow,
    SectorIndicatorRow, SectorRow, ServiceRow, StoreOutcome,
};
pub use security::{
    DatasetImport, ImportRow, PopulationRow, SecurityRegionRow, SecurityRepository,
};

use crab_domain::Region;
use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::{Postgres, Transaction};

pub async fn connect(database_url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await
}

pub async fn migrate(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("../../migrations").run(pool).await
}

pub(crate) fn enum_str<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// Insere ou encontra uma região e devolve o id. Um nome igual ao código
/// (fonte sem nome legível) não sobrescreve um nome já conhecido.
pub(crate) async fn upsert_region(
    tx: &mut Transaction<'_, Postgres>,
    region: &Region,
) -> Result<i64, sqlx::Error> {
    let (id,): (i64,) = sqlx::query_as(
        "INSERT INTO regions (level, code, name, municipality_ibge_code) VALUES ($1,$2,$3,$4) \
         ON CONFLICT (level, code, municipality_ibge_code) DO UPDATE SET name = \
            CASE WHEN EXCLUDED.name = EXCLUDED.code THEN regions.name ELSE EXCLUDED.name END \
         RETURNING id",
    )
    .bind(region.level.as_str())
    .bind(&region.code)
    .bind(&region.name)
    .bind(&region.municipality_ibge_code)
    .fetch_one(&mut **tx)
    .await?;
    Ok(id)
}

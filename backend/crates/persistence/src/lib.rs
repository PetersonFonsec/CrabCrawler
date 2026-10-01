//! Persistência em PostgreSQL/PostGIS.
//!
//! As queries usam a API dinâmica do sqlx (sem checagem em tempo de
//! compilação) para o projeto compilar sem banco rodando.

pub mod indicators;
pub mod listings;

pub use indicators::{IndicatorRepository, IndicatorRow};
pub use listings::{ListingFilter, ListingRepository, ListingRow};

use sqlx::postgres::{PgPool, PgPoolOptions};

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

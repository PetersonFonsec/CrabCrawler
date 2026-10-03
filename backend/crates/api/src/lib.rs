//! API HTTP (Axum) consumida pela interface web.

mod error;
mod routes;
mod security;

use axum::routing::get;
use axum::Router;
use crab_persistence::{IndicatorRepository, ListingRepository, SecurityRepository};
use sqlx::PgPool;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

#[derive(Clone)]
pub struct AppState {
    pub listings: ListingRepository,
    pub indicators: IndicatorRepository,
    pub security: SecurityRepository,
}

impl AppState {
    pub fn new(pool: PgPool) -> Self {
        Self {
            listings: ListingRepository::new(pool.clone()),
            indicators: IndicatorRepository::new(pool.clone()),
            security: SecurityRepository::new(pool),
        }
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(routes::health))
        .route("/sources", get(routes::sources))
        .route("/listings", get(routes::list_listings))
        .route("/listings/{id}", get(routes::get_listing))
        .route("/listings/{id}/security", get(security::listing_security))
        .route(
            "/regions/{ibge_code}/indicators",
            get(routes::region_indicators),
        )
        .route(
            "/regions/{ibge_code}/security",
            get(security::region_security),
        )
        .route(
            "/regions/{ibge_code}/security/history",
            get(security::region_security_history),
        )
        .route("/security/compare", get(security::compare))
        .route("/security/sources", get(security::sources))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}

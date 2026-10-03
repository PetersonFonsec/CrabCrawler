//! API HTTP (Axum) consumida pela interface web.

mod error;
mod regional;
mod routes;
mod security;

use axum::routing::get;
use axum::Router;
use crab_persistence::{
    IndicatorRepository, ListingRepository, RegionalRepository, SecurityRepository,
};
use sqlx::PgPool;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

#[derive(Clone)]
pub struct AppState {
    pub listings: ListingRepository,
    pub indicators: IndicatorRepository,
    pub security: SecurityRepository,
    pub regional: RegionalRepository,
}

impl AppState {
    pub fn new(pool: PgPool) -> Self {
        Self {
            listings: ListingRepository::new(pool.clone()),
            indicators: IndicatorRepository::new(pool.clone()),
            security: SecurityRepository::new(pool.clone()),
            regional: RegionalRepository::new(pool),
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
        .route("/listings/{id}/region", get(regional::listing_region))
        .route(
            "/listings/{id}/region/profile",
            get(regional::listing_profile),
        )
        .route(
            "/listings/{id}/region/services",
            get(regional::listing_services),
        )
        .route("/listings/{id}/region/risks", get(regional::listing_risks))
        .route(
            "/listings/{id}/intelligence",
            get(regional::listing_intelligence),
        )
        .route("/regional/sources", get(regional::sources))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}

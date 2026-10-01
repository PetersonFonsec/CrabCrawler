//! API HTTP (Axum) consumida pela interface web.

mod error;
mod routes;

use axum::routing::get;
use axum::Router;
use crab_persistence::{IndicatorRepository, ListingRepository};
use sqlx::PgPool;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

#[derive(Clone)]
pub struct AppState {
    pub listings: ListingRepository,
    pub indicators: IndicatorRepository,
}

impl AppState {
    pub fn new(pool: PgPool) -> Self {
        Self {
            listings: ListingRepository::new(pool.clone()),
            indicators: IndicatorRepository::new(pool),
        }
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(routes::health))
        .route("/sources", get(routes::sources))
        .route("/listings", get(routes::list_listings))
        .route("/listings/{id}", get(routes::get_listing))
        .route(
            "/regions/{ibge_code}/indicators",
            get(routes::region_indicators),
        )
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}

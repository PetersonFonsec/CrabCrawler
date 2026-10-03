//! API HTTP (Axum) consumida pela interface web.

mod error;
mod properties;
mod regional;
mod routes;
mod security;

use axum::extract::DefaultBodyLimit;
use axum::routing::{get, post};
use axum::Router;
use crab_ingest::PropertyIngestor;
use crab_persistence::{
    IndicatorRepository, ListingRepository, PropertyRepository, RegionalRepository,
    SecurityRepository,
};
use crab_processing::{Gazetteer, PropertyNormalizer};
use sqlx::PgPool;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

#[derive(Clone)]
pub struct AppState {
    pub listings: ListingRepository,
    pub indicators: IndicatorRepository,
    pub security: SecurityRepository,
    pub regional: RegionalRepository,
    pub properties: PropertyRepository,
    pub ingestor: PropertyIngestor,
}

impl AppState {
    /// Estado sem enriquecimento externo (sem consulta de CEP nem
    /// geocoding).
    pub fn new(pool: PgPool) -> Self {
        let ingestor = PropertyIngestor::new(
            PropertyRepository::new(pool.clone()),
            PropertyNormalizer::new(Gazetteer::mvp()),
        );
        Self::with_ingestor(pool, ingestor)
    }

    /// Estado com um pipeline de ingestão já configurado (CEP, geocoding).
    pub fn with_ingestor(pool: PgPool, ingestor: PropertyIngestor) -> Self {
        Self {
            listings: ListingRepository::new(pool.clone()),
            indicators: IndicatorRepository::new(pool.clone()),
            security: SecurityRepository::new(pool.clone()),
            regional: RegionalRepository::new(pool.clone()),
            properties: PropertyRepository::new(pool),
            ingestor,
        }
    }
}

/// Limite do corpo do cadastro manual: o formulário tem poucos campos.
const MANUAL_BODY_LIMIT: usize = 32 * 1024;

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
        .route(
            "/properties/manual",
            post(properties::create_manual).layer(DefaultBodyLimit::max(MANUAL_BODY_LIMIT)),
        )
        .route("/properties", get(properties::list_properties))
        .route("/properties/{id}", get(properties::get_property))
        .route(
            "/properties/{id}/listings",
            get(properties::property_listings),
        )
        .route("/properties/{id}/region", get(regional::property_region))
        .route(
            "/properties/{id}/intelligence",
            get(regional::property_intelligence),
        )
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}

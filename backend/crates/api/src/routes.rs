use axum::extract::{Path, Query, State};
use axum::Json;
use crab_domain::DataSource;
use crab_persistence::{IndicatorRow, ListingFilter, ListingRow};
use crab_processing::{compare_price_per_m2, PriceComparison};
use serde::Serialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::error::ApiError;
use crate::AppState;

pub async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

/// Lista as fontes de dados, para a interface explicar a origem de cada dado.
pub async fn sources() -> Json<Value> {
    Json(json!(DataSource::ALL
        .iter()
        .map(|s| json!({ "id": s.as_str(), "reference_url": s.reference_url() }))
        .collect::<Vec<_>>()))
}

pub async fn list_listings(
    State(state): State<AppState>,
    Query(filter): Query<ListingFilter>,
) -> Result<Json<Vec<ListingRow>>, ApiError> {
    Ok(Json(state.listings.search(&filter).await?))
}

/// Imóvel + contexto da região: o cruzamento que é o objetivo do MVP.
#[derive(Serialize)]
pub struct ListingDetail {
    listing: ListingRow,
    price_comparison: Option<PriceComparison>,
    region_indicators: Vec<IndicatorRow>,
}

pub async fn get_listing(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ListingDetail>, ApiError> {
    let listing = state.listings.get(id).await?.ok_or(ApiError::NotFound)?;

    let price_comparison = match price_per_m2(&listing) {
        Some(own) => {
            let comparables = state.listings.comparables(&listing).await?;
            compare_price_per_m2(own, comparables.iter().filter_map(price_per_m2).collect())
        }
        None => None,
    };

    let region_indicators = match &listing.municipality_ibge_code {
        Some(code) => state.indicators.for_municipality(code).await?,
        None => vec![],
    };

    Ok(Json(ListingDetail {
        listing,
        price_comparison,
        region_indicators,
    }))
}

pub async fn region_indicators(
    State(state): State<AppState>,
    Path(ibge_code): Path<String>,
) -> Result<Json<Vec<IndicatorRow>>, ApiError> {
    Ok(Json(state.indicators.for_municipality(&ibge_code).await?))
}

fn price_per_m2(row: &ListingRow) -> Option<f64> {
    match (row.price_brl, row.area_m2) {
        (Some(p), Some(a)) if a > 0.0 => Some(p / a),
        _ => None,
    }
}

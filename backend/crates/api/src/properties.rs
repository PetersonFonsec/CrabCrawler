//! Imóveis de qualquer origem: cadastro manual, leitura, anúncios e
//! histórico de preço.
//!
//! Não há endpoint administrativo (parceiros, sincronização): a API não tem
//! autenticação, então essas operações ficam só na CLI.

use std::collections::HashMap;

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::Utc;
use crab_domain::{ManualPropertyInput, PriceObservation};
use crab_ingest::IngestError;
use crab_persistence::{PropertyFilter, PropertyListingRow, PropertyRow};
use crab_processing::property::{price_insight, FieldIssue, PriceInsight};
use crab_processing::{compare_price_per_m2, PriceComparison};
use serde::Serialize;
use uuid::Uuid;

use crate::error::ApiError;
use crate::AppState;

/// O que já dá para analisar com os dados do imóvel.
#[derive(Debug, Serialize)]
pub struct AnalysisReadiness {
    /// Há coordenada precisa o bastante para Regional Intelligence.
    pub region_ready: bool,
    /// Há município (código IBGE) para dados de segurança e indicadores.
    pub municipality_ready: bool,
    /// Há preço, área, tipo e bairro para comparar preço/m².
    pub comparison_ready: bool,
    /// O que falta, em português, para a interface mostrar.
    pub missing: Vec<String>,
}

pub fn readiness(p: &PropertyRow) -> AnalysisReadiness {
    let mut missing = Vec::new();
    let approximate = matches!(
        p.coordinate_precision.as_deref(),
        Some("POSTAL_CODE") | Some("APPROXIMATE")
    );
    let region_ready = p.lat.is_some() && p.lon.is_some() && !approximate;
    if p.lat.is_none() {
        missing.push(
            "Coordenada: informe rua e número (ou ative o geocoding) para analisar a região."
                .into(),
        );
    } else if approximate {
        missing.push(
            "Coordenada aproximada: informe o endereço completo para analisar a região.".into(),
        );
    }
    let municipality_ready = p.municipality_ibge_code.is_some();
    if !municipality_ready {
        missing.push("Município: não identificado; informe cidade e UF ou um CEP válido.".into());
    }
    let mut comparison_ready = true;
    if p.area_m2.is_none() {
        comparison_ready = false;
        missing.push("Área: necessária para comparar o preço por m².".into());
    }
    if p.property_type == "other" {
        comparison_ready = false;
        missing.push("Tipo do imóvel: necessário para achar imóveis comparáveis.".into());
    }
    if p.neighborhood_slug.is_none() {
        comparison_ready = false;
        missing.push("Bairro: necessário para comparar com imóveis da vizinhança.".into());
    }
    AnalysisReadiness {
        region_ready,
        municipality_ready,
        comparison_ready,
        missing,
    }
}

#[derive(Debug, Serialize)]
pub struct ManualCreated {
    pub property: PropertyRow,
    pub listing: PropertyListingRow,
    pub warnings: Vec<FieldIssue>,
    pub notes: Vec<String>,
    pub analysis: AnalysisReadiness,
}

/// `POST /properties/manual`
pub async fn create_manual(
    State(state): State<AppState>,
    body: Result<Json<ManualPropertyInput>, JsonRejection>,
) -> Result<(StatusCode, Json<ManualCreated>), ApiError> {
    let Json(input) =
        body.map_err(|e| ApiError::BadRequest(format!("JSON inválido: {}", e.body_text())))?;
    let outcome = match state.ingestor.ingest(input.into_raw(), Utc::now()).await {
        Ok(o) => o,
        Err(IngestError::Invalid(f)) => {
            return Err(ApiError::Invalid {
                errors: serde_json::to_value(&f.errors).unwrap_or_default(),
                warnings: serde_json::to_value(&f.warnings).unwrap_or_default(),
            })
        }
        Err(IngestError::Database(e)) => return Err(e.into()),
    };
    let listing_id = outcome
        .listings
        .first()
        .map(|l| l.listing_id)
        .ok_or(ApiError::Internal)?;
    let property = state
        .properties
        .property(outcome.property_id)
        .await?
        .ok_or(ApiError::Internal)?;
    let listing = state
        .properties
        .listing(listing_id)
        .await?
        .ok_or(ApiError::Internal)?;
    tracing::info!(
        property_id = %property.id,
        listing_id = %listing.id,
        source = "MANUAL",
        warnings = outcome.warnings.len(),
        "imóvel cadastrado manualmente"
    );
    Ok((
        StatusCode::CREATED,
        Json(ManualCreated {
            analysis: readiness(&property),
            property,
            listing,
            warnings: outcome.warnings,
            notes: outcome.notes,
        }),
    ))
}

#[derive(Debug, Serialize)]
pub struct ListingSummary {
    pub id: Uuid,
    pub source: String,
    pub transaction: String,
    pub status: String,
    pub price_brl: f64,
    pub source_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PropertySummary {
    #[serde(flatten)]
    pub property: PropertyRow,
    pub listings: Vec<ListingSummary>,
}

/// `GET /properties`
pub async fn list_properties(
    State(state): State<AppState>,
    Query(filter): Query<PropertyFilter>,
) -> Result<Json<Vec<PropertySummary>>, ApiError> {
    let properties = state.properties.search(&filter).await?;
    let ids: Vec<Uuid> = properties.iter().map(|p| p.id).collect();
    let mut by_property: HashMap<Uuid, Vec<ListingSummary>> = HashMap::new();
    for l in state.properties.listings_of(&ids).await? {
        by_property
            .entry(l.property_id)
            .or_default()
            .push(ListingSummary {
                id: l.id,
                source: l.source,
                transaction: l.transaction,
                status: l.status,
                price_brl: l.price_brl,
                source_url: l.source_url,
            });
    }
    Ok(Json(
        properties
            .into_iter()
            .map(|p| PropertySummary {
                listings: by_property.remove(&p.id).unwrap_or_default(),
                property: p,
            })
            .collect(),
    ))
}

#[derive(Debug, Serialize)]
pub struct PricePoint {
    pub price_brl: f64,
    pub observed_at: chrono::DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct ListingView {
    #[serde(flatten)]
    pub listing: PropertyListingRow,
    pub price_history: Vec<PricePoint>,
    /// Ex.: "Preço reduzido em 4,8% há 17 dias".
    pub price_insight: Option<PriceInsight>,
    /// Preço/m² contra anúncios ativos semelhantes (mesmo bairro, tipo e
    /// finalidade), de qualquer fonte.
    pub price_comparison: Option<PriceComparison>,
}

async fn listing_views(
    state: &AppState,
    property: &PropertyRow,
) -> Result<Vec<ListingView>, ApiError> {
    let listings = state.properties.listings_of(&[property.id]).await?;
    let ids: Vec<Uuid> = listings.iter().map(|l| l.id).collect();
    let mut history: HashMap<Uuid, Vec<PriceObservation>> = HashMap::new();
    for obs in state.properties.price_history(&ids).await? {
        history.entry(obs.listing_id).or_default().push(obs);
    }
    let now = Utc::now();
    let mut views = Vec::new();
    for listing in listings {
        let obs = history.remove(&listing.id).unwrap_or_default();
        let price_comparison = match property.area_m2.filter(|a| *a > 0.0) {
            Some(area) => match state.listings.get(listing.id).await? {
                Some(row) => {
                    let values = state
                        .listings
                        .comparables(&row)
                        .await?
                        .iter()
                        .filter_map(|c| match (c.price_brl, c.area_m2) {
                            (Some(p), Some(a)) if a > 0.0 => Some(p / a),
                            _ => None,
                        })
                        .collect();
                    compare_price_per_m2(listing.price_brl / area, values)
                }
                None => None,
            },
            None => None,
        };
        views.push(ListingView {
            price_insight: price_insight(&obs, now),
            price_history: obs
                .into_iter()
                .map(|o| PricePoint {
                    price_brl: o.price_brl,
                    observed_at: o.observed_at,
                })
                .collect(),
            price_comparison,
            listing,
        });
    }
    Ok(views)
}

#[derive(Debug, Serialize)]
pub struct PropertyDetail {
    pub property: PropertyRow,
    pub listings: Vec<ListingView>,
    pub analysis: AnalysisReadiness,
}

/// `GET /properties/{id}`
pub async fn get_property(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<PropertyDetail>, ApiError> {
    let property = state
        .properties
        .property(id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let listings = listing_views(&state, &property).await?;
    Ok(Json(PropertyDetail {
        analysis: readiness(&property),
        property,
        listings,
    }))
}

/// `GET /properties/{id}/listings`
pub async fn property_listings(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<ListingView>>, ApiError> {
    let property = state
        .properties
        .property(id)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(listing_views(&state, &property).await?))
}

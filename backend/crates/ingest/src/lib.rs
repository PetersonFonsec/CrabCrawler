//! Pipeline de ingestão de imóveis:
//!
//! ```text
//! RawProperty ─► normalizar ─► (inalterado? só marca "visto")
//!                     │
//!                     ▼
//!     completar endereço pelo CEP (opcional) ─► geocoding (opcional, com cache)
//!                     │
//!                     ▼
//!     gravar imóvel + anúncios + histórico de preço (uma transação)
//! ```
//!
//! O mesmo pipeline atende o cadastro manual (API) e as sincronizações de
//! feeds e APIs (CLI). Nada aqui depende do formato de uma fonte.

pub mod fixture;
pub mod sync;

use std::sync::Arc;

use chrono::{DateTime, Utc};
use crab_crawler::geocoding::{GeocodeQuery, Geocoder, PostalCodeLookup};
use crab_domain::{
    Address, CoordinatePrecision, CoordinateSource, Coordinates, GeoPoint, PropertySource,
    RawProperty,
};
use crab_persistence::{
    ListingGroupWrite, ListingWriteResult, PropertyRepository, PropertyRow, WriteOutcome,
};
use crab_processing::normalize::slugify;
use crab_processing::property::{content_hash, FieldIssue, NormalizationFailure};
use crab_processing::PropertyNormalizer;
use serde::Serialize;
use uuid::Uuid;

pub use sync::{DeactivationDecision, SyncOptions, SyncReport};

#[derive(Debug, thiserror::Error)]
pub enum IngestError {
    #[error("imóvel inválido: {0}")]
    Invalid(NormalizationFailure),
    #[error("erro de banco: {0}")]
    Database(#[from] sqlx::Error),
}

/// O que aconteceu com um item.
#[derive(Debug, Clone, Serialize)]
pub struct IngestOutcome {
    pub property_id: Uuid,
    pub external_id: String,
    pub listings: Vec<ListingWriteResult>,
    pub warnings: Vec<FieldIssue>,
    /// Passos de enriquecimento aplicados (CEP, geocoding), em português.
    pub notes: Vec<String>,
    pub coordinates: Option<Coordinates>,
}

impl IngestOutcome {
    pub fn count(&self, outcome: WriteOutcome) -> u32 {
        self.listings
            .iter()
            .filter(|l| l.outcome == outcome)
            .count() as u32
    }
}

/// Orquestra normalização, enriquecimento e gravação.
#[derive(Clone)]
pub struct PropertyIngestor {
    repo: PropertyRepository,
    normalizer: PropertyNormalizer,
    postal: Option<Arc<dyn PostalCodeLookup>>,
    geocoder: Option<Arc<dyn Geocoder>>,
}

impl PropertyIngestor {
    pub fn new(repo: PropertyRepository, normalizer: PropertyNormalizer) -> Self {
        Self {
            repo,
            normalizer,
            postal: None,
            geocoder: None,
        }
    }

    pub fn with_postal_lookup(mut self, lookup: Arc<dyn PostalCodeLookup>) -> Self {
        self.postal = Some(lookup);
        self
    }

    pub fn with_geocoder(mut self, geocoder: Arc<dyn Geocoder>) -> Self {
        self.geocoder = Some(geocoder);
        self
    }

    pub fn repository(&self) -> &PropertyRepository {
        &self.repo
    }

    /// Normaliza e grava um item. `now` é o instante da observação (numa
    /// sincronização, o mesmo para todos os itens).
    pub async fn ingest(
        &self,
        raw: RawProperty,
        now: DateTime<Utc>,
    ) -> Result<IngestOutcome, IngestError> {
        let mut n = self
            .normalizer
            .normalize(raw)
            .map_err(IngestError::Invalid)?;
        // O cadastro manual não tem id externo: cada cadastro é um anúncio novo.
        let external_id = n
            .external_id
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string());

        // Hash sobre o dado normalizado, antes de qualquer enriquecimento:
        // assim um item igual ao anterior não dispara CEP nem geocoding.
        let hashes: Vec<String> = n
            .listings
            .iter()
            .map(|l| content_hash(&n.property, l))
            .collect();

        let existing = self
            .repo
            .find_by_identity(n.source, n.partner_id, &external_id)
            .await?;
        let unchanged = !existing.is_empty()
            && n.listings.iter().zip(&hashes).all(|(l, h)| {
                existing.iter().any(|e| {
                    e.transaction == l.transaction.as_str() && e.content_hash.as_deref() == Some(h)
                })
            });
        if unchanged {
            let ids: Vec<Uuid> = n
                .listings
                .iter()
                .filter_map(|l| {
                    existing
                        .iter()
                        .find(|e| e.transaction == l.transaction.as_str())
                })
                .map(|e| e.listing_id)
                .collect();
            self.repo.touch(&ids, now).await?;
            let listings = n
                .listings
                .iter()
                .filter_map(|l| {
                    let e = existing
                        .iter()
                        .find(|e| e.transaction == l.transaction.as_str())?;
                    Some(ListingWriteResult {
                        listing_id: e.listing_id,
                        transaction: l.transaction,
                        outcome: if e.status == "ACTIVE" || e.status == "REMOVED" {
                            WriteOutcome::Unchanged
                        } else {
                            WriteOutcome::Updated
                        },
                        price_changed: false,
                    })
                })
                .collect();
            let property = self.repo.property(existing[0].property_id).await?;
            return Ok(IngestOutcome {
                property_id: existing[0].property_id,
                external_id,
                listings,
                warnings: n.warnings,
                notes: vec![],
                coordinates: property.as_ref().and_then(PropertyRow::coordinates),
            });
        }

        let mut notes = Vec::new();
        self.complete_address(&mut n.property.address, &mut n.warnings, &mut notes)
            .await;

        let mut keep_existing_coordinates = false;
        if n.property.coordinates.is_none() {
            if let Some(e) = existing.first() {
                if let Some(current) = self.repo.property(e.property_id).await? {
                    keep_existing_coordinates = current.coordinates().is_some()
                        && same_location(&current, &n.property.address);
                }
            }
            if !keep_existing_coordinates {
                n.property.coordinates = self.geocode(&n.property.address, &mut notes).await;
            }
        }

        let write = ListingGroupWrite {
            source: n.source,
            partner_id: n.partner_id,
            external_id: &external_id,
            property: &n.property,
            keep_existing_coordinates,
            listings: n.listings.iter().zip(hashes).collect(),
            original: &n.original,
            raw_payload: &n.raw_payload,
            now,
        };
        let result = self.repo.write_group(&write).await?;
        let coordinates = if keep_existing_coordinates {
            self.repo
                .property(result.property_id)
                .await?
                .as_ref()
                .and_then(PropertyRow::coordinates)
        } else {
            n.property.coordinates
        };
        Ok(IngestOutcome {
            property_id: result.property_id,
            external_id,
            listings: result.listings,
            warnings: n.warnings,
            notes,
            coordinates,
        })
    }

    /// CEP → endereço oficial, preenchendo só o que falta. Um município ou
    /// UF divergente vira aviso e o valor informado é mantido.
    async fn complete_address(
        &self,
        address: &mut Address,
        warnings: &mut Vec<FieldIssue>,
        notes: &mut Vec<String>,
    ) {
        let needs = address.municipality.is_none()
            || address.state.is_none()
            || address.municipality_ibge_code.is_none()
            || address.street.is_none()
            || address.neighborhood.is_none();
        if let (Some(lookup), Some(cep), true) = (&self.postal, address.postal_code.clone(), needs)
        {
            match lookup.lookup(&cep).await {
                Ok(Some(found)) => {
                    let differs = |given: &Option<String>, official: &Option<String>| matches!((given, official), (Some(g), Some(o)) if slugify(g) != slugify(o));
                    if differs(&address.municipality, &found.municipality)
                        || differs(&address.state, &found.state)
                    {
                        warnings.push(FieldIssue {
                            field: "postal_code".into(),
                            message: format!(
                                "o CEP pertence a {} - {}, diferente do município/UF informado",
                                found.municipality.as_deref().unwrap_or("?"),
                                found.state.as_deref().unwrap_or("?")
                            ),
                        });
                    } else {
                        let filled = fill(&mut address.street, &found.street)
                            | fill(&mut address.neighborhood, &found.neighborhood)
                            | fill(&mut address.municipality, &found.municipality)
                            | fill(&mut address.state, &found.state)
                            | fill(
                                &mut address.municipality_ibge_code,
                                &found.municipality_ibge_code,
                            );
                        if address.neighborhood_slug.is_none() {
                            address.neighborhood_slug =
                                address.neighborhood.as_deref().map(slugify);
                        }
                        if filled {
                            notes
                                .push(format!("Endereço completado pelo CEP ({}).", lookup.name()));
                        }
                    }
                }
                Ok(None) => warnings.push(FieldIssue {
                    field: "postal_code".into(),
                    message: format!("CEP não encontrado em {}", lookup.name()),
                }),
                Err(e) => {
                    tracing::warn!(error = %e, lookup = lookup.name(), "consulta de CEP falhou")
                }
            }
        }
        if address.municipality_ibge_code.is_none() {
            if let (Some(uf), Some(city)) = (&address.state, &address.municipality) {
                address.municipality_ibge_code = self
                    .normalizer
                    .gazetteer()
                    .lookup(uf, city)
                    .map(str::to_string);
            }
        }
    }

    /// Endereço → coordenada, passando pelo cache (inclusive para "não
    /// encontrado"). Falha do serviço não impede a gravação.
    async fn geocode(&self, address: &Address, notes: &mut Vec<String>) -> Option<Coordinates> {
        let geocoder = self.geocoder.as_ref()?;
        let query = GeocodeQuery::from_address(address);
        if !query.is_usable() {
            return None;
        }
        let key = query.cache_key(geocoder.name());
        let cached = match self.repo.geocode_cache_get(&key).await {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(error = %e, "cache de geocoding indisponível");
                None
            }
        };
        let result = match cached {
            Some(row) if row.found => row
                .lat
                .zip(row.lon)
                .and_then(|(lat, lon)| GeoPoint::new(lat, lon))
                .map(|p| {
                    let precision = row
                        .precision
                        .as_deref()
                        .and_then(CoordinatePrecision::parse)
                        .unwrap_or(CoordinatePrecision::Approximate);
                    (p, precision)
                }),
            Some(_) => None,
            None => match geocoder.geocode(&query).await {
                Ok(found) => {
                    let value = found.as_ref().map(|r| (r.point, r.precision));
                    let payload = serde_json::json!({ "label": found.as_ref().and_then(|r| r.label.clone()) });
                    if let Err(e) = self
                        .repo
                        .geocode_cache_put(&key, geocoder.name(), value, &payload)
                        .await
                    {
                        tracing::warn!(error = %e, "falha ao gravar cache de geocoding");
                    }
                    value
                }
                Err(e) => {
                    tracing::warn!(error = %e, geocoder = geocoder.name(), "geocoding falhou");
                    None
                }
            },
        };
        let (point, precision) = result?;
        notes.push(format!(
            "Coordenada estimada pelo endereço ({}), precisão {}.",
            geocoder.name(),
            precision.as_str()
        ));
        Some(Coordinates {
            point,
            source: CoordinateSource::Geocoding,
            precision,
        })
    }
}

fn fill(target: &mut Option<String>, value: &Option<String>) -> bool {
    if target.is_none() && value.is_some() {
        *target = value.clone();
        true
    } else {
        false
    }
}

/// Mesmo endereço do que está gravado (para não geocodificar de novo).
fn same_location(current: &PropertyRow, address: &Address) -> bool {
    let eq = |a: &Option<String>, b: &Option<String>| {
        a.as_deref().map(slugify) == b.as_deref().map(slugify)
    };
    eq(&current.street, &address.street)
        && eq(&current.street_number, &address.number)
        && eq(&current.postal_code, &address.postal_code)
        && eq(&current.municipality, &address.municipality)
        && eq(&current.state, &address.state)
}

/// Fonte de um item, para logs.
pub fn source_label(source: PropertySource, partner: Option<&str>) -> String {
    match partner {
        Some(p) => format!("{}:{p}", source.as_str()),
        None => source.as_str().to_string(),
    }
}

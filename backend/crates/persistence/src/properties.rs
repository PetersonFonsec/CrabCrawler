//! Imóveis (`properties`), anúncios (`listings`), histórico de preço,
//! parceiros, execuções de sincronização e cache de geocoding.

use chrono::{DateTime, Utc};
use crab_domain::{
    CoordinatePrecision, CoordinateSource, Coordinates, GeoPoint, ListingDetails, ListingStatus,
    PartnerStatus, PartnerType, PriceObservation, PropertyDetails, PropertySource,
    PropertySourcePartner, SyncItemError, SyncMetrics, SyncRunStatus, TransactionType,
};
use serde::Serialize;
use serde_json::Value;
use sqlx::types::Json;
use sqlx::{FromRow, PgPool, Postgres, QueryBuilder, Transaction};
use uuid::Uuid;

/// Imóvel físico como exposto para leitura.
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct PropertyRow {
    pub id: Uuid,
    pub property_type: String,
    pub area_m2: Option<f64>,
    pub lot_area_m2: Option<f64>,
    pub bedrooms: Option<i32>,
    pub bathrooms: Option<i32>,
    pub suites: Option<i32>,
    pub parking_spaces: Option<i32>,
    pub street: Option<String>,
    pub street_number: Option<String>,
    pub complement: Option<String>,
    pub neighborhood: Option<String>,
    pub neighborhood_slug: Option<String>,
    pub municipality: Option<String>,
    pub municipality_ibge_code: Option<String>,
    pub state: Option<String>,
    pub postal_code: Option<String>,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub coordinate_source: Option<String>,
    pub coordinate_precision: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

const PROPERTY_SELECT: &str = "SELECT p.id, p.property_type, p.area_m2, p.lot_area_m2, \
    p.bedrooms, p.bathrooms, p.suites, p.parking_spaces, p.street, p.street_number, \
    p.complement, p.neighborhood, p.neighborhood_slug, p.municipality, \
    p.municipality_ibge_code, p.state, p.postal_code, \
    ST_Y(p.geom::geometry) AS lat, ST_X(p.geom::geometry) AS lon, \
    p.coordinate_source, p.coordinate_precision, p.created_at, p.updated_at FROM properties p";

/// Anúncio com proveniência completa (sem o payload bruto, que fica só no
/// banco para auditoria).
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct PropertyListingRow {
    pub id: Uuid,
    pub property_id: Uuid,
    pub source: String,
    pub partner_id: Option<Uuid>,
    pub external_id: String,
    pub title: String,
    pub transaction: String,
    pub status: String,
    pub price_brl: f64,
    pub condominium_fee_brl: Option<f64>,
    pub property_tax_brl: Option<f64>,
    pub description: Option<String>,
    pub notes: Option<String>,
    pub source_url: Option<String>,
    pub images: Json<Value>,
    pub original: Json<Value>,
    pub content_hash: Option<String>,
    pub first_seen_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub imported_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

const LISTING_SELECT: &str = "SELECT l.id, l.property_id, l.source, l.partner_id, \
    l.external_id, l.title, l.transaction, l.status, l.price_brl, l.condominium_fee_brl, \
    l.property_tax_brl, l.description, l.notes, l.source_url, l.images, l.original, \
    l.content_hash, l.first_seen_at, l.last_seen_at, l.imported_at, l.updated_at FROM listings l";

/// Anúncio já gravado, como visto antes de decidir o que fazer com um item.
#[derive(Debug, Clone, FromRow)]
pub struct ExistingListing {
    pub listing_id: Uuid,
    pub property_id: Uuid,
    pub transaction: String,
    pub status: String,
    pub content_hash: Option<String>,
}

/// O que gravar para um item normalizado (um imóvel e seus anúncios).
#[derive(Debug, Clone)]
pub struct ListingGroupWrite<'a> {
    pub source: PropertySource,
    pub partner_id: Option<Uuid>,
    pub external_id: &'a str,
    pub property: &'a PropertyDetails,
    /// Sem coordenada nova: `true` mantém a que já está gravada (endereço
    /// não mudou); `false` apaga (endereço mudou e não houve geocoding).
    pub keep_existing_coordinates: bool,
    pub listings: Vec<(&'a ListingDetails, String)>,
    pub original: &'a Value,
    pub raw_payload: &'a Value,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WriteOutcome {
    Created,
    Updated,
    Unchanged,
}

#[derive(Debug, Clone, Serialize)]
pub struct ListingWriteResult {
    pub listing_id: Uuid,
    pub transaction: TransactionType,
    pub outcome: WriteOutcome,
    pub price_changed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct GroupWriteResult {
    pub property_id: Uuid,
    pub listings: Vec<ListingWriteResult>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct PropertyFilter {
    pub municipality_ibge_code: Option<String>,
    pub neighborhood: Option<String>,
    pub property_type: Option<String>,
    pub transaction: Option<String>,
    pub source: Option<String>,
    pub status: Option<String>,
    pub min_bedrooms: Option<i32>,
    pub max_price: Option<f64>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct SyncRunRow {
    pub id: i64,
    pub provider: String,
    pub partner_id: Option<Uuid>,
    pub status: String,
    pub received: i32,
    pub created: i32,
    pub updated: i32,
    pub unchanged: i32,
    pub invalid: i32,
    pub deactivated: i32,
    pub failed: i32,
    pub duration_ms: Option<i64>,
    pub error: Option<String>,
    pub item_errors: Json<Value>,
    pub details: Json<Value>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

/// Fim de uma sincronização.
#[derive(Debug, Clone)]
pub struct SyncRunFinish<'a> {
    pub status: SyncRunStatus,
    pub metrics: SyncMetrics,
    pub duration_ms: i64,
    pub error: Option<&'a str>,
    pub item_errors: &'a [SyncItemError],
    pub details: Value,
}

/// Entrada do cache de geocoding (inclusive "não encontrado").
#[derive(Debug, Clone, FromRow)]
pub struct GeocodeCacheRow {
    pub provider: String,
    pub found: bool,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub precision: Option<String>,
    pub payload: Json<Value>,
}

#[derive(Clone)]
pub struct PropertyRepository {
    pool: PgPool,
}

impl PropertyRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Anúncios já gravados para o mesmo item (todas as finalidades).
    pub async fn find_by_identity(
        &self,
        source: PropertySource,
        partner_id: Option<Uuid>,
        external_id: &str,
    ) -> Result<Vec<ExistingListing>, sqlx::Error> {
        sqlx::query_as(
            "SELECT id AS listing_id, property_id, transaction, status, content_hash \
             FROM listings WHERE source = $1 AND partner_id IS NOT DISTINCT FROM $2 \
             AND external_id = $3",
        )
        .bind(source.as_str())
        .bind(partner_id)
        .bind(external_id)
        .fetch_all(&self.pool)
        .await
    }

    /// Item sem mudança: só registra que foi visto (e reativa se estava
    /// inativo).
    pub async fn touch(&self, listing_ids: &[Uuid], now: DateTime<Utc>) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE listings SET last_seen_at = $2, \
                status = CASE WHEN status = 'REMOVED' THEN status ELSE 'ACTIVE' END \
             WHERE id = ANY($1)",
        )
        .bind(listing_ids)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Item que veio no feed mas não pôde ser lido: continua visto, com
    /// status `UNKNOWN`, para não ser inativado por um erro de validação.
    pub async fn mark_unreadable(
        &self,
        source: PropertySource,
        partner_id: Option<Uuid>,
        external_ids: &[String],
        now: DateTime<Utc>,
    ) -> Result<u64, sqlx::Error> {
        let done = sqlx::query(
            "UPDATE listings SET last_seen_at = $4, status = 'UNKNOWN', updated_at = $4 \
             WHERE source = $1 AND partner_id IS NOT DISTINCT FROM $2 \
             AND external_id = ANY($3) AND status IN ('ACTIVE', 'INACTIVE', 'UNKNOWN')",
        )
        .bind(source.as_str())
        .bind(partner_id)
        .bind(external_ids)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected())
    }

    /// Endereço e coordenada atuais do imóvel, para decidir se é preciso
    /// geocodificar de novo.
    pub async fn property(&self, id: Uuid) -> Result<Option<PropertyRow>, sqlx::Error> {
        sqlx::query_as(&format!("{PROPERTY_SELECT} WHERE p.id = $1"))
            .bind(id)
            .fetch_optional(&self.pool)
            .await
    }

    /// Grava um imóvel e seus anúncios numa transação: cria o que é novo,
    /// atualiza o que mudou e registra preço novo no histórico.
    pub async fn write_group(
        &self,
        w: &ListingGroupWrite<'_>,
    ) -> Result<GroupWriteResult, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let existing: Vec<ExistingListing> = sqlx::query_as(
            "SELECT id AS listing_id, property_id, transaction, status, content_hash \
             FROM listings WHERE source = $1 AND partner_id IS NOT DISTINCT FROM $2 \
             AND external_id = $3 FOR UPDATE",
        )
        .bind(w.source.as_str())
        .bind(w.partner_id)
        .bind(w.external_id)
        .fetch_all(&mut *tx)
        .await?;

        let property_id = match existing.first() {
            Some(e) => {
                update_property(
                    &mut tx,
                    e.property_id,
                    w.property,
                    w.keep_existing_coordinates,
                    w.now,
                )
                .await?;
                e.property_id
            }
            None => {
                let id = Uuid::new_v4();
                insert_property(&mut tx, id, w.property, w.now).await?;
                id
            }
        };

        let mut results = Vec::new();
        for (details, hash) in &w.listings {
            let current = existing
                .iter()
                .find(|e| e.transaction == details.transaction.as_str());
            let result = match current {
                None => {
                    let id = Uuid::new_v4();
                    insert_listing(&mut tx, id, property_id, w, details, hash).await?;
                    insert_price(&mut tx, id, details.price_brl, w.now).await?;
                    ListingWriteResult {
                        listing_id: id,
                        transaction: details.transaction,
                        outcome: WriteOutcome::Created,
                        price_changed: false,
                    }
                }
                Some(e) if e.content_hash.as_deref() == Some(hash.as_str()) => {
                    sqlx::query(
                        "UPDATE listings SET last_seen_at = $2, \
                            status = CASE WHEN status = 'REMOVED' THEN status ELSE 'ACTIVE' END \
                         WHERE id = $1",
                    )
                    .bind(e.listing_id)
                    .bind(w.now)
                    .execute(&mut *tx)
                    .await?;
                    ListingWriteResult {
                        listing_id: e.listing_id,
                        transaction: details.transaction,
                        outcome: if e.status == "ACTIVE" {
                            WriteOutcome::Unchanged
                        } else {
                            WriteOutcome::Updated
                        },
                        price_changed: false,
                    }
                }
                Some(e) => {
                    update_listing(&mut tx, e.listing_id, w, details, hash).await?;
                    let price_changed =
                        record_price_if_changed(&mut tx, e.listing_id, details.price_brl, w.now)
                            .await?;
                    ListingWriteResult {
                        listing_id: e.listing_id,
                        transaction: details.transaction,
                        outcome: WriteOutcome::Updated,
                        price_changed,
                    }
                }
            };
            results.push(result);
        }
        tx.commit().await?;
        Ok(GroupWriteResult {
            property_id,
            listings: results,
        })
    }

    /// Anúncios de um parceiro que não apareceram desde `seen_before` viram
    /// `INACTIVE`. Nada é apagado.
    pub async fn deactivate_missing(
        &self,
        source: PropertySource,
        partner_id: Option<Uuid>,
        seen_before: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<u64, sqlx::Error> {
        let done = sqlx::query(
            "UPDATE listings SET status = 'INACTIVE', updated_at = $4 \
             WHERE source = $1 AND partner_id IS NOT DISTINCT FROM $2 \
             AND status IN ('ACTIVE', 'UNKNOWN') AND last_seen_at < $3",
        )
        .bind(source.as_str())
        .bind(partner_id)
        .bind(seen_before)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected())
    }

    /// Quantos anúncios seriam inativados por `deactivate_missing`.
    pub async fn count_missing(
        &self,
        source: PropertySource,
        partner_id: Option<Uuid>,
        seen_before: DateTime<Utc>,
    ) -> Result<i64, sqlx::Error> {
        let (n,): (i64,) = sqlx::query_as(
            "SELECT count(*) FROM listings WHERE source = $1 \
             AND partner_id IS NOT DISTINCT FROM $2 AND status IN ('ACTIVE', 'UNKNOWN') \
             AND last_seen_at < $3",
        )
        .bind(source.as_str())
        .bind(partner_id)
        .bind(seen_before)
        .fetch_one(&self.pool)
        .await?;
        Ok(n)
    }

    /// Quantos anúncios ativos o parceiro tem (base para a trava de
    /// inativação em massa).
    pub async fn count_active(
        &self,
        source: PropertySource,
        partner_id: Option<Uuid>,
    ) -> Result<i64, sqlx::Error> {
        let (n,): (i64,) = sqlx::query_as(
            "SELECT count(*) FROM listings WHERE source = $1 \
             AND partner_id IS NOT DISTINCT FROM $2 AND status IN ('ACTIVE', 'UNKNOWN')",
        )
        .bind(source.as_str())
        .bind(partner_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(n)
    }

    pub async fn search(&self, f: &PropertyFilter) -> Result<Vec<PropertyRow>, sqlx::Error> {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(PROPERTY_SELECT);
        qb.push(" WHERE TRUE");
        if let Some(code) = &f.municipality_ibge_code {
            qb.push(" AND p.municipality_ibge_code = ")
                .push_bind(code.clone());
        }
        if let Some(slug) = &f.neighborhood {
            qb.push(" AND p.neighborhood_slug = ")
                .push_bind(slug.clone());
        }
        if let Some(t) = &f.property_type {
            qb.push(" AND p.property_type = ").push_bind(t.clone());
        }
        if let Some(n) = f.min_bedrooms {
            qb.push(" AND p.bedrooms >= ").push_bind(n);
        }
        let listing_filter = f.transaction.is_some()
            || f.source.is_some()
            || f.status.is_some()
            || f.max_price.is_some();
        if listing_filter {
            qb.push(" AND EXISTS (SELECT 1 FROM listings l WHERE l.property_id = p.id");
            if let Some(t) = &f.transaction {
                qb.push(" AND l.transaction = ").push_bind(t.clone());
            }
            if let Some(s) = &f.source {
                qb.push(" AND l.source = ").push_bind(s.to_uppercase());
            }
            if let Some(s) = &f.status {
                qb.push(" AND l.status = ").push_bind(s.to_uppercase());
            }
            if let Some(p) = f.max_price {
                qb.push(" AND l.price_brl <= ").push_bind(p);
            }
            qb.push(")");
        }
        qb.push(" ORDER BY p.updated_at DESC LIMIT ")
            .push_bind(f.limit.unwrap_or(50).clamp(1, 500));
        qb.build_query_as().fetch_all(&self.pool).await
    }

    pub async fn listings_of(
        &self,
        property_ids: &[Uuid],
    ) -> Result<Vec<PropertyListingRow>, sqlx::Error> {
        sqlx::query_as(&format!(
            "{LISTING_SELECT} WHERE l.property_id = ANY($1) \
             ORDER BY (l.status = 'ACTIVE') DESC, l.updated_at DESC"
        ))
        .bind(property_ids)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn listing(&self, id: Uuid) -> Result<Option<PropertyListingRow>, sqlx::Error> {
        sqlx::query_as(&format!("{LISTING_SELECT} WHERE l.id = $1"))
            .bind(id)
            .fetch_optional(&self.pool)
            .await
    }

    /// Histórico de preço dos anúncios, mais antigo primeiro.
    pub async fn price_history(
        &self,
        listing_ids: &[Uuid],
    ) -> Result<Vec<PriceObservation>, sqlx::Error> {
        let rows: Vec<(Uuid, f64, DateTime<Utc>)> = sqlx::query_as(
            "SELECT listing_id, price_brl, observed_at FROM listing_price_history \
             WHERE listing_id = ANY($1) ORDER BY observed_at, id",
        )
        .bind(listing_ids)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(listing_id, price_brl, observed_at)| PriceObservation {
                listing_id,
                price_brl,
                observed_at,
            })
            .collect())
    }

    // ------------------------------------------------------------ parceiros

    pub async fn create_partner(&self, p: &PropertySourcePartner) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO property_source_partners \
                (id, slug, name, partner_type, status, provider, configuration, created_at, updated_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
        )
        .bind(p.id)
        .bind(&p.slug)
        .bind(&p.name)
        .bind(p.partner_type.as_str())
        .bind(p.status.as_str())
        .bind(p.provider.as_str())
        .bind(Json(&p.configuration))
        .bind(p.created_at)
        .bind(p.updated_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn set_partner_status(
        &self,
        slug: &str,
        status: PartnerStatus,
    ) -> Result<bool, sqlx::Error> {
        let done = sqlx::query(
            "UPDATE property_source_partners SET status = $2, updated_at = now() WHERE slug = $1",
        )
        .bind(slug)
        .bind(status.as_str())
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected() > 0)
    }

    pub async fn partner(&self, slug: &str) -> Result<Option<PropertySourcePartner>, sqlx::Error> {
        let row: Option<PartnerRow> = sqlx::query_as(&format!("{PARTNER_SELECT} WHERE slug = $1"))
            .bind(slug)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.and_then(PartnerRow::into_domain))
    }

    pub async fn partners(&self) -> Result<Vec<PropertySourcePartner>, sqlx::Error> {
        let rows: Vec<PartnerRow> = sqlx::query_as(&format!("{PARTNER_SELECT} ORDER BY slug"))
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .filter_map(PartnerRow::into_domain)
            .collect())
    }

    // ------------------------------------------------------- sincronizações

    pub async fn start_run(
        &self,
        provider: PropertySource,
        partner_id: Option<Uuid>,
        started_at: DateTime<Utc>,
    ) -> Result<i64, sqlx::Error> {
        let (id,): (i64,) = sqlx::query_as(
            "INSERT INTO property_sync_runs (provider, partner_id, status, started_at) \
             VALUES ($1, $2, 'RUNNING', $3) RETURNING id",
        )
        .bind(provider.as_str())
        .bind(partner_id)
        .bind(started_at)
        .fetch_one(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn finish_run(&self, id: i64, f: &SyncRunFinish<'_>) -> Result<(), sqlx::Error> {
        let m = &f.metrics;
        sqlx::query(
            "UPDATE property_sync_runs SET status = $2, received = $3, created = $4, \
                updated = $5, unchanged = $6, invalid = $7, deactivated = $8, failed = $9, \
                duration_ms = $10, error = $11, item_errors = $12, details = $13, \
                finished_at = now() WHERE id = $1",
        )
        .bind(id)
        .bind(f.status.as_str())
        .bind(m.received as i32)
        .bind(m.created as i32)
        .bind(m.updated as i32)
        .bind(m.unchanged as i32)
        .bind(m.invalid as i32)
        .bind(m.deactivated as i32)
        .bind(m.failed as i32)
        .bind(f.duration_ms)
        .bind(f.error)
        .bind(Json(f.item_errors))
        .bind(Json(&f.details))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn recent_runs(&self, limit: i64) -> Result<Vec<SyncRunRow>, sqlx::Error> {
        sqlx::query_as(
            "SELECT id, provider, partner_id, status, received, created, updated, unchanged, \
                invalid, deactivated, failed, duration_ms, error, item_errors, details, \
                started_at, finished_at \
             FROM property_sync_runs ORDER BY started_at DESC, id DESC LIMIT $1",
        )
        .bind(limit.clamp(1, 200))
        .fetch_all(&self.pool)
        .await
    }

    // ------------------------------------------------------------ geocoding

    pub async fn geocode_cache_get(
        &self,
        key: &str,
    ) -> Result<Option<GeocodeCacheRow>, sqlx::Error> {
        sqlx::query_as(
            "SELECT provider, found, lat, lon, precision, payload FROM geocoding_cache \
             WHERE query_key = $1",
        )
        .bind(key)
        .fetch_optional(&self.pool)
        .await
    }

    pub async fn geocode_cache_put(
        &self,
        key: &str,
        provider: &str,
        result: Option<(GeoPoint, CoordinatePrecision)>,
        payload: &Value,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO geocoding_cache (query_key, provider, found, lat, lon, precision, payload) \
             VALUES ($1,$2,$3,$4,$5,$6,$7) \
             ON CONFLICT (query_key) DO UPDATE SET provider = EXCLUDED.provider, \
                found = EXCLUDED.found, lat = EXCLUDED.lat, lon = EXCLUDED.lon, \
                precision = EXCLUDED.precision, payload = EXCLUDED.payload, created_at = now()",
        )
        .bind(key)
        .bind(provider)
        .bind(result.is_some())
        .bind(result.map(|(p, _)| p.lat))
        .bind(result.map(|(p, _)| p.lon))
        .bind(result.map(|(_, prec)| prec.as_str()))
        .bind(Json(payload))
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

impl PropertyRow {
    pub fn coordinates(&self) -> Option<Coordinates> {
        let point = GeoPoint::new(self.lat?, self.lon?)?;
        Some(Coordinates {
            point,
            source: CoordinateSource::parse(self.coordinate_source.as_deref()?)?,
            precision: CoordinatePrecision::parse(self.coordinate_precision.as_deref()?)?,
        })
    }
}

impl PropertyListingRow {
    pub fn status(&self) -> ListingStatus {
        ListingStatus::parse(&self.status).unwrap_or(ListingStatus::Unknown)
    }
}

const PARTNER_SELECT: &str = "SELECT id, slug, name, partner_type, status, provider, \
    configuration, created_at, updated_at FROM property_source_partners";

#[derive(FromRow)]
struct PartnerRow {
    id: Uuid,
    slug: String,
    name: String,
    partner_type: String,
    status: String,
    provider: String,
    configuration: Json<Value>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl PartnerRow {
    fn into_domain(self) -> Option<PropertySourcePartner> {
        Some(PropertySourcePartner {
            id: self.id,
            slug: self.slug,
            name: self.name,
            partner_type: PartnerType::parse(&self.partner_type)?,
            status: PartnerStatus::parse(&self.status)?,
            provider: PropertySource::parse(&self.provider)?,
            configuration: self.configuration.0,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

fn opt_i32(v: Option<u16>) -> Option<i32> {
    v.map(i32::from)
}

async fn insert_property(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    p: &PropertyDetails,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    let a = &p.address;
    let c = p.coordinates;
    sqlx::query(
        "INSERT INTO properties (id, property_type, area_m2, lot_area_m2, bedrooms, bathrooms, \
            suites, parking_spaces, street, street_number, complement, neighborhood, \
            neighborhood_slug, municipality, municipality_ibge_code, state, postal_code, geom, \
            coordinate_source, coordinate_precision, created_at, updated_at) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17, \
            CASE WHEN $18::float8 IS NULL THEN NULL \
                 ELSE ST_SetSRID(ST_MakePoint($19, $18), 4326)::geography END, \
            $20,$21,$22,$22)",
    )
    .bind(id)
    .bind(p.property_type.as_str())
    .bind(p.area_m2)
    .bind(p.lot_area_m2)
    .bind(opt_i32(p.bedrooms))
    .bind(opt_i32(p.bathrooms))
    .bind(opt_i32(p.suites))
    .bind(opt_i32(p.parking_spaces))
    .bind(&a.street)
    .bind(&a.number)
    .bind(&a.complement)
    .bind(&a.neighborhood)
    .bind(&a.neighborhood_slug)
    .bind(&a.municipality)
    .bind(&a.municipality_ibge_code)
    .bind(&a.state)
    .bind(&a.postal_code)
    .bind(c.map(|c| c.point.lat))
    .bind(c.map(|c| c.point.lon))
    .bind(c.map(|c| c.source.as_str()))
    .bind(c.map(|c| c.precision.as_str()))
    .bind(now)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn update_property(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    p: &PropertyDetails,
    keep_existing_coordinates: bool,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    let a = &p.address;
    let c = p.coordinates;
    sqlx::query(
        "UPDATE properties SET property_type = $2, area_m2 = $3, lot_area_m2 = $4, \
            bedrooms = $5, bathrooms = $6, suites = $7, parking_spaces = $8, street = $9, \
            street_number = $10, complement = $11, neighborhood = $12, neighborhood_slug = $13, \
            municipality = $14, municipality_ibge_code = $15, state = $16, postal_code = $17, \
            geom = CASE WHEN $18::float8 IS NOT NULL \
                    THEN ST_SetSRID(ST_MakePoint($19, $18), 4326)::geography \
                    WHEN $23 THEN geom ELSE NULL END, \
            coordinate_source = CASE WHEN $18::float8 IS NOT NULL THEN $20 \
                    WHEN $23 THEN coordinate_source ELSE NULL END, \
            coordinate_precision = CASE WHEN $18::float8 IS NOT NULL THEN $21 \
                    WHEN $23 THEN coordinate_precision ELSE NULL END, \
            updated_at = $22 \
         WHERE id = $1",
    )
    .bind(id)
    .bind(p.property_type.as_str())
    .bind(p.area_m2)
    .bind(p.lot_area_m2)
    .bind(opt_i32(p.bedrooms))
    .bind(opt_i32(p.bathrooms))
    .bind(opt_i32(p.suites))
    .bind(opt_i32(p.parking_spaces))
    .bind(&a.street)
    .bind(&a.number)
    .bind(&a.complement)
    .bind(&a.neighborhood)
    .bind(&a.neighborhood_slug)
    .bind(&a.municipality)
    .bind(&a.municipality_ibge_code)
    .bind(&a.state)
    .bind(&a.postal_code)
    .bind(c.map(|c| c.point.lat))
    .bind(c.map(|c| c.point.lon))
    .bind(c.map(|c| c.source.as_str()))
    .bind(c.map(|c| c.precision.as_str()))
    .bind(now)
    .bind(keep_existing_coordinates)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn insert_listing(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    property_id: Uuid,
    w: &ListingGroupWrite<'_>,
    d: &ListingDetails,
    hash: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO listings (id, property_id, source, partner_id, external_id, title, \
            transaction, status, price_brl, condominium_fee_brl, property_tax_brl, description, \
            notes, source_url, images, original, raw_payload, content_hash, first_seen_at, \
            last_seen_at, imported_at, collected_at, updated_at) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,'ACTIVE',$8,$9,$10,$11,$12,$13,$14,$15,$16,$17, \
            $18,$18,$18,$18,$18)",
    )
    .bind(id)
    .bind(property_id)
    .bind(w.source.as_str())
    .bind(w.partner_id)
    .bind(w.external_id)
    .bind(d.title.as_deref().unwrap_or("Imóvel"))
    .bind(d.transaction.as_str())
    .bind(d.price_brl)
    .bind(d.condominium_fee_brl)
    .bind(d.property_tax_brl)
    .bind(&d.description)
    .bind(&d.notes)
    .bind(&d.source_url)
    .bind(Json(&d.images))
    .bind(Json(w.original))
    .bind(Json(w.raw_payload))
    .bind(hash)
    .bind(w.now)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn update_listing(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    w: &ListingGroupWrite<'_>,
    d: &ListingDetails,
    hash: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE listings SET title = $2, price_brl = $3, condominium_fee_brl = $4, \
            property_tax_brl = $5, description = $6, notes = $7, source_url = $8, images = $9, \
            original = $10, raw_payload = $11, content_hash = $12, last_seen_at = $13, \
            imported_at = $13, collected_at = $13, updated_at = $13, \
            status = CASE WHEN status = 'REMOVED' THEN status ELSE 'ACTIVE' END \
         WHERE id = $1",
    )
    .bind(id)
    .bind(d.title.as_deref().unwrap_or("Imóvel"))
    .bind(d.price_brl)
    .bind(d.condominium_fee_brl)
    .bind(d.property_tax_brl)
    .bind(&d.description)
    .bind(&d.notes)
    .bind(&d.source_url)
    .bind(Json(&d.images))
    .bind(Json(w.original))
    .bind(Json(w.raw_payload))
    .bind(hash)
    .bind(w.now)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn insert_price(
    tx: &mut Transaction<'_, Postgres>,
    listing_id: Uuid,
    price: f64,
    at: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO listing_price_history (listing_id, price_brl, observed_at) VALUES ($1,$2,$3)",
    )
    .bind(listing_id)
    .bind(price)
    .bind(at)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Nova linha no histórico só quando o preço difere do último registrado.
async fn record_price_if_changed(
    tx: &mut Transaction<'_, Postgres>,
    listing_id: Uuid,
    price: f64,
    at: DateTime<Utc>,
) -> Result<bool, sqlx::Error> {
    let last: Option<(f64,)> = sqlx::query_as(
        "SELECT price_brl FROM listing_price_history WHERE listing_id = $1 \
         ORDER BY observed_at DESC, id DESC LIMIT 1",
    )
    .bind(listing_id)
    .fetch_optional(&mut **tx)
    .await?;
    if last.is_some_and(|(p,)| p == price) {
        return Ok(false);
    }
    insert_price(tx, listing_id, price, at).await?;
    Ok(last.is_some())
}

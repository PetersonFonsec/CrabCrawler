use chrono::{DateTime, Utc};
use crab_domain::Listing;
use serde::Serialize;
use sqlx::{FromRow, PgPool, Postgres, QueryBuilder};
use uuid::Uuid;

use crate::enum_str;

/// Linha de `listings` como exposta para leitura (API).
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct ListingRow {
    pub id: Uuid,
    pub source: String,
    pub external_id: String,
    pub title: String,
    pub transaction: String,
    pub kind: String,
    pub price_brl: Option<f64>,
    pub area_m2: Option<f64>,
    pub bedrooms: Option<i32>,
    pub bathrooms: Option<i32>,
    pub parking_spots: Option<i32>,
    pub state: Option<String>,
    pub municipality: Option<String>,
    pub municipality_ibge_code: Option<String>,
    pub neighborhood: Option<String>,
    pub neighborhood_slug: Option<String>,
    pub postal_code: Option<String>,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub source_url: Option<String>,
    pub collected_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct ListingFilter {
    pub municipality_ibge_code: Option<String>,
    pub neighborhood: Option<String>,
    pub transaction: Option<String>,
    pub min_bedrooms: Option<i32>,
    pub max_price: Option<f64>,
    pub limit: Option<i64>,
}

const SELECT: &str = "SELECT id, source, external_id, title, transaction, kind, price_brl, \
    area_m2, bedrooms, bathrooms, parking_spots, state, municipality, municipality_ibge_code, \
    neighborhood, neighborhood_slug, postal_code, \
    ST_Y(geom::geometry) AS lat, ST_X(geom::geometry) AS lon, \
    source_url, collected_at, updated_at FROM listings";

#[derive(Clone)]
pub struct ListingRepository {
    pool: PgPool,
}

impl ListingRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Insere ou atualiza pelo par (source, external_id).
    pub async fn upsert(&self, listing: &Listing) -> Result<(), sqlx::Error> {
        let loc = &listing.location;
        sqlx::query(
            "INSERT INTO listings (id, source, external_id, title, transaction, kind, price_brl, \
                area_m2, bedrooms, bathrooms, parking_spots, state, municipality, \
                municipality_ibge_code, neighborhood, neighborhood_slug, postal_code, geom, \
                source_url, collected_at, updated_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17, \
                CASE WHEN $18::float8 IS NULL OR $19::float8 IS NULL THEN NULL \
                     ELSE ST_SetSRID(ST_MakePoint($19, $18), 4326)::geography END, \
                $20,$21,$22) \
             ON CONFLICT (source, external_id) DO UPDATE SET \
                title = EXCLUDED.title, transaction = EXCLUDED.transaction, kind = EXCLUDED.kind, \
                price_brl = EXCLUDED.price_brl, area_m2 = EXCLUDED.area_m2, \
                bedrooms = EXCLUDED.bedrooms, bathrooms = EXCLUDED.bathrooms, \
                parking_spots = EXCLUDED.parking_spots, state = EXCLUDED.state, \
                municipality = EXCLUDED.municipality, \
                municipality_ibge_code = EXCLUDED.municipality_ibge_code, \
                neighborhood = EXCLUDED.neighborhood, neighborhood_slug = EXCLUDED.neighborhood_slug, \
                postal_code = EXCLUDED.postal_code, geom = EXCLUDED.geom, \
                source_url = EXCLUDED.source_url, collected_at = EXCLUDED.collected_at, \
                updated_at = EXCLUDED.updated_at",
        )
        .bind(listing.id)
        .bind(listing.provenance.source.as_str())
        .bind(&listing.external_id)
        .bind(&listing.title)
        .bind(enum_str(&listing.transaction))
        .bind(enum_str(&listing.kind))
        .bind(listing.price_brl)
        .bind(listing.area_m2)
        .bind(listing.bedrooms.map(i32::from))
        .bind(listing.bathrooms.map(i32::from))
        .bind(listing.parking_spots.map(i32::from))
        .bind(&loc.state)
        .bind(&loc.municipality)
        .bind(&loc.municipality_ibge_code)
        .bind(&loc.neighborhood)
        .bind(&loc.neighborhood_slug)
        .bind(&loc.postal_code)
        .bind(loc.point.map(|p| p.lat))
        .bind(loc.point.map(|p| p.lon))
        .bind(&listing.provenance.url)
        .bind(listing.provenance.collected_at)
        .bind(listing.updated_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get(&self, id: Uuid) -> Result<Option<ListingRow>, sqlx::Error> {
        sqlx::query_as(&format!("{SELECT} WHERE id = $1"))
            .bind(id)
            .fetch_optional(&self.pool)
            .await
    }

    pub async fn search(&self, filter: &ListingFilter) -> Result<Vec<ListingRow>, sqlx::Error> {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(SELECT);
        qb.push(" WHERE TRUE");
        if let Some(code) = &filter.municipality_ibge_code {
            qb.push(" AND municipality_ibge_code = ")
                .push_bind(code.clone());
        }
        if let Some(slug) = &filter.neighborhood {
            qb.push(" AND neighborhood_slug = ").push_bind(slug.clone());
        }
        if let Some(t) = &filter.transaction {
            qb.push(" AND transaction = ").push_bind(t.clone());
        }
        if let Some(n) = filter.min_bedrooms {
            qb.push(" AND bedrooms >= ").push_bind(n);
        }
        if let Some(p) = filter.max_price {
            qb.push(" AND price_brl <= ").push_bind(p);
        }
        qb.push(" ORDER BY updated_at DESC LIMIT ")
            .push_bind(filter.limit.unwrap_or(50).clamp(1, 500));
        qb.build_query_as().fetch_all(&self.pool).await
    }

    /// Anúncios comparáveis: mesmo bairro, tipo e transação.
    pub async fn comparables(&self, row: &ListingRow) -> Result<Vec<ListingRow>, sqlx::Error> {
        sqlx::query_as(&format!(
            "{SELECT} WHERE id <> $1 AND municipality_ibge_code IS NOT DISTINCT FROM $2 \
             AND neighborhood_slug IS NOT DISTINCT FROM $3 AND kind = $4 AND transaction = $5"
        ))
        .bind(row.id)
        .bind(&row.municipality_ibge_code)
        .bind(&row.neighborhood_slug)
        .bind(&row.kind)
        .bind(&row.transaction)
        .fetch_all(&self.pool)
        .await
    }
}

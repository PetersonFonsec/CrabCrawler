use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{FromRow, PgPool, Postgres, QueryBuilder};
use uuid::Uuid;

/// Anúncio + dados físicos do imóvel, como exposto para leitura (API).
///
/// Os campos físicos (tipo, área, localização) vêm de `properties`; os do
/// anúncio (preço, finalidade, fonte) de `listings`. O formato é o mesmo de
/// antes da separação, então Segurança e Regional Intelligence não mudam.
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct ListingRow {
    pub id: Uuid,
    pub property_id: Uuid,
    pub source: String,
    pub partner_id: Option<Uuid>,
    pub external_id: String,
    pub title: String,
    pub transaction: String,
    pub kind: String,
    pub status: String,
    pub price_brl: Option<f64>,
    pub condominium_fee_brl: Option<f64>,
    pub property_tax_brl: Option<f64>,
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
    pub coordinate_source: Option<String>,
    pub coordinate_precision: Option<String>,
    pub source_url: Option<String>,
    pub collected_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct ListingFilter {
    pub municipality_ibge_code: Option<String>,
    pub neighborhood: Option<String>,
    pub transaction: Option<String>,
    pub source: Option<String>,
    pub status: Option<String>,
    pub min_bedrooms: Option<i32>,
    pub max_price: Option<f64>,
    pub limit: Option<i64>,
}

const SELECT: &str = "SELECT l.id, l.property_id, l.source, l.partner_id, l.external_id, \
    l.title, l.transaction, p.property_type AS kind, l.status, l.price_brl, \
    l.condominium_fee_brl, l.property_tax_brl, p.area_m2, p.bedrooms, p.bathrooms, \
    p.parking_spaces AS parking_spots, p.state, p.municipality, p.municipality_ibge_code, \
    p.neighborhood, p.neighborhood_slug, p.postal_code, \
    ST_Y(p.geom::geometry) AS lat, ST_X(p.geom::geometry) AS lon, \
    p.coordinate_source, p.coordinate_precision, \
    l.source_url, l.collected_at, l.updated_at \
    FROM listings l JOIN properties p ON p.id = l.property_id";

/// Leitura de anúncios. A escrita passa por `PropertyRepository`, que
/// mantém imóvel, anúncio e histórico de preço consistentes.
#[derive(Clone)]
pub struct ListingRepository {
    pool: PgPool,
}

impl ListingRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn get(&self, id: Uuid) -> Result<Option<ListingRow>, sqlx::Error> {
        sqlx::query_as(&format!("{SELECT} WHERE l.id = $1"))
            .bind(id)
            .fetch_optional(&self.pool)
            .await
    }

    /// Anúncio principal de um imóvel: o ativo mais recente, senão o mais
    /// recente de qualquer status.
    pub async fn primary_for_property(
        &self,
        property_id: Uuid,
    ) -> Result<Option<ListingRow>, sqlx::Error> {
        sqlx::query_as(&format!(
            "{SELECT} WHERE l.property_id = $1 \
             ORDER BY (l.status = 'ACTIVE') DESC, l.updated_at DESC LIMIT 1"
        ))
        .bind(property_id)
        .fetch_optional(&self.pool)
        .await
    }

    pub async fn search(&self, filter: &ListingFilter) -> Result<Vec<ListingRow>, sqlx::Error> {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(SELECT);
        qb.push(" WHERE TRUE");
        if let Some(code) = &filter.municipality_ibge_code {
            qb.push(" AND p.municipality_ibge_code = ")
                .push_bind(code.clone());
        }
        if let Some(slug) = &filter.neighborhood {
            qb.push(" AND p.neighborhood_slug = ")
                .push_bind(slug.clone());
        }
        if let Some(t) = &filter.transaction {
            qb.push(" AND l.transaction = ").push_bind(t.clone());
        }
        if let Some(s) = &filter.source {
            qb.push(" AND l.source = ").push_bind(s.to_uppercase());
        }
        if let Some(s) = &filter.status {
            qb.push(" AND l.status = ").push_bind(s.to_uppercase());
        }
        if let Some(n) = filter.min_bedrooms {
            qb.push(" AND p.bedrooms >= ").push_bind(n);
        }
        if let Some(p) = filter.max_price {
            qb.push(" AND l.price_brl <= ").push_bind(p);
        }
        qb.push(" ORDER BY l.updated_at DESC LIMIT ")
            .push_bind(filter.limit.unwrap_or(50).clamp(1, 500));
        qb.build_query_as().fetch_all(&self.pool).await
    }

    /// Anúncios comparáveis: ativos, mesmo bairro, tipo e finalidade, de
    /// qualquer fonte, sem contar outros anúncios do mesmo imóvel.
    pub async fn comparables(&self, row: &ListingRow) -> Result<Vec<ListingRow>, sqlx::Error> {
        sqlx::query_as(&format!(
            "{SELECT} WHERE l.id <> $1 AND l.property_id <> $6 AND l.status = 'ACTIVE' \
             AND p.municipality_ibge_code IS NOT DISTINCT FROM $2 \
             AND p.neighborhood_slug IS NOT DISTINCT FROM $3 \
             AND p.property_type = $4 AND l.transaction = $5"
        ))
        .bind(row.id)
        .bind(&row.municipality_ibge_code)
        .bind(&row.neighborhood_slug)
        .bind(&row.kind)
        .bind(&row.transaction)
        .bind(row.property_id)
        .fetch_all(&self.pool)
        .await
    }
}

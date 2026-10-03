//! Persistência do Regional Intelligence (setores, indicadores, serviços e
//! áreas de risco) e as consultas geoespaciais usadas pela API.
//!
//! Gravação: cada dataset é gravado numa transação. Reimportar a mesma
//! versão atualiza as linhas pelas chaves naturais; uma versão nova
//! substitui as linhas daquele dataset nos municípios cobertos e remove as
//! que sumiram da fonte. Uma geometria que o PostGIS rejeita descarta só
//! aquele registro (savepoint) e entra no relatório.

use chrono::{DateTime, NaiveDate, Utc};
use crab_domain::regional::{
    Capability, CensusSector, DatasetBatch, DatasetProvenance, EnvironmentalRiskArea, ImportReport,
    RawGeometry, SectorIndicatorValue, UrbanService,
};
use serde::Serialize;
use sqlx::{FromRow, PgPool, Postgres, Transaction};

/// Geometria vinda da fonte → EPSG:4326. `$a` WKB, `$b` GeoJSON, `$c` SRID.
macro_rules! source_geom {
    ($a:literal, $b:literal, $c:literal) => {
        concat!(
            "ST_Transform(ST_SetSRID(COALESCE(ST_GeomFromWKB($",
            $a,
            "), ST_GeomFromGeoJSON($",
            $b,
            ")), $",
            $c,
            "), 4326)"
        )
    };
}

/// Resultado da gravação de um dataset.
#[derive(Debug, Clone, Default, Serialize)]
pub struct StoreOutcome {
    pub dataset_id: i64,
    pub stored: usize,
    pub removed: usize,
    /// Registros recusados pelo banco (geometria inválida, por exemplo).
    pub rejected: usize,
    pub issues: Vec<String>,
}

/// Procedência de um dado lido do banco.
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct DatasetRef {
    pub source: String,
    pub dataset_name: String,
    pub dataset_version: String,
    pub source_url: Option<String>,
    pub reference_date: Option<NaiveDate>,
    pub collected_at: DateTime<Utc>,
    pub geographic_granularity: String,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct DatasetCoverageRow {
    pub source: String,
    pub dataset_name: String,
    pub dataset_version: String,
    pub categories: Vec<String>,
    pub source_url: Option<String>,
    pub reference_date: Option<NaiveDate>,
    pub collected_at: DateTime<Utc>,
    pub geographic_granularity: String,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct SectorRow {
    pub code: String,
    pub municipality_ibge_code: String,
    pub municipality_name: Option<String>,
    pub area_km2: Option<f64>,
    #[sqlx(flatten)]
    pub dataset: DatasetRef,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct SectorIndicatorRow {
    pub indicator: String,
    pub value: Option<f64>,
    pub value_text: Option<String>,
    pub source_variable: String,
    #[sqlx(flatten)]
    pub dataset: DatasetRef,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct ServiceRow {
    pub external_id: String,
    pub name: Option<String>,
    pub category: String,
    pub subcategory: Option<String>,
    pub address: Option<String>,
    pub municipality_ibge_code: String,
    pub lat: f64,
    pub lon: f64,
    pub distance_m: f64,
    #[sqlx(flatten)]
    pub dataset: DatasetRef,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct RiskAreaRow {
    pub external_id: String,
    pub risk_types: Vec<String>,
    pub source_labels: Vec<String>,
    pub severity: Option<String>,
    pub location_name: Option<String>,
    pub mapped_on: Option<NaiveDate>,
    pub municipality_ibge_code: String,
    pub inside: bool,
    pub distance_m: f64,
    #[sqlx(flatten)]
    pub dataset: DatasetRef,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct RegionalImportRow {
    pub source: String,
    pub dataset_name: String,
    pub dataset_version: Option<String>,
    pub scope: String,
    pub status: String,
    pub records_read: i32,
    pub records_stored: i32,
    pub records_skipped: i32,
    pub records_removed: i32,
    pub error: Option<String>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

const DATASET_COLUMNS: &str = "d.source, d.dataset_name, d.dataset_version, d.source_url, \
    d.reference_date, d.collected_at, d.geographic_granularity";

#[derive(Clone)]
pub struct RegionalRepository {
    pool: PgPool,
}

impl RegionalRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    // ---------------------------------------------------------------- import log

    /// Abre o registro de uma importação (status `running`).
    pub async fn start_import(
        &self,
        source: &str,
        dataset_name: &str,
        scope: &str,
    ) -> Result<i64, sqlx::Error> {
        let (id,): (i64,) = sqlx::query_as(
            "INSERT INTO regional_imports (source, dataset_name, scope, status) \
             VALUES ($1, $2, $3, 'running') RETURNING id",
        )
        .bind(source)
        .bind(dataset_name)
        .bind(scope)
        .fetch_one(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn finish_import_ok(
        &self,
        import_id: i64,
        provenance: &DatasetProvenance,
        report: &ImportReport,
        outcome: &StoreOutcome,
    ) -> Result<(), sqlx::Error> {
        let to_i32 = |n: usize| i32::try_from(n).unwrap_or(i32::MAX);
        let mut issues = report.issues.clone();
        issues.extend(outcome.issues.iter().cloned());
        sqlx::query(
            "UPDATE regional_imports SET status = 'succeeded', dataset_version = $2, \
                dataset_id = $3, records_read = $4, records_stored = $5, records_skipped = $6, \
                records_removed = $7, report = $8, finished_at = now() WHERE id = $1",
        )
        .bind(import_id)
        .bind(&provenance.dataset_version)
        .bind(outcome.dataset_id)
        .bind(to_i32(report.read))
        .bind(to_i32(outcome.stored))
        .bind(to_i32(report.skipped + outcome.rejected))
        .bind(to_i32(outcome.removed))
        .bind(serde_json::json!({ "issues": issues }))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn finish_import_failed(
        &self,
        import_id: i64,
        error: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE regional_imports SET status = 'failed', error = $2, finished_at = now() \
             WHERE id = $1",
        )
        .bind(import_id)
        .bind(error)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // ------------------------------------------------------------------ writes

    pub async fn store_sectors(
        &self,
        batch: &DatasetBatch<CensusSector>,
    ) -> Result<StoreOutcome, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let mut out = StoreOutcome {
            dataset_id: upsert_dataset(&mut tx, batch, Capability::CensusSectors, &[]).await?,
            ..Default::default()
        };
        for s in &batch.records {
            let (wkb, json, srid) = geometry_binds(&s.geometry);
            let sql = concat!(
                "INSERT INTO census_sectors (code, municipality_ibge_code, municipality_name, \
                    area_km2, geom, dataset_id) VALUES ($1, $2, $3, $4, \
                    ST_Multi(ST_CollectionExtract(ST_MakeValid(",
                source_geom!("5", "6", "7"),
                "), 3)), $8) \
                 ON CONFLICT (code) DO UPDATE SET \
                    municipality_ibge_code = EXCLUDED.municipality_ibge_code, \
                    municipality_name = EXCLUDED.municipality_name, area_km2 = EXCLUDED.area_km2, \
                    geom = EXCLUDED.geom, dataset_id = EXCLUDED.dataset_id"
            );
            let query = sqlx::query(sql)
                .bind(&s.code)
                .bind(&s.municipality_ibge_code)
                .bind(&s.municipality_name)
                .bind(s.area_km2)
                .bind(wkb)
                .bind(json)
                .bind(srid)
                .bind(out.dataset_id);
            guarded(&mut tx, &mut out, &s.code, query).await?;
        }
        out.removed = sqlx::query(
            "DELETE FROM census_sectors WHERE municipality_ibge_code = ANY($1) AND dataset_id <> $2",
        )
        .bind(&batch.coverage)
        .bind(out.dataset_id)
        .execute(&mut *tx)
        .await?
        .rows_affected() as usize;
        tx.commit().await?;
        Ok(out)
    }

    pub async fn store_sector_indicators(
        &self,
        batch: &DatasetBatch<SectorIndicatorValue>,
        capability: Capability,
    ) -> Result<StoreOutcome, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let mut categories: Vec<String> = batch
            .records
            .iter()
            .map(|r| r.indicator.as_str().to_string())
            .collect();
        categories.sort();
        categories.dedup();
        let p = &batch.provenance;
        let mut out = StoreOutcome {
            dataset_id: upsert_dataset(&mut tx, batch, capability, &categories).await?,
            ..Default::default()
        };
        for r in &batch.records {
            sqlx::query(
                "INSERT INTO census_sector_indicators (sector_code, indicator, value, value_text, \
                    source_variable, source, dataset_name, dataset_id) \
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8) \
                 ON CONFLICT (sector_code, indicator, source, dataset_name) DO UPDATE SET \
                    value = EXCLUDED.value, value_text = EXCLUDED.value_text, \
                    source_variable = EXCLUDED.source_variable, dataset_id = EXCLUDED.dataset_id",
            )
            .bind(&r.sector_code)
            .bind(r.indicator.as_str())
            .bind(r.value)
            .bind(&r.value_text)
            .bind(&r.source_variable)
            .bind(p.source.as_str())
            .bind(&p.dataset_name)
            .bind(out.dataset_id)
            .execute(&mut *tx)
            .await?;
            out.stored += 1;
        }
        out.removed = sqlx::query(
            "DELETE FROM census_sector_indicators WHERE source = $1 AND dataset_name = $2 \
             AND left(sector_code, 7) = ANY($3) AND dataset_id <> $4",
        )
        .bind(p.source.as_str())
        .bind(&p.dataset_name)
        .bind(&batch.coverage)
        .bind(out.dataset_id)
        .execute(&mut *tx)
        .await?
        .rows_affected() as usize;
        tx.commit().await?;
        Ok(out)
    }

    pub async fn store_services(
        &self,
        batch: &DatasetBatch<UrbanService>,
    ) -> Result<StoreOutcome, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let mut categories: Vec<String> = batch
            .records
            .iter()
            .map(|r| r.category.as_str().to_string())
            .collect();
        categories.sort();
        categories.dedup();
        let p = &batch.provenance;
        let mut out = StoreOutcome {
            dataset_id: upsert_dataset(&mut tx, batch, Capability::UrbanServices, &categories)
                .await?,
            ..Default::default()
        };
        for s in &batch.records {
            let (wkb, json, srid) = geometry_binds(&s.geometry);
            let sql = concat!(
                "INSERT INTO urban_services (source, dataset_name, external_id, name, category, \
                    subcategory, address, municipality_ibge_code, geom, attributes, dataset_id) \
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8, ST_PointOnSurface(",
                source_geom!("9", "10", "11"),
                ")::geography, $12, $13) \
                 ON CONFLICT (source, dataset_name, external_id) DO UPDATE SET \
                    name = EXCLUDED.name, category = EXCLUDED.category, \
                    subcategory = EXCLUDED.subcategory, address = EXCLUDED.address, \
                    municipality_ibge_code = EXCLUDED.municipality_ibge_code, geom = EXCLUDED.geom, \
                    attributes = EXCLUDED.attributes, dataset_id = EXCLUDED.dataset_id"
            );
            let query = sqlx::query(sql)
                .bind(p.source.as_str())
                .bind(&p.dataset_name)
                .bind(&s.external_id)
                .bind(&s.name)
                .bind(s.category.as_str())
                .bind(&s.subcategory)
                .bind(&s.address)
                .bind(&s.municipality_ibge_code)
                .bind(wkb)
                .bind(json)
                .bind(srid)
                .bind(&s.attributes)
                .bind(out.dataset_id);
            guarded(&mut tx, &mut out, &s.external_id, query).await?;
        }
        out.removed = sqlx::query(
            "DELETE FROM urban_services WHERE source = $1 AND dataset_name = $2 \
             AND municipality_ibge_code = ANY($3) AND dataset_id <> $4",
        )
        .bind(p.source.as_str())
        .bind(&p.dataset_name)
        .bind(&batch.coverage)
        .bind(out.dataset_id)
        .execute(&mut *tx)
        .await?
        .rows_affected() as usize;
        tx.commit().await?;
        Ok(out)
    }

    pub async fn store_risk_areas(
        &self,
        batch: &DatasetBatch<EnvironmentalRiskArea>,
    ) -> Result<StoreOutcome, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let mut categories: Vec<String> = batch
            .records
            .iter()
            .flat_map(|r| r.risk_types.iter().map(|t| t.as_str().to_string()))
            .collect();
        categories.sort();
        categories.dedup();
        let p = &batch.provenance;
        let mut out = StoreOutcome {
            dataset_id: upsert_dataset(&mut tx, batch, Capability::EnvironmentalRisk, &categories)
                .await?,
            ..Default::default()
        };
        for r in &batch.records {
            let (wkb, json, srid) = geometry_binds(&r.geometry);
            let types: Vec<&str> = r.risk_types.iter().map(|t| t.as_str()).collect();
            let sql = concat!(
                "INSERT INTO environmental_risk_areas (source, dataset_name, external_id, \
                    risk_types, source_labels, severity, location_name, mapped_on, \
                    municipality_ibge_code, geom, attributes, dataset_id) \
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9, ST_MakeValid(",
                source_geom!("10", "11", "12"),
                "), $13, $14) \
                 ON CONFLICT (source, dataset_name, external_id) DO UPDATE SET \
                    risk_types = EXCLUDED.risk_types, source_labels = EXCLUDED.source_labels, \
                    severity = EXCLUDED.severity, location_name = EXCLUDED.location_name, \
                    mapped_on = EXCLUDED.mapped_on, \
                    municipality_ibge_code = EXCLUDED.municipality_ibge_code, \
                    geom = EXCLUDED.geom, attributes = EXCLUDED.attributes, \
                    dataset_id = EXCLUDED.dataset_id"
            );
            let query = sqlx::query(sql)
                .bind(p.source.as_str())
                .bind(&p.dataset_name)
                .bind(&r.external_id)
                .bind(types)
                .bind(&r.source_labels)
                .bind(&r.severity)
                .bind(&r.location_name)
                .bind(r.mapped_on)
                .bind(&r.municipality_ibge_code)
                .bind(wkb)
                .bind(json)
                .bind(srid)
                .bind(&r.attributes)
                .bind(out.dataset_id);
            guarded(&mut tx, &mut out, &r.external_id, query).await?;
        }
        out.removed = sqlx::query(
            "DELETE FROM environmental_risk_areas WHERE source = $1 AND dataset_name = $2 \
             AND municipality_ibge_code = ANY($3) AND dataset_id <> $4",
        )
        .bind(p.source.as_str())
        .bind(&p.dataset_name)
        .bind(&batch.coverage)
        .bind(out.dataset_id)
        .execute(&mut *tx)
        .await?
        .rows_affected() as usize;
        tx.commit().await?;
        Ok(out)
    }

    // ------------------------------------------------------------------- reads

    /// Versão mais recente de cada dataset que cobre o município para uma
    /// capacidade. Vazio = nada importado para esse município.
    pub async fn coverage(
        &self,
        capability: Capability,
        municipality_ibge_code: &str,
    ) -> Result<Vec<DatasetCoverageRow>, sqlx::Error> {
        sqlx::query_as(
            "SELECT * FROM ( \
                SELECT DISTINCT ON (d.source, d.dataset_name) d.source, d.dataset_name, \
                    d.dataset_version, d.categories, d.source_url, d.reference_date, \
                    d.collected_at, d.geographic_granularity \
                FROM regional_datasets d \
                JOIN regional_dataset_coverage c ON c.dataset_id = d.id \
                WHERE d.capability = $1 AND c.municipality_ibge_code = $2 \
                ORDER BY d.source, d.dataset_name, d.collected_at DESC \
             ) latest ORDER BY source, dataset_name",
        )
        .bind(capability.as_str())
        .bind(municipality_ibge_code)
        .fetch_all(&self.pool)
        .await
    }

    /// Setor censitário que contém o ponto (`ST_Covers`: ponto na borda
    /// conta; com dois candidatos, o menor código vence).
    pub async fn sector_at(&self, lat: f64, lon: f64) -> Result<Option<SectorRow>, sqlx::Error> {
        sqlx::query_as(&format!(
            "SELECT s.code, s.municipality_ibge_code, s.municipality_name, s.area_km2, \
                {DATASET_COLUMNS} \
             FROM census_sectors s JOIN regional_datasets d ON d.id = s.dataset_id \
             WHERE ST_Covers(s.geom, ST_SetSRID(ST_MakePoint($2, $1), 4326)) \
             ORDER BY s.code LIMIT 1"
        ))
        .bind(lat)
        .bind(lon)
        .fetch_optional(&self.pool)
        .await
    }

    pub async fn sector_indicators(
        &self,
        sector_code: &str,
    ) -> Result<Vec<SectorIndicatorRow>, sqlx::Error> {
        sqlx::query_as(&format!(
            "SELECT i.indicator, i.value, i.value_text, i.source_variable, {DATASET_COLUMNS} \
             FROM census_sector_indicators i JOIN regional_datasets d ON d.id = i.dataset_id \
             WHERE i.sector_code = $1 ORDER BY i.indicator, d.source"
        ))
        .bind(sector_code)
        .fetch_all(&self.pool)
        .await
    }

    /// Serviços num raio (metros, geodésico), do mais perto ao mais longe,
    /// no máximo `limit_per_category` por categoria.
    pub async fn services_within(
        &self,
        lat: f64,
        lon: f64,
        radius_m: f64,
        categories: &[String],
        limit_per_category: i64,
    ) -> Result<Vec<ServiceRow>, sqlx::Error> {
        sqlx::query_as(&format!(
            "SELECT * FROM ( \
                SELECT u.external_id, u.name, u.category, u.subcategory, u.address, \
                    u.municipality_ibge_code, ST_Y(u.geom::geometry) AS lat, \
                    ST_X(u.geom::geometry) AS lon, ST_Distance(u.geom, p.g) AS distance_m, \
                    {DATASET_COLUMNS}, row_number() OVER ( \
                        PARTITION BY u.category ORDER BY ST_Distance(u.geom, p.g), u.external_id \
                    ) AS rank_in_category \
                FROM urban_services u JOIN regional_datasets d ON d.id = u.dataset_id, \
                    (SELECT ST_SetSRID(ST_MakePoint($2, $1), 4326)::geography AS g) p \
                WHERE ST_DWithin(u.geom, p.g, $3) AND u.category = ANY($4) \
             ) ranked WHERE rank_in_category <= $5 \
             ORDER BY distance_m, external_id"
        ))
        .bind(lat)
        .bind(lon)
        .bind(radius_m)
        .bind(categories)
        .bind(limit_per_category)
        .fetch_all(&self.pool)
        .await
    }

    /// O equipamento mais próximo de cada categoria, sem limite de raio.
    pub async fn nearest_service_per_category(
        &self,
        lat: f64,
        lon: f64,
        categories: &[String],
    ) -> Result<Vec<ServiceRow>, sqlx::Error> {
        sqlx::query_as(&format!(
            "SELECT n.* FROM unnest($3::text[]) AS cat(category), \
                (SELECT ST_SetSRID(ST_MakePoint($2, $1), 4326)::geography AS g) p, \
             LATERAL ( \
                SELECT u.external_id, u.name, u.category, u.subcategory, u.address, \
                    u.municipality_ibge_code, ST_Y(u.geom::geometry) AS lat, \
                    ST_X(u.geom::geometry) AS lon, ST_Distance(u.geom, p.g) AS distance_m, \
                    {DATASET_COLUMNS} \
                FROM urban_services u JOIN regional_datasets d ON d.id = u.dataset_id \
                WHERE u.category = cat.category \
                ORDER BY u.geom <-> p.g LIMIT 1 \
             ) n ORDER BY n.distance_m"
        ))
        .bind(lat)
        .bind(lon)
        .bind(categories)
        .fetch_all(&self.pool)
        .await
    }

    /// Áreas de risco que contêm o ponto ou estão a até `max_distance_m`.
    pub async fn risk_areas_near(
        &self,
        lat: f64,
        lon: f64,
        max_distance_m: f64,
    ) -> Result<Vec<RiskAreaRow>, sqlx::Error> {
        sqlx::query_as(&format!(
            "SELECT r.external_id, r.risk_types, r.source_labels, r.severity, r.location_name, \
                r.mapped_on, r.municipality_ibge_code, ST_Intersects(r.geom, p.g) AS inside, \
                ST_Distance(r.geom::geography, p.g::geography) AS distance_m, {DATASET_COLUMNS} \
             FROM environmental_risk_areas r JOIN regional_datasets d ON d.id = r.dataset_id, \
                (SELECT ST_SetSRID(ST_MakePoint($2, $1), 4326) AS g) p \
             WHERE ST_DWithin(r.geom::geography, p.g::geography, $3) \
             ORDER BY distance_m, r.external_id"
        ))
        .bind(lat)
        .bind(lon)
        .bind(max_distance_m)
        .fetch_all(&self.pool)
        .await
    }

    /// Última importação de cada dataset (inclusive falhas).
    pub async fn latest_imports(&self) -> Result<Vec<RegionalImportRow>, sqlx::Error> {
        sqlx::query_as(
            "SELECT DISTINCT ON (source, dataset_name) source, dataset_name, dataset_version, \
                scope, status, records_read, records_stored, records_skipped, records_removed, \
                error, started_at, finished_at \
             FROM regional_imports ORDER BY source, dataset_name, started_at DESC, id DESC",
        )
        .fetch_all(&self.pool)
        .await
    }

    /// Última importação bem-sucedida por fonte (para saber se está vencida).
    pub async fn last_success_by_source(
        &self,
    ) -> Result<Vec<(String, DateTime<Utc>)>, sqlx::Error> {
        sqlx::query_as(
            "SELECT source, MAX(finished_at) FROM regional_imports \
             WHERE status = 'succeeded' GROUP BY source",
        )
        .fetch_all(&self.pool)
        .await
    }
}

fn geometry_binds(geometry: &RawGeometry) -> (Option<&[u8]>, Option<&str>, i32) {
    match geometry {
        RawGeometry::Wkb { bytes, srid } => (Some(bytes.as_slice()), None, *srid),
        RawGeometry::GeoJson { json, srid } => (None, Some(json.as_str()), *srid),
    }
}

/// Insere/encontra o dataset e grava a cobertura.
async fn upsert_dataset<T>(
    tx: &mut Transaction<'_, Postgres>,
    batch: &DatasetBatch<T>,
    capability: Capability,
    categories: &[String],
) -> Result<i64, sqlx::Error> {
    let p = &batch.provenance;
    let (id,): (i64,) = sqlx::query_as(
        "INSERT INTO regional_datasets (source, dataset_name, dataset_version, capability, \
            categories, source_url, reference_date, geographic_granularity, collected_at) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) \
         ON CONFLICT (source, dataset_name, dataset_version) DO UPDATE SET \
            capability = EXCLUDED.capability, categories = EXCLUDED.categories, \
            source_url = EXCLUDED.source_url, reference_date = EXCLUDED.reference_date, \
            geographic_granularity = EXCLUDED.geographic_granularity, \
            collected_at = EXCLUDED.collected_at \
         RETURNING id",
    )
    .bind(p.source.as_str())
    .bind(&p.dataset_name)
    .bind(&p.dataset_version)
    .bind(capability.as_str())
    .bind(categories)
    .bind(&p.source_url)
    .bind(p.reference_date)
    .bind(p.granularity.as_str())
    .bind(p.collected_at)
    .fetch_one(&mut **tx)
    .await?;
    sqlx::query(
        "INSERT INTO regional_dataset_coverage (dataset_id, municipality_ibge_code) \
         SELECT $1, unnest($2::text[]) ON CONFLICT DO NOTHING",
    )
    .bind(id)
    .bind(&batch.coverage)
    .execute(&mut **tx)
    .await?;
    Ok(id)
}

/// Executa o insert de um registro dentro de um savepoint: se o PostGIS
/// recusar a geometria, só aquele registro é descartado.
async fn guarded(
    tx: &mut Transaction<'_, Postgres>,
    out: &mut StoreOutcome,
    id: &str,
    query: sqlx::query::Query<'_, Postgres, sqlx::postgres::PgArguments>,
) -> Result<(), sqlx::Error> {
    sqlx::query("SAVEPOINT regional_record")
        .execute(&mut **tx)
        .await?;
    match query.execute(&mut **tx).await {
        Ok(_) => {
            out.stored += 1;
            sqlx::query("RELEASE SAVEPOINT regional_record")
                .execute(&mut **tx)
                .await?;
        }
        Err(sqlx::Error::Database(err)) => {
            sqlx::query("ROLLBACK TO SAVEPOINT regional_record")
                .execute(&mut **tx)
                .await?;
            out.rejected += 1;
            if out.issues.len() < ImportReport::MAX_ISSUES {
                out.issues
                    .push(format!("{id}: recusado pelo banco: {}", err.message()));
            }
            tracing::warn!(id, error = %err.message(), "registro regional recusado");
        }
        Err(other) => return Err(other),
    }
    Ok(())
}

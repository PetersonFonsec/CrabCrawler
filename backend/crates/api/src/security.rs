//! Endpoints de segurança pública.
//!
//! Só apresentam indicadores oficiais e comparações. Não há score nem
//! classificação "seguro/perigoso": a interface recebe os números, a
//! população usada, o período, a granularidade e as limitações de cada fonte.

use std::collections::BTreeSet;

use axum::extract::{Path, Query, State};
use axum::Json;
use chrono::{DateTime, Datelike, Utc};
use crab_domain::{CountingUnit, CrimeType, DataSource, GeoPoint, RegionLevel, SourceMetadata};
use crab_persistence::{ListingRow, PopulationRow, SecurityRegionRow};
use crab_processing::security::{
    annual_indicators, default_year, trend, AnnualIndicator, PopulationRef, Trend,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::ApiError;
use crate::AppState;

/// Avisos gerais que acompanham toda resposta de segurança.
const GENERAL_NOTES: [&str; 4] = [
    "Os números são ocorrências registradas pela polícia; não representam toda a criminalidade.",
    "Taxas por 100 mil habitantes só são calculadas no nível do município, onde há população oficial compatível.",
    "Ocorrências e vítimas são unidades diferentes e não devem ser somadas.",
    "Fontes diferentes (SSP-SP e Sinesp) seguem metodologias próprias e não são somadas.",
];

const METHODOLOGY: &str = "Contagens mensais oficiais somadas por ano civil. \
    taxa = ocorrências / população × 100.000, usando a população do IBGE do ano \
    mais próximo disponível (o ano usado é informado). Anos incompletos são \
    marcados com complete = false e não entram no cálculo de tendência.";

/// Trend é calculado sobre os últimos N anos completos.
const TREND_WINDOW: usize = 3;

#[derive(Debug, Serialize)]
pub struct RegionRef {
    pub level: RegionLevel,
    pub code: String,
    pub name: Option<String>,
    pub municipality_ibge_code: String,
}

#[derive(Debug, Serialize)]
pub struct Transparency {
    pub sources: Vec<SourceMetadata>,
    pub last_updated: Option<DateTime<Utc>>,
    pub methodology: &'static str,
    pub notes: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct SecurityReport {
    pub region: RegionRef,
    /// Granularidade real dos números (nunca "rebaixada" para bairro).
    pub geographic_scope: RegionLevel,
    /// Ano apresentado; `None` quando não há dados.
    pub year: Option<i32>,
    pub available_years: Vec<i32>,
    pub indicators: Vec<AnnualIndicator>,
    /// Recortes mais finos disponíveis (ex.: áreas de delegacia), só no
    /// relatório municipal.
    pub subregions: Vec<SecurityRegionRow>,
    pub transparency: Transparency,
}

#[derive(Debug, Deserialize)]
pub struct RegionQuery {
    pub year: Option<i32>,
    /// `municipality` (padrão) ou `police_district`.
    pub level: Option<String>,
    /// Código da sub-região (obrigatório quando `level` não é município).
    pub code: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    pub crime_type: Option<String>,
    pub level: Option<String>,
    pub code: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CompareQuery {
    /// Códigos IBGE separados por vírgula.
    pub municipalities: Option<String>,
    /// IDs de anúncios separados por vírgula.
    pub listings: Option<String>,
    pub year: Option<i32>,
}

/// `GET /regions/{ibge_code}/security`
pub async fn region_security(
    State(state): State<AppState>,
    Path(ibge_code): Path<String>,
    Query(q): Query<RegionQuery>,
) -> Result<Json<SecurityReport>, ApiError> {
    let target = Target::parse(&ibge_code, q.level.as_deref(), q.code.as_deref())?;
    Ok(Json(build_report(&state, &target, q.year).await?))
}

#[derive(Debug, Serialize)]
pub struct Series {
    pub source: DataSource,
    pub crime_type: CrimeType,
    pub counting_unit: CountingUnit,
    pub points: Vec<AnnualIndicator>,
    pub trend: Option<Trend>,
}

#[derive(Debug, Serialize)]
pub struct SecurityHistory {
    pub region: RegionRef,
    pub geographic_scope: RegionLevel,
    pub series: Vec<Series>,
    pub transparency: Transparency,
}

/// `GET /regions/{ibge_code}/security/history`
pub async fn region_security_history(
    State(state): State<AppState>,
    Path(ibge_code): Path<String>,
    Query(q): Query<HistoryQuery>,
) -> Result<Json<SecurityHistory>, ApiError> {
    let target = Target::parse(&ibge_code, q.level.as_deref(), q.code.as_deref())?;
    let crime_type = q
        .crime_type
        .as_deref()
        .map(|t| {
            CrimeType::parse(t).ok_or_else(|| ApiError::BadRequest(format!("crime_type `{t}`")))
        })
        .transpose()?;
    let loaded = load(&state, &target, crime_type).await?;

    let mut series: Vec<Series> = Vec::new();
    for ind in loaded.indicators {
        match series.iter_mut().find(|s| {
            s.source == ind.source
                && s.crime_type == ind.crime_type
                && s.counting_unit == ind.counting_unit
        }) {
            Some(s) => s.points.push(ind),
            None => series.push(Series {
                source: ind.source,
                crime_type: ind.crime_type,
                counting_unit: ind.counting_unit,
                points: vec![ind],
                trend: None,
            }),
        }
    }
    for s in &mut series {
        s.points.sort_by_key(|p| p.year);
        s.trend = trend(&s.points, TREND_WINDOW);
    }

    let all: Vec<&AnnualIndicator> = series.iter().flat_map(|s| s.points.iter()).collect();
    let transparency = transparency(&target, &all, &loaded.population);
    Ok(Json(SecurityHistory {
        region: target.region_ref(loaded.region_name),
        geographic_scope: target.level,
        series,
        transparency,
    }))
}

#[derive(Debug, Serialize)]
pub struct ListingSecurity {
    pub listing_id: Uuid,
    pub location: ListingLocation,
    /// `None` quando o município do anúncio não foi resolvido.
    pub security: Option<SecurityReport>,
    pub notes: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ListingLocation {
    pub state: Option<String>,
    pub municipality: Option<String>,
    pub municipality_ibge_code: Option<String>,
    pub neighborhood: Option<String>,
    pub postal_code: Option<String>,
    pub point: Option<GeoPoint>,
}

/// `GET /listings/{id}/security`
pub async fn listing_security(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<RegionQuery>,
) -> Result<Json<ListingSecurity>, ApiError> {
    let listing = state.listings.get(id).await?.ok_or(ApiError::NotFound)?;
    Ok(Json(listing_report(&state, listing, q.year).await?))
}

async fn listing_report(
    state: &AppState,
    listing: ListingRow,
    year: Option<i32>,
) -> Result<ListingSecurity, ApiError> {
    let mut notes = Vec::new();
    let security = match &listing.municipality_ibge_code {
        Some(code) => {
            let target = Target::municipality(code);
            Some(build_report(state, &target, year).await?)
        }
        None => {
            notes.push(
                "Município do anúncio não identificado; não é possível associar dados de segurança."
                    .to_string(),
            );
            None
        }
    };
    if listing.neighborhood.is_some() {
        notes.push(
            "Os dados oficiais disponíveis são municipais ou por delegacia; não há estatística \
             oficial por bairro, então os números não descrevem o bairro do imóvel."
                .to_string(),
        );
    }
    let point = match (listing.lat, listing.lon) {
        (Some(lat), Some(lon)) => GeoPoint::new(lat, lon),
        _ => None,
    };
    Ok(ListingSecurity {
        listing_id: listing.id,
        location: ListingLocation {
            state: listing.state,
            municipality: listing.municipality,
            municipality_ibge_code: listing.municipality_ibge_code,
            neighborhood: listing.neighborhood,
            postal_code: listing.postal_code,
            point,
        },
        security,
        notes,
    })
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum CompareItem {
    Region(SecurityReport),
    Listing(ListingSecurity),
}

#[derive(Debug, Serialize)]
pub struct SecurityComparison {
    pub items: Vec<CompareItem>,
    pub notes: Vec<&'static str>,
}

/// `GET /security/compare?municipalities=3548708,3547809` ou
/// `?listings=<uuid>,<uuid>`.
pub async fn compare(
    State(state): State<AppState>,
    Query(q): Query<CompareQuery>,
) -> Result<Json<SecurityComparison>, ApiError> {
    let split = |s: &str| -> Vec<String> {
        s.split(',')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(str::to_string)
            .collect()
    };
    let mut items = Vec::new();
    match (q.municipalities.as_deref(), q.listings.as_deref()) {
        (Some(m), None) => {
            for code in limited(split(m))? {
                let target = Target::parse(&code, None, None)?;
                items.push(CompareItem::Region(
                    build_report(&state, &target, q.year).await?,
                ));
            }
        }
        (None, Some(l)) => {
            for id in limited(split(l))? {
                let id: Uuid = id
                    .parse()
                    .map_err(|_| ApiError::BadRequest(format!("id de anúncio inválido `{id}`")))?;
                let listing = state.listings.get(id).await?.ok_or(ApiError::NotFound)?;
                items.push(CompareItem::Listing(
                    listing_report(&state, listing, q.year).await?,
                ));
            }
        }
        _ => {
            return Err(ApiError::BadRequest(
                "informe `municipalities` ou `listings` (não ambos)".into(),
            ))
        }
    }
    Ok(Json(SecurityComparison {
        items,
        notes: vec![
            "Compare taxas por 100 mil, não valores absolutos, entre regiões de populações diferentes.",
            "Só compare indicadores com a mesma fonte, unidade de contagem e período completo.",
        ],
    }))
}

fn limited(items: Vec<String>) -> Result<Vec<String>, ApiError> {
    match items.len() {
        2..=5 => Ok(items),
        _ => Err(ApiError::BadRequest("compare de 2 a 5 itens".into())),
    }
}

#[derive(Debug, Serialize)]
pub struct SecuritySources {
    pub sources: Vec<SourceMetadata>,
    pub latest_imports: Vec<crab_persistence::ImportRow>,
    pub methodology: &'static str,
    pub notes: [&'static str; 4],
}

/// `GET /security/sources`
pub async fn sources(State(state): State<AppState>) -> Result<Json<SecuritySources>, ApiError> {
    Ok(Json(SecuritySources {
        sources: [
            DataSource::SspSp,
            DataSource::SinespVde,
            DataSource::IbgeLocalidades,
        ]
        .iter()
        .map(DataSource::metadata)
        .collect(),
        latest_imports: state.security.latest_imports().await?,
        methodology: METHODOLOGY,
        notes: GENERAL_NOTES,
    }))
}

// ---------------------------------------------------------------------------

/// Região pedida: nível + código dentro de um município.
struct Target {
    municipality: String,
    level: RegionLevel,
    code: String,
}

impl Target {
    fn municipality(code: &str) -> Self {
        Self {
            municipality: code.to_string(),
            level: RegionLevel::Municipality,
            code: code.to_string(),
        }
    }

    fn parse(ibge_code: &str, level: Option<&str>, code: Option<&str>) -> Result<Self, ApiError> {
        if ibge_code.len() != 7 || !ibge_code.bytes().all(|b| b.is_ascii_digit()) {
            return Err(ApiError::BadRequest(format!(
                "código IBGE do município inválido `{ibge_code}`"
            )));
        }
        match level.unwrap_or("municipality") {
            "municipality" => Ok(Self::municipality(ibge_code)),
            "police_district" => {
                let code = code.filter(|c| !c.is_empty()).ok_or_else(|| {
                    ApiError::BadRequest("`code` é obrigatório para police_district".into())
                })?;
                Ok(Self {
                    municipality: ibge_code.to_string(),
                    level: RegionLevel::PoliceDistrict,
                    code: code.to_string(),
                })
            }
            other => Err(ApiError::BadRequest(format!(
                "level `{other}` não suportado"
            ))),
        }
    }

    fn region_ref(&self, name: Option<String>) -> RegionRef {
        RegionRef {
            level: self.level,
            code: self.code.clone(),
            name,
            municipality_ibge_code: self.municipality.clone(),
        }
    }
}

struct Loaded {
    region_name: Option<String>,
    indicators: Vec<AnnualIndicator>,
    population: Vec<PopulationRef>,
}

async fn load(
    state: &AppState,
    target: &Target,
    crime_type: Option<CrimeType>,
) -> Result<Loaded, ApiError> {
    let stats = state
        .security
        .statistics_for_region(&target.municipality, target.level, &target.code, crime_type)
        .await?;
    // População só é compatível com o município inteiro.
    let population = if target.level == RegionLevel::Municipality {
        state
            .security
            .municipal_population(&target.municipality)
            .await?
            .into_iter()
            .map(population_ref)
            .collect()
    } else {
        vec![]
    };
    let region_name = stats
        .iter()
        .map(|s| s.region.name.clone())
        .find(|n| *n != target.code);
    tracing::debug!(
        municipality = %target.municipality,
        level = target.level.as_str(),
        rows = stats.len(),
        "estatísticas de segurança carregadas"
    );
    Ok(Loaded {
        region_name,
        indicators: annual_indicators(&stats, &population),
        population,
    })
}

async fn build_report(
    state: &AppState,
    target: &Target,
    year: Option<i32>,
) -> Result<SecurityReport, ApiError> {
    let loaded = load(state, target, None).await?;
    let available_years: Vec<i32> = loaded
        .indicators
        .iter()
        .map(|i| i.year)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let year = year.or_else(|| default_year(&loaded.indicators));
    let indicators: Vec<AnnualIndicator> = loaded
        .indicators
        .into_iter()
        .filter(|i| Some(i.year) == year)
        .collect();
    let subregions = if target.level == RegionLevel::Municipality {
        state
            .security
            .regions_in_municipality(&target.municipality)
            .await?
            .into_iter()
            .filter(|r| r.level != RegionLevel::Municipality.as_str())
            .collect()
    } else {
        vec![]
    };
    let refs: Vec<&AnnualIndicator> = indicators.iter().collect();
    let transparency = transparency(target, &refs, &loaded.population);
    Ok(SecurityReport {
        region: target.region_ref(loaded.region_name),
        geographic_scope: target.level,
        year,
        available_years,
        indicators,
        subregions,
        transparency,
    })
}

fn transparency(
    target: &Target,
    indicators: &[&AnnualIndicator],
    population: &[PopulationRef],
) -> Transparency {
    let mut sources: Vec<DataSource> = indicators.iter().map(|i| i.source).collect();
    if indicators.iter().any(|i| i.population.is_some()) {
        sources.extend(population.iter().map(|p| p.source));
    }
    sources.sort_by_key(|s| s.as_str());
    sources.dedup();

    let mut notes: Vec<String> = GENERAL_NOTES.iter().map(|n| n.to_string()).collect();
    if target.level != RegionLevel::Municipality {
        notes.push(
            "Área de delegacia: não há malha nem população oficial para este recorte, por isso não há taxa."
                .into(),
        );
    } else if population.is_empty() {
        notes.push("População do município não importada: taxas indisponíveis.".into());
    }
    let pop_years: BTreeSet<(i32, i32)> = indicators
        .iter()
        .filter_map(|i| i.population.as_ref().map(|p| (i.year, p.year)))
        .filter(|(y, p)| y != p)
        .collect();
    for (year, pop_year) in pop_years {
        notes.push(format!(
            "Taxas de {year} usam a população de {pop_year} (ano mais próximo disponível)."
        ));
    }
    if indicators.iter().any(|i| !i.complete) {
        notes.push("Há anos incompletos (complete = false): compare com cuidado.".into());
    }
    Transparency {
        sources: sources.iter().map(DataSource::metadata).collect(),
        last_updated: indicators.iter().map(|i| i.collected_at).max(),
        methodology: METHODOLOGY,
        notes,
    }
}

fn population_ref(row: PopulationRow) -> PopulationRef {
    PopulationRef {
        value: row.value,
        year: row.period_start.year(),
        source: DataSource::parse(&row.source).unwrap_or(DataSource::IbgeLocalidades),
        source_url: row.source_url,
    }
}

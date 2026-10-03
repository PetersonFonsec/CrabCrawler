//! Rotas do Regional Intelligence: perfil da região, serviços próximos e
//! riscos ambientais de um imóvel, sempre pela coordenada do anúncio.
//!
//! Regra central: `NO_DATA` (não há dado para esta localização, com o
//! motivo) é diferente de "a fonte foi consultada e não há nada mapeado".
//! Nenhuma resposta conclui ausência de risco ou de serviço por falta de
//! dataset.

use axum::extract::{Path, Query, State};
use axum::Json;
use chrono::{DateTime, NaiveDate, Utc};
use crab_domain::regional::{
    Capability, RiskType, SectorIndicator, ServiceCategory, REGIONAL_PROVIDERS,
};
use crab_domain::DataSource;
use crab_persistence::{DatasetCoverageRow, DatasetRef, ListingRow, RegionalImportRow, ServiceRow};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::ApiError;
use crate::AppState;

const DEFAULT_RADIUS_M: f64 = 1_000.0;
const MAX_RADIUS_M: f64 = 5_000.0;
const DEFAULT_RISK_DISTANCE_M: f64 = 2_000.0;
const MAX_RISK_DISTANCE_M: f64 = 10_000.0;
const MAX_SERVICES_PER_CATEGORY: i64 = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Availability {
    Available,
    NoData,
}

/// Por que não há dado. Os valores são estáveis (a interface traduz).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NoDataReason {
    /// O anúncio não tem latitude/longitude; não usamos centroide de bairro.
    PropertyWithoutCoordinates,
    /// A coordenada é do CEP ou do bairro: não serve para análise por ponto.
    PropertyCoordinatesApproximate,
    PropertyWithoutMunicipality,
    /// Nenhum provider conhecido cobre este município para este dado.
    NoProviderForLocation,
    /// Existe provider que cobre, mas o dataset não foi importado.
    NotImported,
    /// A malha do município foi importada, mas o ponto não cai em nenhum setor.
    OutsideImportedSectors,
    /// A fonte publica o campo, mas suprimiu o valor (sigilo).
    SuppressedBySource,
    /// O dado não é publicado pela fonte oficial nesta granularidade.
    NotPublished,
}

impl NoDataReason {
    fn message(&self) -> &'static str {
        match self {
            Self::PropertyWithoutCoordinates => {
                "O anúncio não tem coordenadas; sem elas não há análise geoespacial."
            }
            Self::PropertyCoordinatesApproximate => {
                "A coordenada do imóvel é aproximada (CEP ou bairro); a análise por ponto \
                 exige a coordenada do endereço."
            }
            Self::PropertyWithoutMunicipality => "O município do anúncio não foi identificado.",
            Self::NoProviderForLocation => {
                "Nenhuma fonte integrada cobre esta localização para este dado."
            }
            Self::NotImported => {
                "Há fonte oficial para esta localização, mas o dataset ainda não foi importado."
            }
            Self::OutsideImportedSectors => {
                "A coordenada não está dentro de nenhum setor censitário importado."
            }
            Self::SuppressedBySource => "A fonte suprimiu este valor (sigilo estatístico).",
            Self::NotPublished => {
                "Não encontramos este dado publicado pela fonte oficial nesta granularidade."
            }
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct NoData {
    pub item: String,
    pub status: Availability,
    pub reason: NoDataReason,
    pub message: &'static str,
}

impl NoData {
    fn new(item: impl Into<String>, reason: NoDataReason) -> Self {
        Self {
            item: item.into(),
            status: Availability::NoData,
            reason,
            message: reason.message(),
        }
    }
}

/// Procedência de um dado na resposta.
#[derive(Debug, Clone, Serialize)]
pub struct SourceRef {
    pub source: String,
    pub source_name: Option<&'static str>,
    pub dataset_name: String,
    pub dataset_version: String,
    pub source_url: Option<String>,
    pub reference_date: Option<NaiveDate>,
    pub collected_at: DateTime<Utc>,
    pub geographic_granularity: String,
}

impl From<DatasetRef> for SourceRef {
    fn from(d: DatasetRef) -> Self {
        Self {
            source_name: DataSource::parse(&d.source).map(|s| s.metadata().name),
            source: d.source,
            dataset_name: d.dataset_name,
            dataset_version: d.dataset_version,
            source_url: d.source_url,
            reference_date: d.reference_date,
            collected_at: d.collected_at,
            geographic_granularity: d.geographic_granularity,
        }
    }
}

impl From<DatasetCoverageRow> for SourceRef {
    fn from(d: DatasetCoverageRow) -> Self {
        Self::from(DatasetRef {
            source: d.source,
            dataset_name: d.dataset_name,
            dataset_version: d.dataset_version,
            source_url: d.source_url,
            reference_date: d.reference_date,
            collected_at: d.collected_at,
            geographic_granularity: d.geographic_granularity,
        })
    }
}

/// Onde o imóvel está, como usado nas consultas.
#[derive(Debug, Clone, Serialize)]
pub struct PropertyLocation {
    pub listing_id: Uuid,
    pub property_id: Uuid,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub municipality: Option<String>,
    pub municipality_ibge_code: Option<String>,
    pub neighborhood: Option<String>,
    pub postal_code: Option<String>,
    /// De onde veio a coordenada (fonte, geocoding, manual...).
    pub coordinate_source: Option<String>,
    /// `EXACT`, `STREET`, `POSTAL_CODE`, `APPROXIMATE` ou `REPORTED`.
    pub coordinate_precision: Option<String>,
}

impl From<&ListingRow> for PropertyLocation {
    fn from(l: &ListingRow) -> Self {
        Self {
            listing_id: l.id,
            property_id: l.property_id,
            lat: l.lat,
            lon: l.lon,
            municipality: l.municipality.clone(),
            municipality_ibge_code: l.municipality_ibge_code.clone(),
            neighborhood: l.neighborhood.clone(),
            postal_code: l.postal_code.clone(),
            coordinate_source: l.coordinate_source.clone(),
            coordinate_precision: l.coordinate_precision.clone(),
        }
    }
}

/// Coordenada e município do anúncio, ou o motivo de não haver análise.
struct Anchor {
    lat: f64,
    lon: f64,
    municipality: String,
}

fn anchor(listing: &ListingRow) -> Result<Anchor, NoDataReason> {
    let (Some(lat), Some(lon)) = (listing.lat, listing.lon) else {
        return Err(NoDataReason::PropertyWithoutCoordinates);
    };
    if matches!(
        listing.coordinate_precision.as_deref(),
        Some("POSTAL_CODE") | Some("APPROXIMATE")
    ) {
        return Err(NoDataReason::PropertyCoordinatesApproximate);
    }
    let municipality = listing
        .municipality_ibge_code
        .clone()
        .ok_or(NoDataReason::PropertyWithoutMunicipality)?;
    Ok(Anchor {
        lat,
        lon,
        municipality,
    })
}

/// Motivo de não haver dataset: há provider que cobre (não importado) ou
/// nenhum cobre. `category` restringe a providers de serviço daquela categoria.
fn missing_reason(
    capability: Capability,
    municipality: &str,
    category: Option<ServiceCategory>,
) -> NoDataReason {
    let covered = REGIONAL_PROVIDERS.iter().any(|p| {
        p.provides(capability)
            && category.is_none_or(|c| p.service_categories.contains(&c))
            && p.coverage.covers(municipality) != Some(false)
    });
    if covered {
        NoDataReason::NotImported
    } else {
        NoDataReason::NoProviderForLocation
    }
}

async fn load_listing(state: &AppState, id: Uuid) -> Result<ListingRow, ApiError> {
    state.listings.get(id).await?.ok_or(ApiError::NotFound)
}

// ------------------------------------------------------------------- perfil

#[derive(Debug, Clone, Serialize)]
pub struct SectorInfo {
    pub code: String,
    pub municipality_ibge_code: String,
    pub municipality_name: Option<String>,
    pub area_km2: Option<f64>,
    pub source: SourceRef,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProfileIndicator {
    pub indicator: String,
    pub value: Option<f64>,
    /// Texto original (rótulo do grupo do IPVS, marcador de sigilo).
    pub value_text: Option<String>,
    pub unit: &'static str,
    /// Sempre a granularidade real do dado.
    pub granularity: &'static str,
    pub source_variable: String,
    /// `true` quando o número é calculado pelo CrabCrawler a partir de dados oficiais.
    pub derived: bool,
    pub method: Option<&'static str>,
    pub source: SourceRef,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegionalProfile {
    pub status: Availability,
    pub census_sector: Option<SectorInfo>,
    pub indicators: Vec<ProfileIndicator>,
    /// O que não está disponível e por quê.
    pub unavailable: Vec<NoData>,
}

async fn build_profile(
    state: &AppState,
    listing: &ListingRow,
) -> Result<RegionalProfile, ApiError> {
    let no_data = |reason| RegionalProfile {
        status: Availability::NoData,
        census_sector: None,
        indicators: vec![],
        unavailable: vec![NoData::new("census_sector", reason)],
    };
    let a = match anchor(listing) {
        Ok(a) => a,
        Err(reason) => return Ok(no_data(reason)),
    };
    let Some(sector) = state.regional.sector_at(a.lat, a.lon).await? else {
        let imported = !state
            .regional
            .coverage(Capability::CensusSectors, &a.municipality)
            .await?
            .is_empty();
        let reason = if imported {
            NoDataReason::OutsideImportedSectors
        } else {
            missing_reason(Capability::CensusSectors, &a.municipality, None)
        };
        return Ok(no_data(reason));
    };

    let rows = state.regional.sector_indicators(&sector.code).await?;
    let mut indicators = Vec::new();
    let mut unavailable = Vec::new();
    let mut population = None;
    for row in rows {
        let Some(kind) = SectorIndicator::parse(&row.indicator) else {
            continue;
        };
        if row.value.is_none() && kind != SectorIndicator::IpvsGroup {
            unavailable.push(NoData::new(
                row.indicator.clone(),
                NoDataReason::SuppressedBySource,
            ));
        }
        if kind == SectorIndicator::Population {
            population = row.value.map(|v| (v, row.dataset.clone()));
        }
        indicators.push(ProfileIndicator {
            indicator: row.indicator,
            value: row.value,
            value_text: row.value_text,
            unit: kind.unit(),
            granularity: "census_sector",
            source_variable: row.source_variable,
            derived: false,
            method: None,
            source: row.dataset.into(),
        });
    }
    // Densidade: não publicada no arquivo Básico; calculada e marcada como tal.
    if let (Some((pop, dataset)), Some(area)) = (population, sector.area_km2.filter(|a| *a > 0.0)) {
        indicators.push(ProfileIndicator {
            indicator: "population_density".into(),
            value: Some((pop / area).round()),
            value_text: None,
            unit: "pessoas por km²",
            granularity: "census_sector",
            source_variable: "V0001 / AREA_KM2".into(),
            derived: true,
            method: Some("Pessoas (V0001) divididas pela área do setor na malha do IBGE."),
            source: dataset.into(),
        });
    }
    let has = |k: SectorIndicator| indicators.iter().any(|i| i.indicator == k.as_str());
    if !has(SectorIndicator::Population) {
        unavailable.push(NoData::new(
            "demographics",
            missing_reason(Capability::Demographics, &a.municipality, None),
        ));
    }
    if !has(SectorIndicator::IpvsGroup) {
        unavailable.push(NoData::new(
            "social_vulnerability",
            missing_reason(Capability::SocialVulnerability, &a.municipality, None),
        ));
    }
    unavailable.push(NoData::new("average_income", NoDataReason::NotPublished));

    Ok(RegionalProfile {
        status: Availability::Available,
        census_sector: Some(SectorInfo {
            code: sector.code,
            municipality_ibge_code: sector.municipality_ibge_code,
            municipality_name: sector.municipality_name,
            area_km2: sector.area_km2,
            source: sector.dataset.into(),
        }),
        indicators,
        unavailable,
    })
}

#[derive(Serialize)]
pub struct ProfileResponse {
    property: PropertyLocation,
    profile: RegionalProfile,
}

pub async fn listing_profile(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ProfileResponse>, ApiError> {
    let listing = load_listing(&state, id).await?;
    Ok(Json(ProfileResponse {
        property: (&listing).into(),
        profile: build_profile(&state, &listing).await?,
    }))
}

// ----------------------------------------------------------------- serviços

#[derive(Debug, Default, Deserialize)]
pub struct ServicesQuery {
    pub radius: Option<f64>,
    pub category: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServiceItem {
    pub external_id: String,
    pub name: Option<String>,
    pub category: String,
    pub subcategory: Option<String>,
    pub address: Option<String>,
    pub lat: f64,
    pub lon: f64,
    /// Distância geodésica em linha reta, em metros.
    pub distance_m: f64,
    pub distance_label: String,
    pub source: SourceRef,
}

impl From<ServiceRow> for ServiceItem {
    fn from(r: ServiceRow) -> Self {
        Self {
            distance_label: distance_label(r.distance_m),
            distance_m: r.distance_m.round(),
            external_id: r.external_id,
            name: r.name,
            category: r.category,
            subcategory: r.subcategory,
            address: r.address,
            lat: r.lat,
            lon: r.lon,
            source: r.dataset.into(),
        }
    }
}

/// `620 m`, `1,4 km`.
pub fn distance_label(meters: f64) -> String {
    if meters < 1_000.0 {
        format!("{} m", ((meters / 10.0).round() * 10.0) as i64)
    } else {
        format!("{:.1} km", meters / 1_000.0).replace('.', ",")
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CategoryServices {
    pub category: ServiceCategory,
    pub label: &'static str,
    pub status: Availability,
    pub reason: Option<NoDataReason>,
    pub message: Option<&'static str>,
    /// O mais próximo, sem limite de raio (só entre os datasets importados).
    pub nearest: Option<ServiceItem>,
    pub within_radius: Vec<ServiceItem>,
    /// Datasets considerados: só os tipos de equipamento listados aqui foram
    /// procurados.
    pub datasets: Vec<SourceRef>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegionalServices {
    pub status: Availability,
    pub reason: Option<NoDataReason>,
    pub radius_m: f64,
    pub distance_method: &'static str,
    pub categories: Vec<CategoryServices>,
}

fn parse_services_query(q: &ServicesQuery) -> Result<(f64, Vec<ServiceCategory>), ApiError> {
    let radius = q.radius.unwrap_or(DEFAULT_RADIUS_M);
    if !(1.0..=MAX_RADIUS_M).contains(&radius) {
        return Err(ApiError::BadRequest(format!(
            "radius deve estar entre 1 e {MAX_RADIUS_M} metros"
        )));
    }
    let categories = match &q.category {
        None => ServiceCategory::ALL.to_vec(),
        Some(raw) => raw
            .split(',')
            .map(|c| {
                ServiceCategory::parse(c)
                    .ok_or_else(|| ApiError::BadRequest(format!("categoria desconhecida: {c}")))
            })
            .collect::<Result<_, _>>()?,
    };
    Ok((radius, categories))
}

async fn build_services(
    state: &AppState,
    listing: &ListingRow,
    radius: f64,
    categories: &[ServiceCategory],
) -> Result<RegionalServices, ApiError> {
    let method = "Distância geodésica (PostGIS, geography) entre a coordenada do anúncio e a do equipamento.";
    let a = match anchor(listing) {
        Ok(a) => a,
        Err(reason) => {
            return Ok(RegionalServices {
                status: Availability::NoData,
                reason: Some(reason),
                radius_m: radius,
                distance_method: method,
                categories: vec![],
            })
        }
    };
    let coverage = state
        .regional
        .coverage(Capability::UrbanServices, &a.municipality)
        .await?;
    let covered: Vec<String> = categories
        .iter()
        .filter(|c| {
            coverage
                .iter()
                .any(|d| d.categories.iter().any(|x| x == c.as_str()))
        })
        .map(|c| c.as_str().to_string())
        .collect();
    let (within, nearest) = if covered.is_empty() {
        (vec![], vec![])
    } else {
        (
            state
                .regional
                .services_within(a.lat, a.lon, radius, &covered, MAX_SERVICES_PER_CATEGORY)
                .await?,
            state
                .regional
                .nearest_service_per_category(a.lat, a.lon, &covered)
                .await?,
        )
    };

    let mut out = Vec::new();
    for &category in categories {
        let datasets: Vec<SourceRef> = coverage
            .iter()
            .filter(|d| d.categories.iter().any(|x| x == category.as_str()))
            .cloned()
            .map(Into::into)
            .collect();
        if datasets.is_empty() {
            let reason = missing_reason(Capability::UrbanServices, &a.municipality, Some(category));
            out.push(CategoryServices {
                category,
                label: category.label_pt(),
                status: Availability::NoData,
                reason: Some(reason),
                message: Some(reason.message()),
                nearest: None,
                within_radius: vec![],
                datasets,
            });
            continue;
        }
        out.push(CategoryServices {
            category,
            label: category.label_pt(),
            status: Availability::Available,
            reason: None,
            message: None,
            nearest: nearest
                .iter()
                .find(|s| s.category == category.as_str())
                .cloned()
                .map(Into::into),
            within_radius: within
                .iter()
                .filter(|s| s.category == category.as_str())
                .cloned()
                .map(Into::into)
                .collect(),
            datasets,
        });
    }
    let status = if out.iter().any(|c| c.status == Availability::Available) {
        Availability::Available
    } else {
        Availability::NoData
    };
    Ok(RegionalServices {
        status,
        reason: None,
        radius_m: radius,
        distance_method: method,
        categories: out,
    })
}

#[derive(Serialize)]
pub struct ServicesResponse {
    property: PropertyLocation,
    services: RegionalServices,
}

pub async fn listing_services(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<ServicesQuery>,
) -> Result<Json<ServicesResponse>, ApiError> {
    let (radius, categories) = parse_services_query(&q)?;
    let listing = load_listing(&state, id).await?;
    Ok(Json(ServicesResponse {
        property: (&listing).into(),
        services: build_services(&state, &listing, radius, &categories).await?,
    }))
}

// ------------------------------------------------------------------- riscos

#[derive(Debug, Default, Deserialize)]
pub struct RisksQuery {
    pub max_distance: Option<f64>,
    #[serde(rename = "type")]
    pub risk_type: Option<String>,
}

/// Leitura dos riscos na localização. Nunca há "sem risco".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RiskAssessment {
    /// A coordenada está dentro de ao menos uma área de risco mapeada.
    InsideMappedRiskArea,
    /// Há dataset para o município e a coordenada não está em nenhuma área
    /// mapeada. Não significa ausência de risco.
    NotInMappedRiskArea,
    NoData,
}

#[derive(Debug, Clone, Serialize)]
pub struct RiskEntry {
    #[serde(rename = "type")]
    pub risk_type: String,
    pub label: &'static str,
    pub inside_risk_area: bool,
    /// 0 quando dentro da área.
    pub distance_meters: f64,
    /// Grau de risco exatamente como a fonte publica.
    pub severity: Option<String>,
    pub source_labels: Vec<String>,
    pub location_name: Option<String>,
    pub mapped_on: Option<NaiveDate>,
    pub external_id: String,
    pub source: SourceRef,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegionalRisks {
    pub assessment: RiskAssessment,
    pub reason: Option<NoDataReason>,
    pub message: String,
    pub search_radius_m: f64,
    /// Para cada tipo de risco encontrado no raio: a área que contém o
    /// imóvel ou, se nenhuma, a mais próxima.
    pub risks: Vec<RiskEntry>,
    pub datasets: Vec<SourceRef>,
}

fn parse_risks_query(q: &RisksQuery) -> Result<(f64, Option<RiskType>), ApiError> {
    let max = q.max_distance.unwrap_or(DEFAULT_RISK_DISTANCE_M);
    if !(0.0..=MAX_RISK_DISTANCE_M).contains(&max) {
        return Err(ApiError::BadRequest(format!(
            "max_distance deve estar entre 0 e {MAX_RISK_DISTANCE_M} metros"
        )));
    }
    let risk_type = q
        .risk_type
        .as_deref()
        .map(|t| {
            RiskType::parse(t)
                .ok_or_else(|| ApiError::BadRequest(format!("tipo desconhecido: {t}")))
        })
        .transpose()?;
    Ok((max, risk_type))
}

async fn build_risks(
    state: &AppState,
    listing: &ListingRow,
    max_distance: f64,
    filter: Option<RiskType>,
) -> Result<RegionalRisks, ApiError> {
    let no_data = |reason: NoDataReason| RegionalRisks {
        assessment: RiskAssessment::NoData,
        reason: Some(reason),
        message: reason.message().to_string(),
        search_radius_m: max_distance,
        risks: vec![],
        datasets: vec![],
    };
    let a = match anchor(listing) {
        Ok(a) => a,
        Err(reason) => return Ok(no_data(reason)),
    };
    let coverage = state
        .regional
        .coverage(Capability::EnvironmentalRisk, &a.municipality)
        .await?;
    if coverage.is_empty() {
        return Ok(no_data(missing_reason(
            Capability::EnvironmentalRisk,
            &a.municipality,
            None,
        )));
    }
    let rows = state
        .regional
        .risk_areas_near(a.lat, a.lon, max_distance)
        .await?;

    // Por tipo: a área que contém o ponto ou a mais próxima (rows já vêm
    // ordenadas por distância; dentro = distância 0).
    let mut risks: Vec<RiskEntry> = Vec::new();
    for row in rows {
        for t in &row.risk_types {
            let Some(kind) = RiskType::parse(t) else {
                continue;
            };
            if filter.is_some_and(|f| f != kind)
                || risks.iter().any(|r| r.risk_type == kind.as_str())
            {
                continue;
            }
            risks.push(RiskEntry {
                risk_type: kind.as_str().to_string(),
                label: kind.label_pt(),
                inside_risk_area: row.inside,
                distance_meters: if row.inside {
                    0.0
                } else {
                    row.distance_m.round()
                },
                severity: row.severity.clone(),
                source_labels: row.source_labels.clone(),
                location_name: row.location_name.clone(),
                mapped_on: row.mapped_on,
                external_id: row.external_id.clone(),
                source: row.dataset.clone().into(),
            });
        }
    }
    let inside = risks.iter().any(|r| r.inside_risk_area);
    let (assessment, message) = if inside {
        (
            RiskAssessment::InsideMappedRiskArea,
            "O imóvel está dentro de uma área de risco mapeada pela fonte oficial.".to_string(),
        )
    } else {
        (
            RiskAssessment::NotInMappedRiskArea,
            "O imóvel não está dentro de nenhuma área de risco mapeada pelas fontes importadas. \
             Isso não significa ausência de risco: as fontes mapeiam apenas áreas específicas."
                .to_string(),
        )
    };
    Ok(RegionalRisks {
        assessment,
        reason: None,
        message,
        search_radius_m: max_distance,
        risks,
        datasets: coverage.into_iter().map(Into::into).collect(),
    })
}

#[derive(Serialize)]
pub struct RisksResponse {
    property: PropertyLocation,
    environmental_risks: RegionalRisks,
}

pub async fn listing_risks(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<RisksQuery>,
) -> Result<Json<RisksResponse>, ApiError> {
    let (max, filter) = parse_risks_query(&q)?;
    let listing = load_listing(&state, id).await?;
    Ok(Json(RisksResponse {
        property: (&listing).into(),
        environmental_risks: build_risks(&state, &listing, max, filter).await?,
    }))
}

// ---------------------------------------------------------------- agregados

#[derive(Serialize)]
pub struct Region {
    profile: RegionalProfile,
    services: RegionalServices,
    environmental_risks: RegionalRisks,
}

async fn build_region(state: &AppState, listing: &ListingRow) -> Result<Region, ApiError> {
    Ok(Region {
        profile: build_profile(state, listing).await?,
        services: build_services(state, listing, DEFAULT_RADIUS_M, &ServiceCategory::ALL).await?,
        environmental_risks: build_risks(state, listing, DEFAULT_RISK_DISTANCE_M, None).await?,
    })
}

#[derive(Serialize)]
pub struct RegionResponse {
    property: PropertyLocation,
    region: Region,
}

/// Perfil + serviços (raio padrão) + riscos de um imóvel.
pub async fn listing_region(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<RegionResponse>, ApiError> {
    let listing = load_listing(&state, id).await?;
    Ok(Json(RegionResponse {
        property: (&listing).into(),
        region: build_region(&state, &listing).await?,
    }))
}

#[derive(Serialize)]
pub struct IntelligenceResponse {
    property: ListingRow,
    region: Region,
}

/// Imóvel completo + região. Base para comparar imóveis no futuro.
pub async fn listing_intelligence(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<IntelligenceResponse>, ApiError> {
    let listing = load_listing(&state, id).await?;
    let region = build_region(&state, &listing).await?;
    Ok(Json(IntelligenceResponse {
        property: listing,
        region,
    }))
}

/// Anúncio principal do imóvel, usado como âncora das análises. A origem
/// do anúncio não muda nada: só coordenada e município importam.
async fn load_property_anchor(state: &AppState, property_id: Uuid) -> Result<ListingRow, ApiError> {
    state
        .listings
        .primary_for_property(property_id)
        .await?
        .ok_or(ApiError::NotFound)
}

/// `GET /properties/{id}/region`
pub async fn property_region(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<RegionResponse>, ApiError> {
    let listing = load_property_anchor(&state, id).await?;
    Ok(Json(RegionResponse {
        property: (&listing).into(),
        region: build_region(&state, &listing).await?,
    }))
}

/// `GET /properties/{id}/intelligence`
pub async fn property_intelligence(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<IntelligenceResponse>, ApiError> {
    let listing = load_property_anchor(&state, id).await?;
    let region = build_region(&state, &listing).await?;
    Ok(Json(IntelligenceResponse {
        property: listing,
        region,
    }))
}

// ------------------------------------------------------------------- fontes

#[derive(Serialize)]
pub struct ProviderInfo {
    source: &'static str,
    name: &'static str,
    publisher: &'static str,
    reference_url: &'static str,
    methodology: &'static str,
    limitations: &'static [&'static str],
    coverage: crab_domain::regional::Coverage,
    capabilities: &'static [Capability],
    service_categories: &'static [ServiceCategory],
    refresh: crab_domain::regional::RefreshPolicy,
    last_successful_import: Option<DateTime<Utc>>,
    /// `true` quando a última importação bem-sucedida é mais antiga que o
    /// intervalo sugerido, ou quando nunca houve importação.
    refresh_due: bool,
}

#[derive(Serialize)]
pub struct RegionalSources {
    providers: Vec<ProviderInfo>,
    imports: Vec<RegionalImportRow>,
}

pub async fn sources(State(state): State<AppState>) -> Result<Json<RegionalSources>, ApiError> {
    let last = state.regional.last_success_by_source().await?;
    let now = Utc::now();
    let providers = REGIONAL_PROVIDERS
        .iter()
        .map(|p| {
            let meta = p.source.metadata();
            let last_ok = last
                .iter()
                .find(|(s, _)| s == p.source.as_str())
                .map(|(_, t)| *t);
            ProviderInfo {
                source: meta.id,
                name: meta.name,
                publisher: meta.publisher,
                reference_url: meta.reference_url,
                methodology: meta.methodology,
                limitations: meta.limitations,
                coverage: p.coverage,
                capabilities: p.capabilities,
                service_categories: p.service_categories,
                refresh: p.refresh,
                last_successful_import: last_ok,
                refresh_due: last_ok
                    .is_none_or(|t| (now - t).num_days() >= i64::from(p.refresh.interval_days)),
            }
        })
        .collect();
    Ok(Json(RegionalSources {
        providers,
        imports: state.regional.latest_imports().await?,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distance_labels() {
        assert_eq!(distance_label(617.0), "620 m");
        assert_eq!(distance_label(1_420.0), "1,4 km");
        assert_eq!(distance_label(2_080.0), "2,1 km");
        assert_eq!(distance_label(4.0), "0 m");
    }

    #[test]
    fn missing_reason_uses_provider_coverage() {
        // GeoSampa só cobre a capital: em SBC não há provider de serviços.
        assert_eq!(
            missing_reason(
                Capability::UrbanServices,
                "3548708",
                Some(ServiceCategory::Health)
            ),
            NoDataReason::NoProviderForLocation
        );
        assert_eq!(
            missing_reason(
                Capability::UrbanServices,
                "3550308",
                Some(ServiceCategory::Health)
            ),
            NoDataReason::NotImported
        );
        // Transporte: nenhum provider integrado, nem na capital.
        assert_eq!(
            missing_reason(
                Capability::UrbanServices,
                "3550308",
                Some(ServiceCategory::Transport)
            ),
            NoDataReason::NoProviderForLocation
        );
        // IPVS só cobre SP.
        assert_eq!(
            missing_reason(Capability::SocialVulnerability, "3304557", None),
            NoDataReason::NoProviderForLocation
        );
        // SGB: cobertura só conhecida após importar.
        assert_eq!(
            missing_reason(Capability::EnvironmentalRisk, "3304557", None),
            NoDataReason::NotImported
        );
    }

    #[test]
    fn query_validation() {
        assert!(parse_services_query(&ServicesQuery {
            radius: Some(10_000.0),
            category: None
        })
        .is_err());
        let (r, c) = parse_services_query(&ServicesQuery {
            radius: None,
            category: Some("health,PARK".into()),
        })
        .unwrap();
        assert_eq!(r, DEFAULT_RADIUS_M);
        assert_eq!(c, vec![ServiceCategory::Health, ServiceCategory::Park]);
        assert!(parse_services_query(&ServicesQuery {
            radius: None,
            category: Some("bar".into())
        })
        .is_err());
        assert!(parse_risks_query(&RisksQuery {
            max_distance: None,
            risk_type: Some("tsunami".into())
        })
        .is_err());
    }
}

use serde::{Deserialize, Serialize};

/// Coordenada WGS84 (SRID 4326).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GeoPoint {
    pub lat: f64,
    pub lon: f64,
}

impl GeoPoint {
    pub fn new(lat: f64, lon: f64) -> Option<Self> {
        let valid = (-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon);
        valid.then_some(Self { lat, lon })
    }
}

/// Localização normalizada de um anúncio ou registro público.
///
/// `municipality_ibge_code` é a chave principal de junção entre fontes: todo
/// dataset oficial brasileiro usa o código IBGE de 7 dígitos do município.
/// `neighborhood_slug` é o bairro normalizado (sem acento, minúsculo) e serve
/// de junção mais fina quando não há coordenada.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Location {
    pub state: Option<String>,
    pub municipality: Option<String>,
    pub municipality_ibge_code: Option<String>,
    pub neighborhood: Option<String>,
    pub neighborhood_slug: Option<String>,
    pub street: Option<String>,
    pub postal_code: Option<String>,
    pub point: Option<GeoPoint>,
}

/// Nível geográfico ao qual um indicador se refere.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegionLevel {
    Municipality,
    Neighborhood,
    /// Setor censitário do IBGE.
    CensusTract,
    /// Área de cobertura de uma delegacia (granularidade dos dados da SSP-SP).
    PoliceDistrict,
}

impl RegionLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Municipality => "municipality",
            Self::Neighborhood => "neighborhood",
            Self::CensusTract => "census_tract",
            Self::PoliceDistrict => "police_district",
        }
    }
}

/// Uma região identificável à qual indicadores são associados.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Region {
    pub level: RegionLevel,
    /// Código estável da região na fonte (código IBGE, nome da delegacia...).
    pub code: String,
    pub name: String,
    pub municipality_ibge_code: String,
}

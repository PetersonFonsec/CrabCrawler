//! Regional Intelligence: perfil da região, serviços próximos e riscos
//! ambientais, sempre ligados ao imóvel pela coordenada.
//!
//! Princípios do módulo:
//! - todo registro aponta para um dataset com procedência completa
//!   ([`DatasetProvenance`]) e guarda o identificador original da fonte;
//! - a granularidade é a da fonte: dado de setor censitário é apresentado
//!   como setor, polígono de risco é analisado como polígono;
//! - ausência de dado nunca vira ausência de risco ou de serviço: cada
//!   provider declara cobertura e capacidades ([`ProviderDescriptor`]);
//! - classificações (grau de risco, grupo do IPVS) são as da fonte, sem
//!   notas ou escalas inventadas.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use crate::DataSource;

/// Categorias de serviço/equipamento urbano. Novas categorias entram aqui
/// sem mudar tabela (a coluna é texto).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ServiceCategory {
    Health,
    Education,
    Transport,
    Park,
    Culture,
    Sport,
}

impl ServiceCategory {
    pub const ALL: [ServiceCategory; 6] = [
        Self::Health,
        Self::Education,
        Self::Transport,
        Self::Park,
        Self::Culture,
        Self::Sport,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Health => "HEALTH",
            Self::Education => "EDUCATION",
            Self::Transport => "TRANSPORT",
            Self::Park => "PARK",
            Self::Culture => "CULTURE",
            Self::Sport => "SPORT",
        }
    }

    /// Aceita `HEALTH`, `health` etc.
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|c| c.as_str().eq_ignore_ascii_case(value.trim()))
    }

    pub fn label_pt(&self) -> &'static str {
        match self {
            Self::Health => "Saúde",
            Self::Education => "Educação",
            Self::Transport => "Transporte",
            Self::Park => "Parques",
            Self::Culture => "Cultura",
            Self::Sport => "Esporte",
        }
    }
}

/// Tipos de risco ambiental. Seguem a COBRADE (Classificação e Codificação
/// Brasileira de Desastres), que é a classificação usada pelo SGB e pela
/// Defesa Civil.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RiskType {
    /// Inundação (COBRADE 1.2.1).
    Flood,
    /// Enxurrada (COBRADE 1.2.2).
    FlashFlood,
    /// Alagamento (COBRADE 1.2.3).
    UrbanFlooding,
    /// Deslizamento (COBRADE 1.1.3.2).
    Landslide,
    /// Outros movimentos de massa: quedas, rolamentos, corridas, rastejo,
    /// subsidência (COBRADE 1.1.3.x).
    Geological,
    /// Risco hidrológico sem subtipo informado pela fonte.
    Hydrological,
    /// Erosão (COBRADE 1.1.4).
    Erosion,
    /// Tipologia que não se encaixa nas anteriores (rótulo original guardado).
    Other,
}

impl RiskType {
    pub const ALL: [RiskType; 8] = [
        Self::Flood,
        Self::FlashFlood,
        Self::UrbanFlooding,
        Self::Landslide,
        Self::Geological,
        Self::Hydrological,
        Self::Erosion,
        Self::Other,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Flood => "FLOOD",
            Self::FlashFlood => "FLASH_FLOOD",
            Self::UrbanFlooding => "URBAN_FLOODING",
            Self::Landslide => "LANDSLIDE",
            Self::Geological => "GEOLOGICAL",
            Self::Hydrological => "HYDROLOGICAL",
            Self::Erosion => "EROSION",
            Self::Other => "OTHER",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|t| t.as_str().eq_ignore_ascii_case(value.trim()))
    }

    pub fn label_pt(&self) -> &'static str {
        match self {
            Self::Flood => "Inundação",
            Self::FlashFlood => "Enxurrada",
            Self::UrbanFlooding => "Alagamento",
            Self::Landslide => "Deslizamento",
            Self::Geological => "Movimento de massa (outros)",
            Self::Hydrological => "Risco hidrológico",
            Self::Erosion => "Erosão",
            Self::Other => "Outro",
        }
    }
}

/// O que um provider sabe fornecer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    /// Malha (polígonos) de setores censitários.
    CensusSectors,
    /// Indicadores demográficos por setor.
    Demographics,
    /// Vulnerabilidade social por setor.
    SocialVulnerability,
    /// Equipamentos urbanos georreferenciados.
    UrbanServices,
    /// Áreas de risco ambiental (polígonos).
    EnvironmentalRisk,
}

impl Capability {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::CensusSectors => "census_sectors",
            Self::Demographics => "demographics",
            Self::SocialVulnerability => "social_vulnerability",
            Self::UrbanServices => "urban_services",
            Self::EnvironmentalRisk => "environmental_risk",
        }
    }
}

/// Nível geográfico real de um dado.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GeographicGranularity {
    Municipality,
    CensusSector,
    /// Coordenada de um equipamento.
    Point,
    /// Polígono próprio da fonte (ex.: setor de risco).
    Polygon,
}

impl GeographicGranularity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Municipality => "municipality",
            Self::CensusSector => "census_sector",
            Self::Point => "point",
            Self::Polygon => "polygon",
        }
    }
}

/// Cobertura geográfica declarada por um provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "codes", rename_all = "snake_case")]
pub enum Coverage {
    /// Todo o Brasil.
    Country,
    /// UFs, pelo código IBGE de 2 dígitos (SP = "35").
    States(&'static [&'static str]),
    /// Municípios, pelo código IBGE de 7 dígitos.
    Municipalities(&'static [&'static str]),
    /// Só os municípios que a própria fonte mapeou; a lista é conhecida
    /// depois da importação (ex.: setorização de risco do SGB).
    MappedMunicipalities,
}

impl Coverage {
    /// `Some(true/false)` quando a cobertura é conhecida sem consultar a
    /// fonte; `None` quando depende do que foi importado.
    pub fn covers(&self, municipality_ibge_code: &str) -> Option<bool> {
        match self {
            Self::Country => Some(true),
            Self::States(ufs) => Some(ufs.iter().any(|uf| municipality_ibge_code.starts_with(uf))),
            Self::Municipalities(codes) => Some(codes.contains(&municipality_ibge_code)),
            Self::MappedMunicipalities => None,
        }
    }
}

/// Frequência de atualização de um dataset. Cada provider tem a sua.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RefreshPolicy {
    /// `decennial`, `annual`, `monthly`, `on_demand`...
    pub cadence: &'static str,
    /// Intervalo sugerido entre importações, em dias.
    pub interval_days: u32,
}

/// Descrição de um provider para o domínio e para a API.
#[derive(Debug, Clone, Serialize)]
pub struct ProviderDescriptor {
    pub source: DataSource,
    pub coverage: Coverage,
    pub capabilities: &'static [Capability],
    /// Categorias de serviço que o provider consegue fornecer (vazio se não
    /// fornece serviços).
    pub service_categories: &'static [ServiceCategory],
    pub refresh: RefreshPolicy,
}

impl ProviderDescriptor {
    pub fn provides(&self, capability: Capability) -> bool {
        self.capabilities.contains(&capability)
    }
}

/// Procedência de um dataset importado. Todo registro regional aponta para
/// uma destas (via `regional_datasets`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DatasetProvenance {
    pub source: DataSource,
    /// Nome do dataset na fonte (arquivo, camada WFS, serviço).
    pub dataset_name: String,
    /// Versão: nome publicado do arquivo e/ou checksum do conteúdo bruto.
    pub dataset_version: String,
    pub source_url: Option<String>,
    /// Data a que o dado se refere (ex.: data de referência do Censo).
    pub reference_date: Option<NaiveDate>,
    pub collected_at: DateTime<Utc>,
    pub granularity: GeographicGranularity,
}

/// Geometria como veio da fonte; a conversão para EPSG:4326 é feita no
/// PostGIS (`ST_Transform`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RawGeometry {
    Wkb { bytes: Vec<u8>, srid: i32 },
    GeoJson { json: String, srid: i32 },
}

/// Setor censitário lido da malha, antes da validação.
#[derive(Debug, Clone, PartialEq)]
pub struct RawCensusSector {
    pub code: String,
    pub municipality_name: Option<String>,
    pub area_km2: Option<f64>,
    pub geometry: RawGeometry,
}

/// Setor censitário validado (código de 15 dígitos).
#[derive(Debug, Clone, PartialEq)]
pub struct CensusSector {
    pub code: String,
    pub municipality_ibge_code: String,
    pub municipality_name: Option<String>,
    pub area_km2: Option<f64>,
    pub geometry: RawGeometry,
}

/// Valor de uma variável por setor, como veio da fonte (texto).
#[derive(Debug, Clone, PartialEq)]
pub struct RawSectorValue {
    pub sector_code: String,
    /// Nome da coluna/variável na fonte (ex.: `V0001`, `ipvs`).
    pub variable: String,
    pub value: String,
}

/// Indicadores por setor que o projeto sabe interpretar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SectorIndicator {
    /// Pessoas residentes (IBGE V0001).
    Population,
    /// Total de domicílios (IBGE V0002).
    Households,
    /// Domicílios particulares (IBGE V0003).
    PrivateHouseholds,
    /// Domicílios coletivos (IBGE V0004).
    CollectiveHouseholds,
    /// Média de moradores em domicílios particulares ocupados (IBGE V0005).
    AvgResidentsPerHousehold,
    /// Domicílios particulares ocupados (IBGE V0007).
    OccupiedPrivateHouseholds,
    /// Grupo do Índice Paulista de Vulnerabilidade Social (Seade).
    IpvsGroup,
}

impl SectorIndicator {
    pub const ALL: [SectorIndicator; 7] = [
        Self::Population,
        Self::Households,
        Self::PrivateHouseholds,
        Self::CollectiveHouseholds,
        Self::AvgResidentsPerHousehold,
        Self::OccupiedPrivateHouseholds,
        Self::IpvsGroup,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Population => "population",
            Self::Households => "households",
            Self::PrivateHouseholds => "private_households",
            Self::CollectiveHouseholds => "collective_households",
            Self::AvgResidentsPerHousehold => "avg_residents_per_household",
            Self::OccupiedPrivateHouseholds => "occupied_private_households",
            Self::IpvsGroup => "ipvs_group",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|i| i.as_str() == value)
    }

    /// Variáveis do arquivo Básico dos Agregados por Setores (Censo 2022)
    /// que o projeto usa. V0006 (percentual de domicílios imputados) é
    /// qualidade da coleta, não perfil da região, e fica de fora.
    pub fn from_ibge_basic(variable: &str) -> Option<Self> {
        match variable.trim().to_ascii_uppercase().as_str() {
            "V0001" => Some(Self::Population),
            "V0002" => Some(Self::Households),
            "V0003" => Some(Self::PrivateHouseholds),
            "V0004" => Some(Self::CollectiveHouseholds),
            "V0005" => Some(Self::AvgResidentsPerHousehold),
            "V0007" => Some(Self::OccupiedPrivateHouseholds),
            _ => None,
        }
    }

    pub fn unit(&self) -> &'static str {
        match self {
            Self::Population => "pessoas",
            Self::AvgResidentsPerHousehold => "pessoas por domicílio",
            Self::IpvsGroup => "grupo",
            _ => "domicílios",
        }
    }
}

/// Valor validado de um indicador por setor.
#[derive(Debug, Clone, PartialEq)]
pub struct SectorIndicatorValue {
    pub sector_code: String,
    pub indicator: SectorIndicator,
    /// `None` quando a fonte suprime o valor (ex.: `X` do IBGE por sigilo).
    pub value: Option<f64>,
    /// Texto original (rótulo do grupo do IPVS, marcador de sigilo...).
    pub value_text: Option<String>,
    pub source_variable: String,
}

/// Equipamento urbano.
#[derive(Debug, Clone, PartialEq)]
pub struct UrbanService {
    pub external_id: String,
    pub name: Option<String>,
    pub category: ServiceCategory,
    pub subcategory: Option<String>,
    pub address: Option<String>,
    pub municipality_ibge_code: String,
    pub geometry: RawGeometry,
    /// Atributos originais da fonte, sem interpretação.
    pub attributes: serde_json::Value,
}

/// Uma tipologia de risco como a fonte descreve (código COBRADE + rótulos).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawRiskTypology {
    pub cobrade: Option<String>,
    pub general: Option<String>,
    pub specific: Option<String>,
}

/// Área de risco como veio da fonte.
#[derive(Debug, Clone, PartialEq)]
pub struct RawRiskArea {
    pub external_id: String,
    pub municipality_ibge_code: Option<String>,
    pub location_name: Option<String>,
    pub typologies: Vec<RawRiskTypology>,
    /// Grau de risco exatamente como a fonte escreve ("Alto", "r3"...).
    pub severity: Option<String>,
    pub mapped_on: Option<NaiveDate>,
    pub geometry: Option<RawGeometry>,
    pub attributes: serde_json::Value,
}

/// Área de risco validada.
#[derive(Debug, Clone, PartialEq)]
pub struct EnvironmentalRiskArea {
    pub external_id: String,
    pub municipality_ibge_code: String,
    pub location_name: Option<String>,
    pub risk_types: Vec<RiskType>,
    /// Rótulos originais das tipologias, na ordem da fonte.
    pub source_labels: Vec<String>,
    pub severity: Option<String>,
    pub mapped_on: Option<NaiveDate>,
    pub geometry: RawGeometry,
    pub attributes: serde_json::Value,
}

/// Resultado de um provider: o dataset, os municípios que ele cobre e os
/// registros. `coverage` é o recorte efetivamente importado; um município
/// coberto com zero registros é informação ("a fonte mapeou e não há
/// nada"), diferente de um município fora da cobertura.
#[derive(Debug, Clone)]
pub struct DatasetBatch<T> {
    pub provenance: DatasetProvenance,
    pub coverage: Vec<String>,
    pub records: Vec<T>,
    pub report: ImportReport,
}

/// Relatório de leitura/normalização, gravado junto com a importação.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ImportReport {
    pub read: usize,
    pub skipped: usize,
    /// Problemas encontrados (no máximo [`ImportReport::MAX_ISSUES`]).
    pub issues: Vec<String>,
}

impl ImportReport {
    pub const MAX_ISSUES: usize = 50;

    pub fn skip(&mut self, issue: impl Into<String>) {
        self.skipped += 1;
        self.note(issue);
    }

    pub fn note(&mut self, issue: impl Into<String>) {
        if self.issues.len() < Self::MAX_ISSUES {
            self.issues.push(issue.into());
        }
    }
}

/// Providers regionais conhecidos, com cobertura e capacidades. Um provider
/// novo (ex.: prefeitura de São Bernardo) entra nesta lista e ganha uma
/// implementação no crawler; domínio e API não mudam.
pub const REGIONAL_PROVIDERS: &[ProviderDescriptor] = &[
    ProviderDescriptor {
        source: DataSource::IbgeCensoSetores,
        coverage: Coverage::Country,
        capabilities: &[Capability::CensusSectors, Capability::Demographics],
        service_categories: &[],
        refresh: RefreshPolicy {
            cadence: "decennial",
            interval_days: 3650,
        },
    },
    ProviderDescriptor {
        source: DataSource::SeadeIpvs,
        coverage: Coverage::States(&["35"]),
        capabilities: &[Capability::SocialVulnerability],
        service_categories: &[],
        refresh: RefreshPolicy {
            cadence: "decennial",
            interval_days: 3650,
        },
    },
    ProviderDescriptor {
        source: DataSource::GeoSampa,
        coverage: Coverage::Municipalities(&["3550308"]),
        capabilities: &[Capability::UrbanServices],
        service_categories: &[
            ServiceCategory::Health,
            ServiceCategory::Education,
            ServiceCategory::Culture,
            ServiceCategory::Sport,
        ],
        refresh: RefreshPolicy {
            cadence: "monthly",
            interval_days: 30,
        },
    },
    ProviderDescriptor {
        source: DataSource::SgbRiskSectors,
        coverage: Coverage::MappedMunicipalities,
        capabilities: &[Capability::EnvironmentalRisk],
        service_categories: &[],
        refresh: RefreshPolicy {
            cadence: "on_demand",
            interval_days: 365,
        },
    },
];

/// Descritor de um provider regional.
pub fn provider_descriptor(source: DataSource) -> Option<&'static ProviderDescriptor> {
    REGIONAL_PROVIDERS.iter().find(|p| p.source == source)
}

/// Código IBGE do município a partir do código do setor (7 primeiros dígitos).
pub fn municipality_of_sector(sector_code: &str) -> Option<&str> {
    sector_code.get(..7)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enums_round_trip_through_serde() {
        for c in ServiceCategory::ALL {
            assert_eq!(ServiceCategory::parse(c.as_str()), Some(c));
            assert_eq!(
                serde_json::to_string(&c).unwrap(),
                format!("\"{}\"", c.as_str())
            );
        }
        for t in RiskType::ALL {
            assert_eq!(RiskType::parse(&t.as_str().to_lowercase()), Some(t));
            assert_eq!(
                serde_json::to_string(&t).unwrap(),
                format!("\"{}\"", t.as_str())
            );
        }
        for i in SectorIndicator::ALL {
            assert_eq!(SectorIndicator::parse(i.as_str()), Some(i));
        }
    }

    #[test]
    fn coverage_by_state_and_municipality() {
        assert_eq!(Coverage::States(&["35"]).covers("3548708"), Some(true));
        assert_eq!(Coverage::States(&["35"]).covers("3304557"), Some(false));
        assert_eq!(
            Coverage::Municipalities(&["3550308"]).covers("3548708"),
            Some(false)
        );
        assert_eq!(Coverage::MappedMunicipalities.covers("3548708"), None);
    }

    #[test]
    fn report_caps_issues() {
        let mut r = ImportReport::default();
        for i in 0..100 {
            r.skip(format!("linha {i}"));
        }
        assert_eq!(r.skipped, 100);
        assert_eq!(r.issues.len(), ImportReport::MAX_ISSUES);
    }
}

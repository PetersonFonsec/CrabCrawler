//! Segurança pública: estatísticas criminais oficiais associadas a regiões.
//!
//! Princípios do módulo:
//! - todo número carrega a procedência (fonte, URL, versão do dataset, data
//!   da coleta) e o rótulo original da fonte;
//! - um dado municipal nunca vira dado de bairro: a região guarda o nível
//!   geográfico real ([`RegionLevel`]);
//! - ocorrências e vítimas são unidades diferentes e nunca são somadas;
//! - não existe "score": só indicadores reais e comparações.

use chrono::{DateTime, Datelike, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use crate::{DataSource, GeoPoint, Region};

/// Tipos de crime normalizados entre fontes.
///
/// Os nomes são estáveis (persistidos e expostos na API). Cada fonte tem seu
/// próprio mapeamento "natureza/evento → tipo" no normalizador.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrimeType {
    Homicide,
    AttemptedHomicide,
    Femicide,
    RobberyFollowedByDeath,
    Rape,
    RapeOfVulnerable,
    Robbery,
    Theft,
    VehicleRobbery,
    VehicleTheft,
    CargoRobbery,
    FinancialInstitutionRobbery,
    BodilyInjury,
}

impl CrimeType {
    pub const ALL: [CrimeType; 13] = [
        Self::Homicide,
        Self::AttemptedHomicide,
        Self::Femicide,
        Self::RobberyFollowedByDeath,
        Self::Rape,
        Self::RapeOfVulnerable,
        Self::Robbery,
        Self::Theft,
        Self::VehicleRobbery,
        Self::VehicleTheft,
        Self::CargoRobbery,
        Self::FinancialInstitutionRobbery,
        Self::BodilyInjury,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Homicide => "homicide",
            Self::AttemptedHomicide => "attempted_homicide",
            Self::Femicide => "femicide",
            Self::RobberyFollowedByDeath => "robbery_followed_by_death",
            Self::Rape => "rape",
            Self::RapeOfVulnerable => "rape_of_vulnerable",
            Self::Robbery => "robbery",
            Self::Theft => "theft",
            Self::VehicleRobbery => "vehicle_robbery",
            Self::VehicleTheft => "vehicle_theft",
            Self::CargoRobbery => "cargo_robbery",
            Self::FinancialInstitutionRobbery => "financial_institution_robbery",
            Self::BodilyInjury => "bodily_injury",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.as_str() == value)
    }

    /// Nome em português para a interface.
    pub fn label_pt(&self) -> &'static str {
        match self {
            Self::Homicide => "Homicídio doloso",
            Self::AttemptedHomicide => "Tentativa de homicídio",
            Self::Femicide => "Feminicídio",
            Self::RobberyFollowedByDeath => "Latrocínio",
            Self::Rape => "Estupro",
            Self::RapeOfVulnerable => "Estupro de vulnerável",
            Self::Robbery => "Roubo (outros)",
            Self::Theft => "Furto (outros)",
            Self::VehicleRobbery => "Roubo de veículo",
            Self::VehicleTheft => "Furto de veículo",
            Self::CargoRobbery => "Roubo de carga",
            Self::FinancialInstitutionRobbery => "Roubo a instituição financeira",
            Self::BodilyInjury => "Lesão corporal dolosa",
        }
    }
}

/// O que está sendo contado. A SSP-SP (Res. 160/2001) separa ocorrências de
/// vítimas: um homicídio com duas vítimas é 1 ocorrência e 2 vítimas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CountingUnit {
    Occurrences,
    Victims,
}

impl CountingUnit {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Occurrences => "occurrences",
            Self::Victims => "victims",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "occurrences" => Some(Self::Occurrences),
            "victims" => Some(Self::Victims),
            _ => None,
        }
    }
}

/// Granularidade temporal de um registro.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PeriodGranularity {
    Month,
    Year,
}

impl PeriodGranularity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Month => "month",
            Self::Year => "year",
        }
    }
}

/// Intervalo fechado de datas ao qual um número se refere.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Period {
    pub start: NaiveDate,
    pub end: NaiveDate,
    pub granularity: PeriodGranularity,
}

impl Period {
    pub fn month(year: i32, month: u32) -> Option<Self> {
        let start = NaiveDate::from_ymd_opt(year, month, 1)?;
        let next = if month == 12 {
            NaiveDate::from_ymd_opt(year + 1, 1, 1)?
        } else {
            NaiveDate::from_ymd_opt(year, month + 1, 1)?
        };
        Some(Self {
            start,
            end: next.pred_opt()?,
            granularity: PeriodGranularity::Month,
        })
    }

    pub fn year(year: i32) -> Option<Self> {
        Some(Self {
            start: NaiveDate::from_ymd_opt(year, 1, 1)?,
            end: NaiveDate::from_ymd_opt(year, 12, 31)?,
            granularity: PeriodGranularity::Year,
        })
    }

    pub fn year_of_start(&self) -> i32 {
        self.start.year()
    }
}

/// Procedência de uma estatística de segurança.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SecurityProvenance {
    pub source: DataSource,
    /// URL ou caminho exato do arquivo lido.
    pub source_url: Option<String>,
    /// Versão do dataset (ex.: nome do arquivo, data de publicação).
    pub dataset_version: Option<String>,
    pub collected_at: DateTime<Utc>,
}

/// Registro como veio da fonte, antes da normalização.
///
/// O provider só lê e estrutura o arquivo; decidir o tipo de crime, a
/// unidade de contagem e a região é papel do normalizador.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawCrimeRecord {
    pub source: DataSource,
    /// UF (sigla), ex.: "SP".
    pub state: String,
    pub municipality_name: Option<String>,
    pub municipality_ibge_code: Option<String>,
    /// Delegacia/unidade policial, quando o dado vem nesse nível.
    pub police_unit: Option<String>,
    pub year: i32,
    pub month: u32,
    /// Natureza/evento exatamente como na fonte.
    pub label: String,
    pub value: f64,
    /// Unidade informada explicitamente pela fonte (coluna própria). Quando
    /// ausente, o normalizador deduz pelo rótulo.
    pub counting_unit: Option<CountingUnit>,
    pub source_url: Option<String>,
    pub dataset_version: Option<String>,
    pub collected_at: DateTime<Utc>,
}

/// Contagem oficial de um tipo de crime numa região e período.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CrimeStatistic {
    pub region: Region,
    pub crime_type: CrimeType,
    pub counting_unit: CountingUnit,
    pub count: u64,
    pub period: Period,
    /// Natureza/evento exatamente como veio da fonte.
    pub source_label: String,
    pub provenance: SecurityProvenance,
}

/// Ocorrência individual georreferenciada (microdados de boletins).
///
/// Ainda sem importador: serve para, quando houver dataset com coordenadas
/// validadas, responder "quantas ocorrências num raio de X m do imóvel".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CrimeOccurrence {
    pub external_id: String,
    pub crime_type: CrimeType,
    pub source_label: String,
    pub occurred_on: NaiveDate,
    pub point: Option<GeoPoint>,
    pub municipality_ibge_code: Option<String>,
    /// Bairro como texto livre da fonte; não é chave de junção.
    pub neighborhood: Option<String>,
    pub police_unit: Option<String>,
    pub provenance: SecurityProvenance,
}

/// Raios suportados para análise de proximidade (quando houver dados).
pub const PROXIMITY_RADII_M: [u32; 4] = [500, 1_000, 2_000, 5_000];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crime_type_round_trips() {
        for t in CrimeType::ALL {
            assert_eq!(CrimeType::parse(t.as_str()), Some(t));
            let json = serde_json::to_string(&t).unwrap();
            assert_eq!(json, format!("\"{}\"", t.as_str()));
        }
    }

    #[test]
    fn month_period_handles_leap_year_and_december() {
        let feb = Period::month(2024, 2).unwrap();
        assert_eq!(feb.end, NaiveDate::from_ymd_opt(2024, 2, 29).unwrap());
        let dec = Period::month(2025, 12).unwrap();
        assert_eq!(dec.end, NaiveDate::from_ymd_opt(2025, 12, 31).unwrap());
        assert!(Period::month(2025, 13).is_none());
    }
}

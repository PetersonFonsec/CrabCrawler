use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::{Provenance, Region};

/// Tipos de indicador regional genéricos. Estatísticas criminais têm modelo
/// próprio em [`crate::security`], com rótulo original e unidade de contagem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndicatorKind {
    Population,
    MedianPricePerM2,
}

impl IndicatorKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Population => "population",
            Self::MedianPricePerM2 => "median_price_per_m2",
        }
    }
}

/// Um valor medido num período.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Indicator {
    pub kind: IndicatorKind,
    pub value: f64,
    pub period_start: NaiveDate,
    pub period_end: NaiveDate,
}

/// Indicador associado a uma região, com procedência.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegionIndicator {
    pub region: Region,
    pub indicator: Indicator,
    pub provenance: Provenance,
}

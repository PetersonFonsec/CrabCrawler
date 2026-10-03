//! Normalização e agregação de estatísticas de segurança pública.
//!
//! - [`normalize_crime_records`]: rótulo da fonte → [`CrimeType`] + unidade,
//!   nome/código → região, validação de valores. Nada é inventado: rótulos
//!   sem mapeamento e municípios não resolvidos são contados no relatório.
//! - [`annual_indicators`]: soma mensal → anual, junta população e calcula
//!   taxa por 100 mil, mantendo valor absoluto, população usada e fonte
//!   separados.
//! - [`trend`]: variação entre anos completos, sem score nem classificação.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use chrono::{DateTime, Datelike, NaiveDate, Utc};
use crab_domain::{
    CountingUnit, CrimeStatistic, CrimeType, DataSource, Period, RawCrimeRecord, Region,
    RegionLevel, SecurityProvenance,
};
use serde::Serialize;

use crate::{slugify, Gazetteer};

/// Classifica um rótulo de uma fonte. Retorna `None` para rótulos que não
/// entram no modelo (totais que somam outras linhas, crimes de trânsito,
/// produtividade policial...).
pub fn classify(source: DataSource, label: &str) -> Option<(CrimeType, Option<CountingUnit>)> {
    let key = label_key(label);
    match source {
        DataSource::SspSp => classify_ssp(&key),
        DataSource::SinespVde => classify_sinesp(&key).map(|t| (t, None)),
        _ => None,
    }
}

/// Slug sem marcadores de nota de rodapé: `"HOMICÍDIO DOLOSO (2)"` →
/// `"homicidio-doloso"`.
fn label_key(label: &str) -> String {
    let slug = slugify(label);
    let mut parts: Vec<&str> = slug.split('-').collect();
    while parts
        .last()
        .is_some_and(|p| p.chars().all(|c| c.is_ascii_digit()))
    {
        parts.pop();
    }
    parts.join("-")
}

/// Naturezas da tabela mensal da SSP-SP (Res. 160/2001).
fn classify_ssp(key: &str) -> Option<(CrimeType, Option<CountingUnit>)> {
    use CountingUnit::{Occurrences as O, Victims as V};
    use CrimeType::*;
    let (t, unit) = match key {
        "homicidio-doloso" => (Homicide, O),
        "n-de-vitimas-em-homicidio-doloso" => (Homicide, V),
        "tentativa-de-homicidio" => (AttemptedHomicide, O),
        "feminicidio" => (Femicide, O),
        "latrocinio" => (RobberyFollowedByDeath, O),
        "n-de-vitimas-em-latrocinio" => (RobberyFollowedByDeath, V),
        "lesao-corporal-dolosa" => (BodilyInjury, O),
        "estupro" => (Rape, O),
        "estupro-de-vulneravel" => (RapeOfVulnerable, O),
        "roubo-outros" => (Robbery, O),
        "roubo-de-veiculo" => (VehicleRobbery, O),
        "roubo-a-banco" => (FinancialInstitutionRobbery, O),
        "roubo-de-carga" => (CargoRobbery, O),
        "furto-outros" => (Theft, O),
        "furto-de-veiculo" => (VehicleTheft, O),
        // "TOTAL DE ESTUPRO", "TOTAL DE ROUBO - OUTROS" e afins somam outras
        // linhas; importá-los duplicaria contagens.
        _ => return None,
    };
    Some((t, Some(unit)))
}

/// Eventos do Sinesp VDE.
fn classify_sinesp(key: &str) -> Option<CrimeType> {
    use CrimeType::*;
    Some(match key {
        "homicidio-doloso" => Homicide,
        "feminicidio" => Femicide,
        "roubo-seguido-de-morte-latrocinio" => RobberyFollowedByDeath,
        "tentativa-de-homicidio" => AttemptedHomicide,
        "estupro" => Rape,
        "roubo-de-veiculo" => VehicleRobbery,
        "furto-de-veiculo" => VehicleTheft,
        "roubo-de-carga" => CargoRobbery,
        "roubo-a-instituicao-financeira" => FinancialInstitutionRobbery,
        _ => return None,
    })
}

/// O que aconteceu na normalização, para log e para o registro da importação.
#[derive(Debug, Default, Clone, PartialEq, Serialize)]
pub struct NormalizationReport {
    pub input: usize,
    pub normalized: usize,
    pub merged_duplicates: usize,
    pub filtered_out: usize,
    pub invalid_values: usize,
    pub unmapped_labels: BTreeMap<String, usize>,
    pub unresolved_municipalities: BTreeMap<String, usize>,
}

impl NormalizationReport {
    pub fn skipped(&self) -> usize {
        self.invalid_values
            + self.unmapped_labels.values().sum::<usize>()
            + self.unresolved_municipalities.values().sum::<usize>()
    }
}

/// Converte registros brutos em estatísticas do domínio.
///
/// `municipality_filter` restringe a um código IBGE (útil para fontes que
/// só trazem o nome do município). Registros idênticos (mesma região, tipo,
/// unidade, período e rótulo) são somados.
pub fn normalize_crime_records(
    raw: Vec<RawCrimeRecord>,
    gazetteer: &Gazetteer,
    municipality_filter: Option<&str>,
) -> (Vec<CrimeStatistic>, NormalizationReport) {
    let mut report = NormalizationReport {
        input: raw.len(),
        ..Default::default()
    };
    let mut merged: Vec<CrimeStatistic> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();

    for record in raw {
        let Some((crime_type, label_unit)) = classify(record.source, &record.label) else {
            *report.unmapped_labels.entry(record.label).or_default() += 1;
            continue;
        };
        let Some(unit) = record.counting_unit.or(label_unit) else {
            *report.unmapped_labels.entry(record.label).or_default() += 1;
            continue;
        };
        let ibge_code = record.municipality_ibge_code.clone().or_else(|| {
            record
                .municipality_name
                .as_deref()
                .and_then(|name| gazetteer.lookup(&record.state, name))
                .map(str::to_string)
        });
        let Some(ibge_code) = ibge_code else {
            let name = record.municipality_name.unwrap_or_default();
            *report.unresolved_municipalities.entry(name).or_default() += 1;
            continue;
        };
        if municipality_filter.is_some_and(|f| f != ibge_code) {
            report.filtered_out += 1;
            continue;
        }
        let Some(count) = as_count(record.value) else {
            report.invalid_values += 1;
            continue;
        };
        let Some(period) = Period::month(record.year, record.month) else {
            report.invalid_values += 1;
            continue;
        };

        let region = match &record.police_unit {
            Some(unit_name) => Region {
                level: RegionLevel::PoliceDistrict,
                code: slugify(unit_name),
                name: unit_name.clone(),
                municipality_ibge_code: ibge_code.clone(),
            },
            None => Region {
                level: RegionLevel::Municipality,
                code: ibge_code.clone(),
                name: record
                    .municipality_name
                    .unwrap_or_else(|| ibge_code.clone()),
                municipality_ibge_code: ibge_code.clone(),
            },
        };

        let key = format!(
            "{}|{}|{}|{}|{}|{}|{}|{}",
            region.level.as_str(),
            region.code,
            region.municipality_ibge_code,
            crime_type.as_str(),
            unit.as_str(),
            period.start,
            record.source.as_str(),
            record.label
        );
        if let Some(&i) = index.get(&key) {
            merged[i].count += count;
            report.merged_duplicates += 1;
            continue;
        }
        index.insert(key, merged.len());
        merged.push(CrimeStatistic {
            region,
            crime_type,
            counting_unit: unit,
            count,
            period,
            source_label: record.label,
            provenance: SecurityProvenance {
                source: record.source,
                source_url: record.source_url,
                dataset_version: record.dataset_version,
                collected_at: record.collected_at,
            },
        });
    }
    report.normalized = merged.len();
    (merged, report)
}

fn as_count(value: f64) -> Option<u64> {
    (value.is_finite() && value >= 0.0 && value.fract() == 0.0).then_some(value as u64)
}

/// `ocorrências / população × 100.000`.
pub fn rate_per_100k(count: u64, population: f64) -> Option<f64> {
    (population > 0.0).then(|| count as f64 / population * 100_000.0)
}

/// População usada para normalizar, com a própria procedência.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PopulationRef {
    pub value: f64,
    /// Ano de referência da população (ex.: 2022 para o Censo).
    pub year: i32,
    pub source: DataSource,
    pub source_url: Option<String>,
}

/// Indicador anual de uma região, pronto para a API.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AnnualIndicator {
    pub crime_type: CrimeType,
    pub label: &'static str,
    pub counting_unit: CountingUnit,
    pub year: i32,
    pub period_start: NaiveDate,
    pub period_end: NaiveDate,
    /// Valor absoluto (soma dos meses disponíveis).
    pub count: u64,
    pub months_covered: usize,
    /// `false` quando faltam meses no ano: compare com cuidado.
    pub complete: bool,
    /// População usada; `None` quando não há população compatível
    /// (ex.: área de delegacia).
    pub population: Option<PopulationRef>,
    pub rate_per_100k: Option<f64>,
    pub source: DataSource,
    pub source_labels: Vec<String>,
    pub source_urls: Vec<String>,
    pub dataset_versions: Vec<String>,
    pub collected_at: DateTime<Utc>,
}

/// Agrega estatísticas mensais de **uma** região por (tipo, fonte, unidade,
/// ano). Fontes diferentes nunca são somadas entre si.
///
/// `population` só deve ser informada quando a região é compatível com ela
/// (município ↔ população municipal). Usa-se a população do ano mais
/// próximo; o ano fica explícito na resposta.
pub fn annual_indicators(
    stats: &[CrimeStatistic],
    population: &[PopulationRef],
) -> Vec<AnnualIndicator> {
    type Key = (CrimeType, &'static str, CountingUnit, i32);
    struct Acc {
        source: DataSource,
        count: u64,
        months: BTreeSet<u32>,
        labels: BTreeSet<String>,
        urls: BTreeSet<String>,
        versions: BTreeSet<String>,
        collected_at: DateTime<Utc>,
    }
    let mut groups: BTreeMap<Key, Acc> = BTreeMap::new();
    for s in stats {
        let year = s.period.start.year();
        let key = (
            s.crime_type,
            s.provenance.source.as_str(),
            s.counting_unit,
            year,
        );
        let acc = groups.entry(key).or_insert_with(|| Acc {
            source: s.provenance.source,
            count: 0,
            months: BTreeSet::new(),
            labels: BTreeSet::new(),
            urls: BTreeSet::new(),
            versions: BTreeSet::new(),
            collected_at: s.provenance.collected_at,
        });
        acc.count += s.count;
        acc.months.insert(s.period.start.month());
        acc.labels.insert(s.source_label.clone());
        acc.urls.extend(s.provenance.source_url.clone());
        acc.versions.extend(s.provenance.dataset_version.clone());
        acc.collected_at = acc.collected_at.max(s.provenance.collected_at);
    }

    groups
        .into_iter()
        .map(|((crime_type, _, unit, year), acc)| {
            let first = *acc.months.first().expect("grupo não vazio");
            let last = *acc.months.last().expect("grupo não vazio");
            let start = Period::month(year, first).expect("mês válido").start;
            let end = Period::month(year, last).expect("mês válido").end;
            let pop = nearest_population(population, year);
            AnnualIndicator {
                crime_type,
                label: crime_type.label_pt(),
                counting_unit: unit,
                year,
                period_start: start,
                period_end: end,
                count: acc.count,
                months_covered: acc.months.len(),
                complete: acc.months.len() == 12,
                rate_per_100k: pop
                    .as_ref()
                    .and_then(|p| rate_per_100k(acc.count, p.value))
                    .map(round2),
                population: pop,
                source: acc.source,
                source_labels: acc.labels.into_iter().collect(),
                source_urls: acc.urls.into_iter().collect(),
                dataset_versions: acc.versions.into_iter().collect(),
                collected_at: acc.collected_at,
            }
        })
        .collect()
}

fn nearest_population(population: &[PopulationRef], year: i32) -> Option<PopulationRef> {
    population
        .iter()
        .min_by_key(|p| ((p.year - year).abs(), -p.year))
        .cloned()
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// Ano mais recente com 12 meses para alguma série; senão o mais recente.
pub fn default_year(indicators: &[AnnualIndicator]) -> Option<i32> {
    indicators
        .iter()
        .filter(|i| i.complete)
        .map(|i| i.year)
        .max()
        .or_else(|| indicators.iter().map(|i| i.year).max())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TrendDirection {
    Decrease,
    Increase,
    Stable,
}

/// Variação de uma série entre anos completos. É uma descrição factual dos
/// registros, não uma avaliação de segurança.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Trend {
    pub from_year: i32,
    pub to_year: i32,
    /// Compara taxas quando todas existem; senão valores absolutos.
    pub metric: &'static str,
    pub from_value: f64,
    pub to_value: f64,
    pub change_pct: f64,
    pub direction: TrendDirection,
    /// `true` quando todos os passos ano a ano foram na mesma direção.
    pub consistent: bool,
}

/// Tendência nos últimos `window` anos completos de uma série (mesma fonte,
/// tipo e unidade). Variações abaixo de 1% contam como estáveis.
pub fn trend(series: &[AnnualIndicator], window: usize) -> Option<Trend> {
    let mut complete: Vec<&AnnualIndicator> = series.iter().filter(|i| i.complete).collect();
    complete.sort_by_key(|i| i.year);
    let tail = &complete[complete.len().saturating_sub(window)..];
    if tail.len() < 2 {
        return None;
    }
    let use_rate = tail.iter().all(|i| i.rate_per_100k.is_some());
    let value = |i: &AnnualIndicator| {
        if use_rate {
            i.rate_per_100k.unwrap_or_default()
        } else {
            i.count as f64
        }
    };
    let (first, last) = (tail[0], tail[tail.len() - 1]);
    let (from, to) = (value(first), value(last));
    if from == 0.0 {
        return None;
    }
    let change_pct = round2((to - from) / from * 100.0);
    let direction = if change_pct.abs() < 1.0 {
        TrendDirection::Stable
    } else if change_pct < 0.0 {
        TrendDirection::Decrease
    } else {
        TrendDirection::Increase
    };
    let steps: Vec<f64> = tail.windows(2).map(|w| value(w[1]) - value(w[0])).collect();
    let consistent = match direction {
        TrendDirection::Decrease => steps.iter().all(|d| *d < 0.0),
        TrendDirection::Increase => steps.iter().all(|d| *d > 0.0),
        TrendDirection::Stable => false,
    };
    Some(Trend {
        from_year: first.year,
        to_year: last.year,
        metric: if use_rate { "rate_per_100k" } else { "count" },
        from_value: from,
        to_value: to,
        change_pct,
        direction,
        consistent,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(label: &str, value: f64, month: u32) -> RawCrimeRecord {
        RawCrimeRecord {
            source: DataSource::SspSp,
            state: "SP".into(),
            municipality_name: None,
            municipality_ibge_code: Some("3548708".into()),
            police_unit: None,
            year: 2025,
            month,
            label: label.into(),
            value,
            counting_unit: None,
            source_url: Some("fixtures/ssp.csv".into()),
            dataset_version: Some("ssp.csv".into()),
            collected_at: Utc::now(),
        }
    }

    #[test]
    fn classifies_ssp_labels_with_footnotes_and_units() {
        assert_eq!(
            classify(DataSource::SspSp, "HOMICÍDIO DOLOSO (2)"),
            Some((CrimeType::Homicide, Some(CountingUnit::Occurrences)))
        );
        assert_eq!(
            classify(DataSource::SspSp, "Nº DE VÍTIMAS EM HOMICÍDIO DOLOSO (3)"),
            Some((CrimeType::Homicide, Some(CountingUnit::Victims)))
        );
        assert_eq!(
            classify(DataSource::SspSp, "ROUBO DE VEÍCULO"),
            Some((CrimeType::VehicleRobbery, Some(CountingUnit::Occurrences)))
        );
        assert_eq!(classify(DataSource::SspSp, "TOTAL DE ESTUPRO (4)"), None);
        assert_eq!(
            classify(
                DataSource::SspSp,
                "HOMICÍDIO DOLOSO POR ACIDENTE DE TRÂNSITO"
            ),
            None
        );
    }

    #[test]
    fn classifies_sinesp_events() {
        assert_eq!(
            classify(DataSource::SinespVde, "Roubo seguido de morte (latrocínio)"),
            Some((CrimeType::RobberyFollowedByDeath, None))
        );
        assert_eq!(
            classify(DataSource::SinespVde, "Furto de veículo"),
            Some((CrimeType::VehicleTheft, None))
        );
        assert_eq!(classify(DataSource::SinespVde, "Suicídio"), None);
    }

    #[test]
    fn normalizes_keeps_granularity_and_reports_skips() {
        let mut district = raw("ROUBO - OUTROS", 40.0, 1);
        district.police_unit = Some("01 DP S.Bernardo do Campo".into());
        let records = vec![
            raw("ROUBO - OUTROS", 100.0, 1),
            district,
            raw("TOTAL DE ROUBO - OUTROS (1)", 140.0, 1),
            raw("FURTO - OUTROS", 2.5, 1),
        ];
        let (stats, report) = normalize_crime_records(records, &Gazetteer::mvp(), None);
        assert_eq!(stats.len(), 2);
        assert_eq!(stats[0].region.level, RegionLevel::Municipality);
        assert_eq!(stats[1].region.level, RegionLevel::PoliceDistrict);
        assert_eq!(stats[1].region.code, "01-dp-s-bernardo-do-campo");
        assert_eq!(stats[1].region.municipality_ibge_code, "3548708");
        assert_eq!(report.unmapped_labels["TOTAL DE ROUBO - OUTROS (1)"], 1);
        assert_eq!(report.invalid_values, 1);
        assert_eq!(report.skipped(), 2);
    }

    #[test]
    fn resolves_municipality_by_name_and_merges_duplicates() {
        let mut a = raw("Roubo de veículo", 10.0, 2);
        a.source = DataSource::SinespVde;
        a.municipality_ibge_code = None;
        a.municipality_name = Some("SAO BERNARDO DO CAMPO".into());
        a.counting_unit = Some(CountingUnit::Occurrences);
        let b = a.clone();
        let mut unknown = a.clone();
        unknown.municipality_name = Some("Cidade Inexistente".into());
        let (stats, report) =
            normalize_crime_records(vec![a, b, unknown], &Gazetteer::mvp(), Some("3548708"));
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].count, 20);
        assert_eq!(stats[0].region.name, "SAO BERNARDO DO CAMPO");
        assert_eq!(report.merged_duplicates, 1);
        assert_eq!(report.unresolved_municipalities["Cidade Inexistente"], 1);
    }

    #[test]
    fn rate_matches_spec_example() {
        // 132 roubos de veículo, população 810.000 → ≈ 16,3 / 100 mil.
        let rate = rate_per_100k(132, 810_000.0).unwrap();
        assert!((rate - 16.296).abs() < 0.001);
        assert_eq!(rate_per_100k(1, 0.0), None);
    }

    fn census() -> Vec<PopulationRef> {
        vec![PopulationRef {
            value: 810_000.0,
            year: 2022,
            source: DataSource::IbgeLocalidades,
            source_url: None,
        }]
    }

    #[test]
    fn annual_aggregation_keeps_absolute_population_and_rate() {
        let (stats, _) = normalize_crime_records(
            (1..=12)
                .map(|m| raw("ROUBO DE VEÍCULO", 11.0, m))
                .chain([raw("FURTO - OUTROS", 5.0, 1)])
                .collect(),
            &Gazetteer::mvp(),
            None,
        );
        let annual = annual_indicators(&stats, &census());
        let vehicle = annual
            .iter()
            .find(|i| i.crime_type == CrimeType::VehicleRobbery)
            .unwrap();
        assert_eq!(vehicle.count, 132);
        assert!(vehicle.complete);
        assert_eq!(vehicle.rate_per_100k, Some(16.3));
        assert_eq!(vehicle.population.as_ref().unwrap().year, 2022);
        assert_eq!(
            vehicle.period_end,
            NaiveDate::from_ymd_opt(2025, 12, 31).unwrap()
        );
        let theft = annual
            .iter()
            .find(|i| i.crime_type == CrimeType::Theft)
            .unwrap();
        assert!(!theft.complete);
        assert_eq!(theft.months_covered, 1);
        assert_eq!(default_year(&annual), Some(2025));
    }

    #[test]
    fn annual_without_population_has_no_rate() {
        let (stats, _) =
            normalize_crime_records(vec![raw("FURTO - OUTROS", 5.0, 1)], &Gazetteer::mvp(), None);
        let annual = annual_indicators(&stats, &[]);
        assert_eq!(annual[0].rate_per_100k, None);
        assert_eq!(annual[0].population, None);
    }

    #[test]
    fn trend_over_complete_years() {
        let mk = |year, count| AnnualIndicator {
            crime_type: CrimeType::Robbery,
            label: "",
            counting_unit: CountingUnit::Occurrences,
            year,
            period_start: NaiveDate::from_ymd_opt(year, 1, 1).unwrap(),
            period_end: NaiveDate::from_ymd_opt(year, 12, 31).unwrap(),
            count,
            months_covered: 12,
            complete: true,
            population: None,
            rate_per_100k: None,
            source: DataSource::SspSp,
            source_labels: vec![],
            source_urls: vec![],
            dataset_versions: vec![],
            collected_at: Utc::now(),
        };
        let mut partial = mk(2026, 10);
        partial.complete = false;
        let series = vec![
            mk(2022, 500),
            mk(2023, 400),
            mk(2024, 300),
            mk(2025, 200),
            partial,
        ];
        let t = trend(&series, 3).unwrap();
        assert_eq!((t.from_year, t.to_year), (2023, 2025));
        assert_eq!(t.direction, TrendDirection::Decrease);
        assert_eq!(t.change_pct, -50.0);
        assert!(t.consistent);
        assert_eq!(t.metric, "count");
        assert!(trend(&series[..1], 3).is_none());
    }
}

//! Normalização dos datasets regionais.
//!
//! Providers só leem e estruturam arquivos; aqui ficam as regras: validar o
//! código do setor, interpretar números no formato brasileiro, traduzir
//! variáveis da fonte em indicadores e tipologias COBRADE em tipos de risco.
//! Tudo que é descartado entra no [`ImportReport`].

use crab_domain::regional::{
    CensusSector, DatasetBatch, EnvironmentalRiskArea, RawCensusSector, RawRiskArea,
    RawRiskTypology, RawSectorValue, RiskType, SectorIndicator, SectorIndicatorValue,
};
use crab_domain::DataSource;

/// Código de setor do Censo 2022: 15 dígitos (UF 2 + município 5 + distrito
/// 2 + subdistrito 2 + setor 4). Aceita sufixos e separadores que aparecem
/// em algumas publicações (ex.: `355030801000001P`) desde que sobrem 15
/// dígitos.
pub fn normalize_sector_code(raw: &str) -> Option<String> {
    let digits: String = raw.chars().filter(char::is_ascii_digit).collect();
    (digits.len() == 15).then_some(digits)
}

/// Número no formato das fontes brasileiras: `1.234,5`, `2,85`, `810729`.
/// `X` (sigilo do IBGE), vazio, `-` e `..` viram `None`.
pub fn parse_br_number(raw: &str) -> Option<f64> {
    let s = raw.trim();
    if is_missing_marker(s) {
        return None;
    }
    let normalized = if s.contains(',') {
        s.replace('.', "").replace(',', ".")
    } else {
        s.to_string()
    };
    normalized.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// Vazio ou marcador de valor ausente/sigiloso usado pelas fontes.
pub fn is_missing_marker(text: &str) -> bool {
    matches!(text.trim(), "" | "X" | "x" | "-" | ".." | "..." | "NA")
}

/// Valida setores da malha: código de 15 dígitos e, se pedido, só um município.
pub fn normalize_sectors(
    batch: DatasetBatch<RawCensusSector>,
    municipality: Option<&str>,
) -> DatasetBatch<CensusSector> {
    let mut report = batch.report;
    let mut records = Vec::with_capacity(batch.records.len());
    for raw in batch.records {
        let Some(code) = normalize_sector_code(&raw.code) else {
            report.skip(format!("código de setor inválido: {:?}", raw.code));
            continue;
        };
        let municipality_ibge_code = code[..7].to_string();
        if municipality.is_some_and(|m| m != municipality_ibge_code) {
            continue;
        }
        if raw.area_km2.is_some_and(|a| a < 0.0) {
            report.note(format!("setor {code}: área negativa ignorada"));
        }
        records.push(CensusSector {
            code,
            municipality_ibge_code,
            municipality_name: raw.municipality_name.filter(|n| !n.trim().is_empty()),
            area_km2: raw.area_km2.filter(|a| *a >= 0.0),
            geometry: raw.geometry,
        });
    }
    let coverage = coverage_of(
        records.iter().map(|r| r.municipality_ibge_code.as_str()),
        municipality,
    );
    DatasetBatch {
        provenance: batch.provenance,
        coverage,
        records,
        report,
    }
}

/// Converte valores brutos por setor em indicadores. `map` diz qual
/// variável da fonte vira qual indicador; variáveis fora do mapa são
/// ignoradas sem contar como erro.
pub fn normalize_sector_values(
    batch: DatasetBatch<RawSectorValue>,
    municipality: Option<&str>,
    map: impl Fn(&str) -> Option<SectorIndicator>,
) -> DatasetBatch<SectorIndicatorValue> {
    let mut report = batch.report;
    let mut records = Vec::new();
    for raw in batch.records {
        let Some(indicator) = map(&raw.variable) else {
            continue;
        };
        let Some(code) = normalize_sector_code(&raw.sector_code) else {
            report.skip(format!("código de setor inválido: {:?}", raw.sector_code));
            continue;
        };
        if municipality.is_some_and(|m| !code.starts_with(m)) {
            continue;
        }
        let text = raw.value.trim();
        let value = match indicator {
            // O grupo do IPVS pode vir como "1" ou como "Grupo 1 - ...".
            SectorIndicator::IpvsGroup => leading_integer(text),
            _ => parse_br_number(text),
        };
        if value.is_none() && !is_missing_marker(text) && indicator != SectorIndicator::IpvsGroup {
            report.skip(format!(
                "setor {code}: valor inválido em {}: {text:?}",
                raw.variable
            ));
            continue;
        }
        if value.is_some_and(|v| v < 0.0) {
            report.skip(format!("setor {code}: valor negativo em {}", raw.variable));
            continue;
        }
        records.push(SectorIndicatorValue {
            sector_code: code,
            indicator,
            value,
            value_text: (value.is_none() || indicator == SectorIndicator::IpvsGroup)
                .then(|| text.to_string())
                .filter(|t| !t.is_empty()),
            source_variable: raw.variable,
        });
    }
    let coverage = coverage_of(records.iter().map(|r| &r.sector_code[..7]), municipality);
    DatasetBatch {
        provenance: batch.provenance,
        coverage,
        records,
        report,
    }
}

/// Tipo de risco a partir da tipologia da fonte. Usa o código COBRADE
/// quando existe; senão, o rótulo geral (como o SGB escreve).
pub fn classify_risk(typology: &RawRiskTypology) -> Option<RiskType> {
    if let Some(code) = typology
        .cobrade
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())
    {
        let by_code = if code.starts_with("1.2.1") {
            Some(RiskType::Flood)
        } else if code.starts_with("1.2.2") {
            Some(RiskType::FlashFlood)
        } else if code.starts_with("1.2.3") {
            Some(RiskType::UrbanFlooding)
        } else if code.starts_with("1.1.3.2") {
            Some(RiskType::Landslide)
        } else if code.starts_with("1.1.3") {
            Some(RiskType::Geological)
        } else if code.starts_with("1.1.4") {
            Some(RiskType::Erosion)
        } else {
            None
        };
        if by_code.is_some() {
            return by_code;
        }
    }
    let label = crate::slugify(typology.general.as_deref()?);
    if label.is_empty() {
        return None;
    }
    Some(match label.as_str() {
        "inundacao" => RiskType::Flood,
        "enxurrada" => RiskType::FlashFlood,
        "alagamento" => RiskType::UrbanFlooding,
        "deslizamento" => RiskType::Landslide,
        "erosao" => RiskType::Erosion,
        "rastejo" | "queda" | "rolamento" | "corrida-de-massa" | "subsidencia" | "colapso" => {
            RiskType::Geological
        }
        l if l.starts_with("queda") || l.starts_with("movimento") => RiskType::Geological,
        _ => RiskType::Other,
    })
}

/// Valida áreas de risco: precisa de geometria, município e ao menos uma
/// tipologia. Tipos repetidos no mesmo setor são unificados.
pub fn normalize_risk_areas(
    batch: DatasetBatch<RawRiskArea>,
    municipality: Option<&str>,
) -> DatasetBatch<EnvironmentalRiskArea> {
    let mut report = batch.report;
    let mut records = Vec::new();
    for raw in batch.records {
        let Some(geometry) = raw.geometry else {
            report.skip(format!("{}: sem geometria", raw.external_id));
            continue;
        };
        let Some(code) = raw
            .municipality_ibge_code
            .as_deref()
            .map(str::trim)
            .filter(|c| c.len() == 7 && c.chars().all(|ch| ch.is_ascii_digit()))
            .map(str::to_string)
        else {
            report.skip(format!(
                "{}: código IBGE do município ausente",
                raw.external_id
            ));
            continue;
        };
        if municipality.is_some_and(|m| m != code) {
            continue;
        }
        let mut risk_types = Vec::new();
        let mut source_labels = Vec::new();
        for t in &raw.typologies {
            let Some(kind) = classify_risk(t) else {
                continue;
            };
            if !risk_types.contains(&kind) {
                risk_types.push(kind);
            }
            let label = [t.general.as_deref(), t.specific.as_deref()]
                .into_iter()
                .flatten()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" / ");
            if !label.is_empty() {
                source_labels.push(label);
            }
        }
        if risk_types.is_empty() {
            report.skip(format!("{}: nenhuma tipologia de risco", raw.external_id));
            continue;
        }
        records.push(EnvironmentalRiskArea {
            external_id: raw.external_id,
            municipality_ibge_code: code,
            location_name: raw.location_name.filter(|s| !s.trim().is_empty()),
            risk_types,
            source_labels,
            severity: raw.severity.filter(|s| !s.trim().is_empty()),
            mapped_on: raw.mapped_on,
            geometry,
            attributes: raw.attributes,
        });
    }
    // Para fontes filtradas por município, o município pedido está coberto
    // mesmo sem nenhuma área: a fonte foi consultada e não mapeou nada.
    let coverage = coverage_of(
        records.iter().map(|r| r.municipality_ibge_code.as_str()),
        municipality,
    );
    DatasetBatch {
        provenance: batch.provenance,
        coverage,
        records,
        report,
    }
}

/// Origem esperada de cada indicador (para dizer "não importado").
pub fn indicator_source(indicator: SectorIndicator) -> DataSource {
    match indicator {
        SectorIndicator::IpvsGroup => DataSource::SeadeIpvs,
        _ => DataSource::IbgeCensoSetores,
    }
}

fn leading_integer(text: &str) -> Option<f64> {
    let digits: String = text
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse::<u32>().ok().map(f64::from)
}

fn coverage_of<'a>(codes: impl Iterator<Item = &'a str>, requested: Option<&str>) -> Vec<String> {
    let mut out: Vec<String> = codes.map(str::to_string).collect();
    if let Some(m) = requested {
        out.push(m.to_string());
    }
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use crab_domain::regional::{
        DatasetProvenance, GeographicGranularity, ImportReport, RawGeometry,
    };

    fn provenance() -> DatasetProvenance {
        DatasetProvenance {
            source: DataSource::IbgeCensoSetores,
            dataset_name: "teste".into(),
            dataset_version: "v1".into(),
            source_url: None,
            reference_date: None,
            collected_at: Utc::now(),
            granularity: GeographicGranularity::CensusSector,
        }
    }

    fn batch<T>(records: Vec<T>) -> DatasetBatch<T> {
        DatasetBatch {
            provenance: provenance(),
            coverage: vec![],
            records,
            report: ImportReport::default(),
        }
    }

    fn geom() -> RawGeometry {
        RawGeometry::GeoJson {
            json: r#"{"type":"Point","coordinates":[0,0]}"#.into(),
            srid: 4326,
        }
    }

    #[test]
    fn sector_code_accepts_suffix_and_rejects_wrong_length() {
        assert_eq!(
            normalize_sector_code("354870805000012P").as_deref(),
            Some("354870805000012")
        );
        assert_eq!(
            normalize_sector_code(" 354870805000012 ").as_deref(),
            Some("354870805000012")
        );
        assert_eq!(normalize_sector_code("3548708050000"), None);
        assert_eq!(normalize_sector_code(""), None);
    }

    #[test]
    fn parses_brazilian_numbers_and_suppressed_values() {
        assert_eq!(parse_br_number("810729"), Some(810729.0));
        assert_eq!(parse_br_number("2,85"), Some(2.85));
        assert_eq!(parse_br_number("1.234,5"), Some(1234.5));
        assert_eq!(parse_br_number("0.0123"), Some(0.0123));
        assert_eq!(parse_br_number("X"), None);
        assert_eq!(parse_br_number(""), None);
        assert_eq!(parse_br_number("abc"), None);
    }

    #[test]
    fn sector_values_keep_suppressed_and_drop_invalid() {
        let raw = |code: &str, var: &str, value: &str| RawSectorValue {
            sector_code: code.into(),
            variable: var.into(),
            value: value.into(),
        };
        let out = normalize_sector_values(
            batch(vec![
                raw("354870805000012", "V0001", "512"),
                raw("354870805000012", "V0005", "2,9"),
                raw("354870805000013", "V0001", "X"),
                raw("354870805000014", "V0001", "muitos"),
                raw("354870805000015", "V0001", "-3"),
                raw("1234", "V0001", "10"),
                raw("354870805000012", "V0006", "1,2"),
                raw("355030801000001", "V0001", "900"),
            ]),
            Some("3548708"),
            SectorIndicator::from_ibge_basic,
        );
        assert_eq!(out.records.len(), 3);
        let suppressed = out
            .records
            .iter()
            .find(|r| r.sector_code == "354870805000013")
            .unwrap();
        assert_eq!(suppressed.value, None);
        assert_eq!(suppressed.value_text.as_deref(), Some("X"));
        let avg = out
            .records
            .iter()
            .find(|r| r.indicator == SectorIndicator::AvgResidentsPerHousehold)
            .unwrap();
        assert_eq!(avg.value, Some(2.9));
        assert_eq!(out.report.skipped, 3);
        assert_eq!(out.coverage, vec!["3548708".to_string()]);
    }

    #[test]
    fn ipvs_group_keeps_original_label() {
        let out = normalize_sector_values(
            batch(vec![RawSectorValue {
                sector_code: "354870805000012".into(),
                variable: "ipvs".into(),
                value: "Grupo 5 - Vulnerabilidade alta".into(),
            }]),
            None,
            |_| Some(SectorIndicator::IpvsGroup),
        );
        let r = &out.records[0];
        assert_eq!(r.value, Some(5.0));
        assert_eq!(
            r.value_text.as_deref(),
            Some("Grupo 5 - Vulnerabilidade alta")
        );
    }

    #[test]
    fn sectors_are_filtered_by_municipality() {
        let raw = |code: &str| RawCensusSector {
            code: code.into(),
            municipality_name: Some("X".into()),
            area_km2: Some(0.1),
            geometry: geom(),
        };
        let out = normalize_sectors(
            batch(vec![
                raw("354870805000012"),
                raw("355030801000001"),
                raw("abc"),
            ]),
            Some("3548708"),
        );
        assert_eq!(out.records.len(), 1);
        assert_eq!(out.records[0].municipality_ibge_code, "3548708");
        assert_eq!(out.report.skipped, 1);
    }

    #[test]
    fn classifies_cobrade_codes_and_labels() {
        let t = |cobrade: Option<&str>, general: &str| RawRiskTypology {
            cobrade: cobrade.map(str::to_string),
            general: Some(general.into()),
            specific: None,
        };
        assert_eq!(
            classify_risk(&t(Some("1.2.2.0.0"), "Enxurrada")),
            Some(RiskType::FlashFlood)
        );
        assert_eq!(
            classify_risk(&t(Some("1.1.3.2.1"), "Deslizamento")),
            Some(RiskType::Landslide)
        );
        assert_eq!(
            classify_risk(&t(Some("1.1.4.2.0"), "Erosão")),
            Some(RiskType::Erosion)
        );
        assert_eq!(
            classify_risk(&t(Some("1.2.1.0.0"), "Inundação")),
            Some(RiskType::Flood)
        );
        assert_eq!(
            classify_risk(&t(None, "Rastejo")),
            Some(RiskType::Geological)
        );
        assert_eq!(
            classify_risk(&t(None, "Alagamento")),
            Some(RiskType::UrbanFlooding)
        );
        assert_eq!(classify_risk(&t(None, "Algo novo")), Some(RiskType::Other));
        assert_eq!(classify_risk(&t(None, " ")), None);
    }

    #[test]
    fn risk_areas_require_geometry_and_typology() {
        let area = |id: &str, geometry: Option<RawGeometry>, typologies: Vec<RawRiskTypology>| {
            RawRiskArea {
                external_id: id.into(),
                municipality_ibge_code: Some("3548708".into()),
                location_name: Some("Vila".into()),
                typologies,
                severity: Some("Alto".into()),
                mapped_on: None,
                geometry,
                attributes: serde_json::json!({}),
            }
        };
        let typ = |g: &str, c: &str| RawRiskTypology {
            cobrade: Some(c.into()),
            general: Some(g.into()),
            specific: None,
        };
        let out = normalize_risk_areas(
            batch(vec![
                area(
                    "a",
                    Some(geom()),
                    vec![
                        typ("Deslizamento", "1.1.3.2.1"),
                        typ("Deslizamento", "1.1.3.2.2"),
                        typ("Enxurrada", "1.2.2.0.0"),
                    ],
                ),
                area("b", None, vec![typ("Enxurrada", "1.2.2.0.0")]),
                area("c", Some(geom()), vec![]),
            ]),
            Some("3548708"),
        );
        assert_eq!(out.records.len(), 1);
        assert_eq!(
            out.records[0].risk_types,
            vec![RiskType::Landslide, RiskType::FlashFlood]
        );
        assert_eq!(out.records[0].severity.as_deref(), Some("Alto"));
        assert_eq!(out.report.skipped, 2);
    }

    #[test]
    fn requested_municipality_is_covered_even_without_records() {
        let out = normalize_risk_areas(batch(vec![]), Some("3548708"));
        assert!(out.records.is_empty());
        assert_eq!(out.coverage, vec!["3548708".to_string()]);
    }
}

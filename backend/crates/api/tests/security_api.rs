//! Fluxo completo contra um PostgreSQL/PostGIS real:
//! fixtures → normalização → persistência → API.
//!
//! Precisa de `TEST_DATABASE_URL` apontando para um banco descartável (as
//! migrations são aplicadas e dados de teste são gravados). Sem a variável o
//! teste é pulado.

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use chrono::{NaiveDate, Utc};
use crab_crawler::security::sinesp::{SinespVdeProvider, VdeInput};
use crab_crawler::security::ssp_sp::SspSpCsvProvider;
use crab_crawler::{SecurityDataProvider, SecurityScope};
use crab_domain::{
    DataSource, Indicator, IndicatorKind, Provenance, Region, RegionIndicator, RegionLevel,
};
use crab_persistence::{DatasetImport, IndicatorRepository, SecurityRepository};
use crab_processing::security::normalize_crime_records;
use crab_processing::Gazetteer;
use serde_json::Value;
use tower::ServiceExt;

fn fixture(name: &str) -> String {
    format!("{}/../../fixtures/{name}", env!("CARGO_MANIFEST_DIR"))
}

async fn setup() -> Option<sqlx::PgPool> {
    let Ok(url) = std::env::var("TEST_DATABASE_URL") else {
        eprintln!("TEST_DATABASE_URL não definida; teste de integração pulado");
        return None;
    };
    let pool = crab_persistence::connect(&url).await.expect("conexão");
    crab_persistence::migrate(&pool).await.expect("migrations");

    let security = SecurityRepository::new(pool.clone());
    let ssp = SspSpCsvProvider::new(fixture("ssp_sp_sample.csv"));
    let raw = ssp.fetch(&SecurityScope::state("SP")).await.unwrap();
    let (stats, _) = normalize_crime_records(raw, &Gazetteer::mvp(), None);
    security.upsert_statistics(&stats).await.unwrap();

    let sinesp = SinespVdeProvider::new(VdeInput::File(fixture("sinesp_vde_sample.xlsx").into()));
    let raw = sinesp.fetch(&SecurityScope::state("SP")).await.unwrap();
    let (stats, _) = normalize_crime_records(raw, &Gazetteer::mvp(), None);
    security.upsert_statistics(&stats).await.unwrap();

    // Duas versões do mesmo dataset: a mais antiga importada por último.
    for version in ["bancovde-2025.xlsx", "bancovde-2023.xlsx"] {
        security
            .record_import(&DatasetImport {
                source: DataSource::SinespVde,
                source_url: None,
                dataset_version: Some(version.into()),
                scope: "SP".into(),
                records_read: 1,
                records_stored: 1,
                records_skipped: 0,
                report: serde_json::json!({}),
            })
            .await
            .unwrap();
    }

    let census = NaiveDate::from_ymd_opt(2022, 8, 1).unwrap();
    let indicators = IndicatorRepository::new(pool.clone());
    for (code, population) in [("3548708", 810_729.0), ("3547809", 748_919.0)] {
        indicators
            .upsert(&RegionIndicator {
                region: Region {
                    level: RegionLevel::Municipality,
                    code: code.into(),
                    name: code.into(),
                    municipality_ibge_code: code.into(),
                },
                indicator: Indicator {
                    kind: IndicatorKind::Population,
                    value: population,
                    period_start: census,
                    period_end: census,
                },
                provenance: Provenance {
                    source: DataSource::IbgeLocalidades,
                    url: Some("teste".into()),
                    collected_at: Utc::now(),
                },
            })
            .await
            .unwrap();
    }
    Some(pool)
}

async fn get(app: &axum::Router, uri: &str) -> (StatusCode, Value) {
    let resp = app
        .clone()
        .oneshot(Request::get(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn security_endpoints_end_to_end() {
    let Some(pool) = setup().await else { return };
    let app = crab_api::router(crab_api::AppState::new(pool));

    // Relatório municipal: ano completo mais recente, taxa e população usada.
    let (status, body) = get(&app, "/regions/3548708/security").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["geographic_scope"], "municipality");
    assert_eq!(body["year"], 2025);
    let vehicle = body["indicators"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["crime_type"] == "vehicle_robbery" && i["source"] == "ssp_sp")
        .expect("roubo de veículo SSP");
    let count = vehicle["count"].as_f64().unwrap();
    assert_eq!(vehicle["population"]["value"], 810_729.0);
    assert_eq!(vehicle["population"]["year"], 2022);
    let rate = vehicle["rate_per_100k"].as_f64().unwrap();
    assert!((rate - count / 810_729.0 * 100_000.0).abs() < 0.01);
    assert_eq!(vehicle["source_labels"][0], "ROUBO DE VEÍCULO");
    assert!(body["subregions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["level"] == "police_district"));
    let notes = body["transparency"]["notes"].to_string();
    assert!(notes.contains("população de 2022"), "{notes}");
    // Totais da SSP não entram (evita contagem dupla).
    assert!(!body.to_string().contains("TOTAL DE ESTUPRO"));

    // Delegacia: números absolutos, sem taxa.
    let (status, body) = get(
        &app,
        "/regions/3548708/security?level=police_district&code=01-dp-s-bernardo-do-campo",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["indicators"]
        .as_array()
        .unwrap()
        .iter()
        .all(|i| i["rate_per_100k"].is_null()));

    // Histórico com tendência só sobre anos completos.
    let (status, body) = get(&app, "/regions/3548708/security/history?crime_type=robbery").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let series = &body["series"][0];
    assert_eq!(series["points"].as_array().unwrap().len(), 4);
    assert_eq!(series["trend"]["from_year"], 2023);
    assert_eq!(series["trend"]["to_year"], 2025);
    assert_eq!(series["trend"]["metric"], "rate_per_100k");

    // Comparação entre municípios.
    let (status, body) = get(&app, "/security/compare?municipalities=3548708,3547809").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["items"].as_array().unwrap().len(), 2);

    // Erros de entrada.
    let (status, _) = get(&app, "/regions/abc/security").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = get(&app, "/regions/3548708/security/history?crime_type=x").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = get(&app, "/security/compare?municipalities=3548708").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, body) = get(&app, "/security/sources").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["sources"].as_array().unwrap().len() >= 3);
    let versions: Vec<&str> = body["latest_imports"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|i| i["dataset_version"].as_str())
        .collect();
    assert!(versions.contains(&"bancovde-2025.xlsx"), "{versions:?}");
    assert!(versions.contains(&"bancovde-2023.xlsx"), "{versions:?}");
}

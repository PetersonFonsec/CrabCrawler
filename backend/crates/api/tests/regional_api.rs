//! Regional Intelligence de ponta a ponta contra um PostgreSQL/PostGIS real:
//! fixtures fictícias → providers → normalização → banco → API.
//!
//! Precisa de `TEST_DATABASE_URL` (banco descartável: as tabelas regionais
//! são limpas no início). Sem a variável o teste é pulado.

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use chrono::Utc;
use crab_crawler::regional::geosampa::{self, GeoSampaInput, GeoSampaProvider};
use crab_crawler::regional::ibge::{IbgeBasicAggregatesProvider, IbgeSectorMeshProvider};
use crab_crawler::regional::seade::SeadeIpvsProvider;
use crab_crawler::regional::sgb::{SgbInput, SgbRiskProvider};
use crab_crawler::regional::{
    CensusSectorProvider, DemographicDataProvider, EnvironmentalRiskProvider,
    InfrastructureDataProvider, RegionalScope,
};
use crab_domain::regional::{
    DatasetBatch, DatasetProvenance, GeographicGranularity, ImportReport, RawGeometry,
    ServiceCategory, UrbanService,
};
use crab_domain::{DataSource, ListingKind, RawListing, TransactionType};
use crab_ingest::PropertyIngestor;
use crab_persistence::{PropertyRepository, RegionalRepository};
use crab_processing::regional::{normalize_risk_areas, normalize_sector_values, normalize_sectors};
use crab_processing::{Gazetteer, PropertyNormalizer};
use serde_json::Value;
use tower::ServiceExt;
use uuid::Uuid;

fn fixture(name: &str) -> String {
    format!("{}/../../fixtures/{name}", env!("CARGO_MANIFEST_DIR"))
}

fn regional(name: &str) -> String {
    fixture(&format!("regional/{name}"))
}

async fn import_fixtures(repo: &RegionalRepository) {
    let state = RegionalScope::parse("35").unwrap();

    let mesh = IbgeSectorMeshProvider::new(regional("ibge_setores_sample.gpkg"));
    let batch = normalize_sectors(mesh.sectors(&state).await.unwrap(), None);
    let out = repo.store_sectors(&batch).await.unwrap();
    assert_eq!(out.stored, 4);

    let aggregates = IbgeBasicAggregatesProvider::new(regional("ibge_agregados_basico_sample.zip"));
    let batch =
        normalize_sector_values(aggregates.sector_values(&state).await.unwrap(), None, |v| {
            aggregates.indicator_for(v)
        });
    repo.store_sector_indicators(&batch, aggregates.capability())
        .await
        .unwrap();

    let ipvs = SeadeIpvsProvider::new(regional("seade_ipvs_sample.csv"), None, None);
    let batch = normalize_sector_values(ipvs.sector_values(&state).await.unwrap(), None, |v| {
        ipvs.indicator_for(v)
    });
    repo.store_sector_indicators(&batch, ipvs.capability())
        .await
        .unwrap();

    let sbc = RegionalScope::parse("3548708").unwrap();
    let sgb = SgbRiskProvider::new(SgbInput::File(regional("sgb_risco_sample.geojson").into()));
    let batch = normalize_risk_areas(sgb.risk_areas(&sbc).await.unwrap(), Some("3548708"));
    assert_eq!(repo.store_risk_areas(&batch).await.unwrap().stored, 2);

    let sp = RegionalScope::parse(geosampa::SAO_PAULO).unwrap();
    for (layer, file) in [
        (
            "equipamento_cultura_bibliotecas",
            "geosampa_bibliotecas_sample.geojson",
        ),
        (
            "equipamento_saude_ambulatorios_especializados",
            "geosampa_ambulatorios_sample.geojson",
        ),
    ] {
        let provider = GeoSampaProvider::new(
            geosampa::layer(layer).unwrap(),
            GeoSampaInput::File(regional(file).into()),
        );
        let batch = provider.services(&sp).await.unwrap();
        repo.store_services(&batch).await.unwrap();
    }
}

async fn listing(
    ingestor: &PropertyIngestor,
    id: &str,
    text: &str,
    point: Option<(f64, f64)>,
) -> Uuid {
    let raw = RawListing {
        external_id: id.into(),
        url: None,
        title: id.into(),
        transaction: TransactionType::Sale,
        kind: ListingKind::Apartment,
        price_brl: Some(500_000.0),
        area_m2: Some(70.0),
        bedrooms: Some(2),
        bathrooms: Some(1),
        parking_spots: Some(1),
        location_text: text.into(),
        postal_code: None,
        lat: point.map(|p| p.0),
        lon: point.map(|p| p.1),
    };
    let raw = crab_ingest::fixture::to_raw_property(raw);
    let outcome = ingestor.ingest(raw, Utc::now()).await.unwrap();
    outcome.listings[0].listing_id
}

async fn get(app: &axum::Router, uri: &str) -> (StatusCode, Value) {
    let resp = app
        .clone()
        .oneshot(Request::get(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

fn near(value: &Value, expected: f64, tolerance: f64) -> bool {
    value
        .as_f64()
        .is_some_and(|v| (v - expected).abs() <= tolerance)
}

#[tokio::test]
async fn regional_intelligence_end_to_end() {
    let Ok(url) = std::env::var("TEST_DATABASE_URL") else {
        eprintln!("TEST_DATABASE_URL não definida; teste de integração pulado");
        return;
    };
    let pool = crab_persistence::connect(&url).await.expect("conexão");
    crab_persistence::migrate(&pool).await.expect("migrations");
    sqlx::query(
        "TRUNCATE census_sectors, census_sector_indicators, urban_services, \
         environmental_risk_areas, regional_dataset_coverage, regional_imports, \
         regional_datasets RESTART IDENTITY CASCADE",
    )
    .execute(&pool)
    .await
    .unwrap();

    let repo = RegionalRepository::new(pool.clone());
    import_fixtures(&repo).await;

    let mut gazetteer = Gazetteer::mvp();
    gazetteer.insert("SP", "São Paulo", geosampa::SAO_PAULO);
    let listings = PropertyIngestor::new(
        PropertyRepository::new(pool.clone()),
        PropertyNormalizer::new(gazetteer),
    );
    let sbc = "São Bernardo do Campo - SP / Rudge Ramos";
    let rudge = listing(&listings, "rt-rudge", sbc, Some((-23.656, -46.573))).await;
    let suppressed = listing(&listings, "rt-sigilo", sbc, Some((-23.656, -46.567))).await;
    let centro = listing(&listings, "rt-centro", sbc, Some((-23.694, -46.565))).await;
    let no_coords = listing(&listings, "rt-sem-coord", sbc, None).await;
    let outside = listing(&listings, "rt-fora", sbc, Some((-23.70, -46.55))).await;
    let capital = listing(
        &listings,
        "rt-sp",
        "São Paulo - SP / Sé",
        Some((-23.55, -46.63)),
    )
    .await;

    let app = crab_api::router(crab_api::AppState::new(pool.clone()));

    // --- Perfil: imóvel → setor (ST_Covers) → IBGE + Seade.
    let (status, body) = get(&app, &format!("/listings/{rudge}/region/profile")).await;
    assert_eq!(status, StatusCode::OK);
    let profile = &body["profile"];
    assert_eq!(profile["status"], "AVAILABLE");
    assert_eq!(profile["census_sector"]["code"], "354870805000012");
    let indicator = |name: &str| {
        profile["indicators"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["indicator"] == name)
            .cloned()
            .unwrap_or(Value::Null)
    };
    assert_eq!(indicator("population")["value"], 512.0);
    assert_eq!(indicator("population")["granularity"], "census_sector");
    assert_eq!(indicator("population")["source_variable"], "V0001");
    assert_eq!(indicator("population_density")["value"], 1422.0);
    assert_eq!(indicator("population_density")["derived"], true);
    assert_eq!(indicator("ipvs_group")["value"], 2.0);
    assert_eq!(indicator("ipvs_group")["source"]["source"], "seade_ipvs");
    assert!(indicator("average_income").is_null());
    let unavailable = profile["unavailable"].as_array().unwrap();
    assert!(unavailable
        .iter()
        .any(|u| u["item"] == "average_income" && u["reason"] == "not_published"));

    // Setor com valores suprimidos ("X").
    let (_, body) = get(&app, &format!("/listings/{suppressed}/region/profile")).await;
    assert_eq!(body["profile"]["census_sector"]["code"], "354870805000013");
    assert!(body["profile"]["unavailable"]
        .as_array()
        .unwrap()
        .iter()
        .any(|u| u["item"] == "population" && u["reason"] == "suppressed_by_source"));

    // Fora de qualquer setor importado do município.
    let (_, body) = get(&app, &format!("/listings/{outside}/region/profile")).await;
    assert_eq!(body["profile"]["status"], "NO_DATA");
    assert_eq!(
        body["profile"]["unavailable"][0]["reason"],
        "outside_imported_sectors"
    );

    // Sem coordenada: nada de centroide de bairro.
    let (_, body) = get(&app, &format!("/listings/{no_coords}/region")).await;
    let region = &body["region"];
    assert_eq!(
        region["profile"]["unavailable"][0]["reason"],
        "property_without_coordinates"
    );
    assert_eq!(region["services"]["reason"], "property_without_coordinates");
    assert_eq!(region["environmental_risks"]["assessment"], "NO_DATA");

    // --- Serviços. São Bernardo: o GeoSampa não cobre → NO_DATA, não "zero serviços".
    let (_, body) = get(&app, &format!("/listings/{rudge}/region/services")).await;
    let categories = body["services"]["categories"].as_array().unwrap();
    assert_eq!(categories.len(), ServiceCategory::ALL.len());
    assert!(categories
        .iter()
        .all(|c| c["status"] == "NO_DATA" && c["reason"] == "no_provider_for_location"));

    // Capital: distância geodésica, raio e mais próximo.
    let (_, body) = get(
        &app,
        &format!("/listings/{capital}/region/services?radius=1000"),
    )
    .await;
    let category = |name: &str| {
        body["services"]["categories"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["category"] == name)
            .cloned()
            .unwrap()
    };
    let culture = category("CULTURE");
    assert_eq!(culture["status"], "AVAILABLE");
    assert_eq!(culture["within_radius"].as_array().unwrap().len(), 1);
    assert_eq!(culture["nearest"]["name"], "Biblioteca Fictícia Centro");
    assert!(
        near(&culture["nearest"]["distance_m"], 232.0, 15.0),
        "{culture}"
    );
    assert_eq!(culture["nearest"]["distance_label"], "230 m");
    let health = category("HEALTH");
    assert!(
        near(&health["nearest"]["distance_m"], 830.0, 25.0),
        "{health}"
    );
    assert_eq!(category("EDUCATION")["reason"], "not_imported");
    assert_eq!(category("TRANSPORT")["reason"], "no_provider_for_location");

    // Filtro por categoria e raio menor: o ambulatório sai do raio, mas
    // continua como o mais próximo.
    let (_, body) = get(
        &app,
        &format!("/listings/{capital}/region/services?radius=500&category=HEALTH"),
    )
    .await;
    let cats = body["services"]["categories"].as_array().unwrap();
    assert_eq!(cats.len(), 1);
    assert!(cats[0]["within_radius"].as_array().unwrap().is_empty());
    assert!(cats[0]["nearest"].is_object());
    let (status, _) = get(
        &app,
        &format!("/listings/{capital}/region/services?radius=99999"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // --- Riscos: dentro da área (ST_Intersects).
    let (_, body) = get(&app, &format!("/listings/{centro}/region/risks")).await;
    let risks = &body["environmental_risks"];
    assert_eq!(risks["assessment"], "INSIDE_MAPPED_RISK_AREA");
    let landslide = risks["risks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["type"] == "LANDSLIDE")
        .unwrap();
    assert_eq!(landslide["inside_risk_area"], true);
    assert_eq!(landslide["severity"], "Muito Alto");
    assert_eq!(landslide["mapped_on"], "2014-05-14");

    // Fora, com a área mais próxima (ST_Distance).
    let (_, body) = get(&app, &format!("/listings/{rudge}/region/risks")).await;
    let risks = &body["environmental_risks"];
    assert_eq!(risks["assessment"], "NOT_IN_MAPPED_RISK_AREA");
    let flood = &risks["risks"][0];
    assert_eq!(flood["type"], "FLOOD");
    assert_eq!(flood["inside_risk_area"], false);
    assert!(near(&flood["distance_meters"], 326.0, 15.0), "{flood}");
    assert!(risks["message"]
        .as_str()
        .unwrap()
        .contains("não significa ausência de risco"));

    let (_, body) = get(
        &app,
        &format!("/listings/{rudge}/region/risks?max_distance=100"),
    )
    .await;
    assert_eq!(
        body["environmental_risks"]["assessment"],
        "NOT_IN_MAPPED_RISK_AREA"
    );
    assert!(body["environmental_risks"]["risks"]
        .as_array()
        .unwrap()
        .is_empty());
    let (_, body) = get(&app, &format!("/listings/{centro}/region/risks?type=flood")).await;
    assert!(body["environmental_risks"]["risks"]
        .as_array()
        .unwrap()
        .is_empty());

    // Capital: nenhum dataset de risco importado para o município → NO_DATA.
    let (_, body) = get(&app, &format!("/listings/{capital}/region/risks")).await;
    assert_eq!(body["environmental_risks"]["assessment"], "NO_DATA");
    assert_eq!(body["environmental_risks"]["reason"], "not_imported");

    // --- Agregado e fontes.
    let (status, body) = get(&app, &format!("/listings/{capital}/intelligence")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["property"]["external_id"], "rt-sp");
    assert!(body["region"]["profile"].is_object());
    assert!(body["region"]["services"].is_object());
    assert!(body["region"]["environmental_risks"].is_object());

    let (_, body) = get(&app, "/regional/sources").await;
    let providers = body["providers"].as_array().unwrap();
    assert_eq!(providers.len(), 4);
    assert!(providers
        .iter()
        .all(|p| p["refresh"]["interval_days"].is_number()));

    idempotency_and_versions(&repo, &pool).await;
}

/// Reimportar a mesma versão não duplica; uma versão nova remove o que
/// sumiu da fonte; geometria inválida descarta só o registro.
async fn idempotency_and_versions(repo: &RegionalRepository, pool: &sqlx::PgPool) {
    let service = |id: &str, json: &str| UrbanService {
        external_id: id.into(),
        name: Some(id.into()),
        category: ServiceCategory::Sport,
        subcategory: None,
        address: None,
        municipality_ibge_code: geosampa::SAO_PAULO.into(),
        geometry: RawGeometry::GeoJson {
            json: json.into(),
            srid: 4326,
        },
        attributes: serde_json::json!({}),
    };
    let point = r#"{"type":"Point","coordinates":[-46.63,-23.55]}"#;
    let batch = |version: &str, records: Vec<UrbanService>| DatasetBatch {
        provenance: DatasetProvenance {
            source: DataSource::GeoSampa,
            dataset_name: "equipamento_esporte_clubes".into(),
            dataset_version: version.into(),
            source_url: None,
            reference_date: None,
            collected_at: Utc::now(),
            granularity: GeographicGranularity::Point,
        },
        coverage: vec![geosampa::SAO_PAULO.into()],
        records,
        report: ImportReport::default(),
    };
    let count = || async {
        let (n,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM urban_services WHERE dataset_name = 'equipamento_esporte_clubes'",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        n
    };

    let v1 = batch("v1", vec![service("a", point), service("b", point)]);
    repo.store_services(&v1).await.unwrap();
    let again = repo.store_services(&v1).await.unwrap();
    assert_eq!(again.removed, 0);
    assert_eq!(count().await, 2);
    let (datasets,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM regional_datasets WHERE dataset_name = 'equipamento_esporte_clubes'",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(datasets, 1);

    // v2: "b" sumiu da fonte e "c" tem geometria inválida.
    let v2 = batch(
        "v2",
        vec![
            service("a", point),
            service("c", r#"{"type":"Point","coordinates":"x"}"#),
        ],
    );
    let out = repo.store_services(&v2).await.unwrap();
    assert_eq!(out.stored, 1);
    assert_eq!(out.rejected, 1);
    assert_eq!(out.removed, 1);
    assert_eq!(count().await, 1);

    // Importação registrada com status e contagens.
    let id = repo
        .start_import("geosampa", "equipamento_esporte_clubes", "3550308")
        .await
        .unwrap();
    repo.finish_import_ok(id, &v2.provenance, &v2.report, &out)
        .await
        .unwrap();
    let failed = repo
        .start_import("geosampa", "equipamento_esporte_clubes", "3550308")
        .await
        .unwrap();
    repo.finish_import_failed(failed, "rede indisponível")
        .await
        .unwrap();
    let latest = repo.latest_imports().await.unwrap();
    let last = latest
        .iter()
        .find(|i| i.dataset_name == "equipamento_esporte_clubes")
        .unwrap();
    assert_eq!(last.status, "failed");
    assert_eq!(last.error.as_deref(), Some("rede indisponível"));
}

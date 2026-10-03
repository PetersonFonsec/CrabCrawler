//! Property Sources de ponta a ponta contra PostgreSQL/PostGIS real:
//! cadastro manual pela API, sincronização VRSync (fixtures fictícias),
//! histórico de preço, inativação e comparador entre origens.
//!
//! Precisa de `TEST_DATABASE_URL`. Cada teste usa parceiros e bairros
//! próprios, então pode rodar num banco já usado por outros testes.

use std::sync::Arc;

use async_trait::async_trait;
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use chrono::Utc;
use crab_crawler::geocoding::{
    GeocodeQuery, GeocodeResult, Geocoder, PostalAddress, PostalCodeLookup,
};
use crab_crawler::property_sources::vrsync::{VrsyncInput, VrsyncProvider};
use crab_crawler::CrawlError;
use crab_domain::{
    CoordinatePrecision, GeoPoint, PartnerStatus, PartnerType, PropertySource,
    PropertySourcePartner, RawProperty, SyncRunStatus,
};
use crab_ingest::{PropertyIngestor, SyncOptions, SyncReport};
use crab_persistence::PropertyRepository;
use crab_processing::{Gazetteer, PropertyNormalizer};
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

fn vrsync(name: &str) -> std::path::PathBuf {
    format!(
        "{}/../../fixtures/vrsync/{name}",
        env!("CARGO_MANIFEST_DIR")
    )
    .into()
}

async fn pool() -> Option<PgPool> {
    let url = std::env::var("TEST_DATABASE_URL").ok()?;
    let pool = crab_persistence::connect(&url).await.expect("conexão");
    crab_persistence::migrate(&pool).await.expect("migrations");
    Some(pool)
}

fn ingestor(pool: &PgPool) -> PropertyIngestor {
    PropertyIngestor::new(
        PropertyRepository::new(pool.clone()),
        PropertyNormalizer::new(Gazetteer::mvp()),
    )
}

async fn call(app: &axum::Router, req: Request<Body>) -> (StatusCode, Value) {
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

async fn get(app: &axum::Router, uri: &str) -> (StatusCode, Value) {
    call(app, Request::get(uri).body(Body::empty()).unwrap()).await
}

async fn post(app: &axum::Router, uri: &str, body: Value) -> (StatusCode, Value) {
    call(
        app,
        Request::post(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap(),
    )
    .await
}

fn error_fields(body: &Value) -> Vec<String> {
    body["errors"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|e| e["field"].as_str().unwrap_or("").to_string())
                .collect()
        })
        .unwrap_or_default()
}

// --------------------------------------------------------------- manual

#[tokio::test]
async fn manual_entry() {
    let Some(pool) = pool().await else {
        eprintln!("TEST_DATABASE_URL não definida; teste pulado");
        return;
    };
    let app = crab_api::router(crab_api::AppState::new(pool.clone()));

    // Cadastro válido e completo.
    let (status, body) = post(
        &app,
        "/properties/manual",
        json!({
            "transaction": "sale",
            "property_type": "apartamento",
            "price": 620000,
            "postal_code": "09640-000",
            "street": "R. Exemplo",
            "number": "100",
            "neighborhood": "Rudge Ramos",
            "municipality": "São Bernardo do Campo",
            "state": "SP",
            "area_m2": 80,
            "bedrooms": 2,
            "bathrooms": 2,
            "suites": 1,
            "parking_spaces": 1,
            "condominium_fee": 650,
            "property_tax": 1800,
            "source_url": "https://www.zapimoveis.com.br/imovel/exemplo-123/",
            "notes": "Encontrei este imóvel no ZAP.",
            "latitude": -23.656,
            "longitude": -46.573
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let property_id = body["property"]["id"].as_str().unwrap().to_string();
    assert_eq!(body["property"]["property_type"], "apartment");
    assert_eq!(body["property"]["street"], "Rua Exemplo");
    assert_eq!(body["property"]["postal_code"], "09640000");
    assert_eq!(body["property"]["municipality_ibge_code"], "3548708");
    assert_eq!(body["property"]["coordinate_source"], "MANUAL");
    assert_eq!(body["listing"]["source"], "MANUAL");
    assert_eq!(body["listing"]["status"], "ACTIVE");
    assert_eq!(body["listing"]["price_brl"], 620000.0);
    assert_eq!(
        body["listing"]["source_url"],
        "https://www.zapimoveis.com.br/imovel/exemplo-123/"
    );
    assert_eq!(body["analysis"]["region_ready"], true);
    assert_eq!(body["analysis"]["comparison_ready"], true);

    let (status, detail) = get(&app, &format!("/properties/{property_id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        detail["listings"][0]["price_history"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(detail["listings"][0]["price_insight"].is_null());

    let (status, listings) = get(&app, &format!("/properties/{property_id}/listings")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listings.as_array().unwrap().len(), 1);

    // Regional Intelligence funciona para o imóvel manual.
    let (status, region) = get(&app, &format!("/properties/{property_id}/region")).await;
    assert_eq!(status, StatusCode::OK, "{region}");
    assert_eq!(region["property"]["coordinate_source"], "MANUAL");

    // Campos mínimos: finalidade, preço e CEP. O resto vira "o que falta".
    let (status, body) = post(
        &app,
        "/properties/manual",
        json!({ "transaction": "venda", "price": 450000, "postal_code": "09715000" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["property"]["property_type"], "other");
    assert_eq!(body["analysis"]["region_ready"], false);
    assert_eq!(body["analysis"]["comparison_ready"], false);
    let missing = body["analysis"]["missing"].to_string();
    assert!(
        missing.contains("Coordenada") && missing.contains("Área"),
        "{missing}"
    );

    // CEP inválido.
    let (status, body) = post(
        &app,
        "/properties/manual",
        json!({ "transaction": "sale", "price": 450000, "postal_code": "0964-00" }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        error_fields(&body).contains(&"postal_code".to_string()),
        "{body}"
    );

    // Preço inválido (zero, negativo, absurdo).
    for price in [json!(0), json!(-10), json!(5_000_000_000_i64)] {
        let (status, body) = post(
            &app,
            "/properties/manual",
            json!({ "transaction": "sale", "price": price, "postal_code": "09640000" }),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{price}");
        assert!(
            error_fields(&body).contains(&"sale_price".to_string()),
            "{body}"
        );
    }

    // Incompleto: sem preço e sem localização.
    let (status, body) = post(&app, "/properties/manual", json!({ "transaction": "rent" })).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let fields = error_fields(&body);
    assert!(fields.contains(&"rent_price".to_string()), "{body}");
    assert!(fields.contains(&"location".to_string()), "{body}");

    // Sem finalidade.
    let (status, body) = post(
        &app,
        "/properties/manual",
        json!({ "price": 450000, "postal_code": "09640000" }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(error_fields(&body).contains(&"transaction".to_string()));

    // URL perigosa não é aceita (e nunca seria acessada).
    let (status, body) = post(
        &app,
        "/properties/manual",
        json!({ "transaction": "sale", "price": 450000, "postal_code": "09640000",
                "source_url": "javascript:alert(1)" }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(error_fields(&body).contains(&"source_url".to_string()));

    // UF inválida.
    let (status, _) = post(
        &app,
        "/properties/manual",
        json!({ "transaction": "sale", "price": 450000, "municipality": "X", "state": "ZZ" }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // JSON quebrado e corpo grande demais.
    let (status, _) = call(
        &app,
        Request::post("/properties/manual")
            .header("content-type", "application/json")
            .body(Body::from("{ nada"))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = post(
        &app,
        "/properties/manual",
        json!({ "transaction": "sale", "price": 1, "notes": "x".repeat(64 * 1024) }),
    )
    .await;
    assert!(
        status == StatusCode::PAYLOAD_TOO_LARGE || status == StatusCode::BAD_REQUEST,
        "{status}"
    );

    // Listagem por origem.
    let (status, list) = get(&app, "/properties?source=MANUAL&limit=5").await;
    assert_eq!(status, StatusCode::OK);
    assert!(list.as_array().unwrap().iter().all(|p| p["listings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|l| l["source"] == "MANUAL")));

    let (status, _) = get(&app, &format!("/properties/{}", Uuid::new_v4())).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ------------------------------------------- manual com CEP e geocoding

struct FakeCep;

#[async_trait]
impl PostalCodeLookup for FakeCep {
    fn name(&self) -> &'static str {
        "cep-de-teste"
    }
    async fn lookup(&self, cep: &str) -> Result<Option<PostalAddress>, CrawlError> {
        Ok((cep == "09640000").then(|| PostalAddress {
            postal_code: cep.into(),
            street: Some("Rua do CEP".into()),
            neighborhood: Some("Rudge Ramos".into()),
            municipality: Some("São Bernardo do Campo".into()),
            state: Some("SP".into()),
            municipality_ibge_code: Some("3548708".into()),
        }))
    }
}

struct FakeGeocoder;

#[async_trait]
impl Geocoder for FakeGeocoder {
    fn name(&self) -> &'static str {
        "geocoder-de-teste"
    }
    async fn geocode(&self, q: &GeocodeQuery) -> Result<Option<GeocodeResult>, CrawlError> {
        Ok(Some(GeocodeResult {
            point: GeoPoint::new(-23.6561, -46.5731).unwrap(),
            precision: if q.number.is_some() {
                CoordinatePrecision::Exact
            } else {
                CoordinatePrecision::Street
            },
            label: Some("teste".into()),
        }))
    }
}

#[tokio::test]
async fn manual_entry_is_completed_by_postal_code_and_geocoded() {
    let Some(pool) = pool().await else { return };
    let ingestor = ingestor(&pool)
        .with_postal_lookup(Arc::new(FakeCep))
        .with_geocoder(Arc::new(FakeGeocoder));
    let app = crab_api::router(crab_api::AppState::with_ingestor(pool.clone(), ingestor));

    let (status, body) = post(
        &app,
        "/properties/manual",
        json!({ "transaction": "sale", "price": 500000, "postal_code": "09640-000",
                "number": "42", "property_type": "Apartamento", "area_m2": 70 }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let p = &body["property"];
    assert_eq!(p["street"], "Rua do CEP");
    assert_eq!(p["municipality_ibge_code"], "3548708");
    assert_eq!(p["neighborhood_slug"], "rudge-ramos");
    assert_eq!(p["coordinate_source"], "GEOCODING");
    assert_eq!(p["coordinate_precision"], "EXACT");
    assert_eq!(body["analysis"]["region_ready"], true);
    assert_eq!(body["notes"].as_array().unwrap().len(), 2);
}

// --------------------------------------------------------------- VRSync

async fn partner(pool: &PgPool) -> PropertySourcePartner {
    let now = Utc::now();
    let p = PropertySourcePartner {
        id: Uuid::new_v4(),
        slug: format!("teste-{}", &Uuid::new_v4().simple().to_string()[..12]),
        name: "Imobiliária de teste".into(),
        partner_type: PartnerType::RealEstateAgency,
        status: PartnerStatus::Active,
        provider: PropertySource::Vrsync,
        configuration: json!({ "feed_url": "https://exemplo.invalid/feed.xml" }),
        created_at: now,
        updated_at: now,
    };
    PropertyRepository::new(pool.clone())
        .create_partner(&p)
        .await
        .unwrap();
    p
}

async fn sync_file(
    ingestor: &PropertyIngestor,
    p: &PropertySourcePartner,
    file: std::path::PathBuf,
) -> SyncReport {
    let provider = VrsyncProvider::new(VrsyncInput::File(file), Some(p.id));
    ingestor
        .sync(&provider, Some(p), &SyncOptions::default())
        .await
        .unwrap()
}

async fn listing_by_external(
    pool: &PgPool,
    partner: Uuid,
    external: &str,
) -> Vec<(Uuid, String, String)> {
    sqlx::query_as(
        "SELECT id, transaction, status FROM listings WHERE partner_id = $1 AND external_id = $2 \
         ORDER BY transaction",
    )
    .bind(partner)
    .bind(external)
    .fetch_all(pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn vrsync_sync_lifecycle() {
    let Some(pool) = pool().await else { return };
    let ingestor = ingestor(&pool);
    let p = partner(&pool).await;

    // Primeira carga: 7 itens, 4 anúncios válidos (SBC-002 é Sale/Rent),
    // 4 recusados (sem ListingID, moeda USD, ListingID repetido, CEP inválido).
    let r = sync_file(&ingestor, &p, vrsync("feed_v1.xml")).await;
    assert_eq!(r.status, SyncRunStatus::Succeeded);
    let m = r.metrics;
    assert_eq!(
        (
            m.received,
            m.created,
            m.updated,
            m.unchanged,
            m.invalid,
            m.deactivated,
            m.failed
        ),
        (7, 4, 0, 0, 4, 0, 0),
        "{r:?}"
    );
    let reasons: Vec<&str> = r.item_errors.iter().map(|e| e.reason.as_str()).collect();
    assert!(
        reasons.iter().any(|r| r.contains("ListingID")),
        "{reasons:?}"
    );
    assert!(reasons.iter().any(|r| r.contains("moeda")), "{reasons:?}");
    assert!(
        reasons.iter().any(|r| r.contains("repetido")),
        "{reasons:?}"
    );
    assert!(reasons.iter().any(|r| r.contains("CEP")), "{reasons:?}");

    // Sale/Rent: um imóvel, dois anúncios.
    let sbc002 = listing_by_external(&pool, p.id, "SBC-002").await;
    assert_eq!(sbc002.len(), 2);
    let props: Vec<(Uuid,)> = sqlx::query_as(
        "SELECT DISTINCT property_id FROM listings WHERE partner_id = $1 AND external_id = 'SBC-002'",
    )
    .bind(p.id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(props.len(), 1);

    // IPTU mensal virou anual; displayAddress="Neighborhood" escondeu rua e ponto.
    let (tax,): (Option<f64>,) = sqlx::query_as(
        "SELECT property_tax_brl FROM listings WHERE partner_id = $1 AND external_id = 'SBC-002' LIMIT 1",
    )
    .bind(p.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(tax, Some(1800.0));
    let (street, has_geom): (Option<String>, bool) = sqlx::query_as(
        "SELECT p.street, p.geom IS NOT NULL FROM listings l JOIN properties p ON p.id = l.property_id \
         WHERE l.partner_id = $1 AND l.external_id = 'SBC-003'",
    )
    .bind(p.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((street, has_geom), (None, false));

    // Importação repetida: idempotente.
    let r = sync_file(&ingestor, &p, vrsync("feed_v1.xml")).await;
    let m = r.metrics;
    assert_eq!(
        (m.created, m.updated, m.unchanged, m.deactivated),
        (0, 0, 4, 0),
        "{r:?}"
    );

    // Segunda versão: SBC-001 baixou de 620 mil para 590 mil, SBC-002 igual,
    // SBC-003 sumiu (inativado, não apagado), SBC-004 novo.
    let r = sync_file(&ingestor, &p, vrsync("feed_v2.xml")).await;
    let m = r.metrics;
    assert_eq!(
        (
            m.received,
            m.created,
            m.updated,
            m.unchanged,
            m.invalid,
            m.deactivated
        ),
        (4, 1, 1, 2, 1, 1),
        "{r:?}"
    );
    assert_eq!(
        listing_by_external(&pool, p.id, "SBC-003").await[0].2,
        "INACTIVE"
    );

    let sbc001 = listing_by_external(&pool, p.id, "SBC-001").await[0].0;
    let repo = PropertyRepository::new(pool.clone());
    let history: Vec<f64> = repo
        .price_history(&[sbc001])
        .await
        .unwrap()
        .iter()
        .map(|o| o.price_brl)
        .collect();
    assert_eq!(history, vec![620_000.0, 590_000.0]);

    // De novo a v2: nada muda, nada novo no histórico.
    let r = sync_file(&ingestor, &p, vrsync("feed_v2.xml")).await;
    assert_eq!(
        (r.metrics.unchanged, r.metrics.deactivated),
        (4, 0),
        "{r:?}"
    );
    assert_eq!(repo.price_history(&[sbc001]).await.unwrap().len(), 2);

    // A API mostra o histórico e o insight de redução.
    let app = crab_api::router(crab_api::AppState::new(pool.clone()));
    let property_id: (Uuid,) = sqlx::query_as("SELECT property_id FROM listings WHERE id = $1")
        .bind(sbc001)
        .fetch_one(&pool)
        .await
        .unwrap();
    let (status, detail) = get(&app, &format!("/properties/{}", property_id.0)).await;
    assert_eq!(status, StatusCode::OK);
    let insight = &detail["listings"][0]["price_insight"];
    assert_eq!(insight["direction"], "DOWN");
    assert!(
        insight["message"]
            .as_str()
            .unwrap()
            .starts_with("Preço reduzido em 4,8%"),
        "{insight}"
    );

    // Volta o SBC-003 (reativado) e o preço do SBC-001 sobe.
    let v3 = std::env::temp_dir().join(format!("crab-vrsync-{}.xml", Uuid::new_v4()));
    let v1 = std::fs::read_to_string(vrsync("feed_v1.xml")).unwrap();
    std::fs::write(
        &v3,
        v1.replace(
            "<ListPrice currency=\"BRL\">620000",
            "<ListPrice currency=\"BRL\">600000",
        ),
    )
    .unwrap();
    let r = sync_file(&ingestor, &p, v3.clone()).await;
    std::fs::remove_file(&v3).ok();
    assert_eq!(
        listing_by_external(&pool, p.id, "SBC-003").await[0].2,
        "ACTIVE"
    );
    assert_eq!(r.metrics.deactivated, 1, "SBC-004 saiu: {r:?}");
    let history: Vec<f64> = repo
        .price_history(&[sbc001])
        .await
        .unwrap()
        .iter()
        .map(|o| o.price_brl)
        .collect();
    assert_eq!(history, vec![620_000.0, 590_000.0, 600_000.0]);

    // XML inválido e XXE: a execução falha inteira e nada é inativado.
    let active_before = repo
        .count_active(PropertySource::Vrsync, Some(p.id))
        .await
        .unwrap();
    for bad in ["feed_malformed.xml", "feed_xxe.xml"] {
        let r = sync_file(&ingestor, &p, vrsync(bad)).await;
        assert_eq!(r.status, SyncRunStatus::Failed, "{bad}");
        assert_eq!(r.metrics.deactivated, 0);
    }
    assert_eq!(
        repo.count_active(PropertySource::Vrsync, Some(p.id))
            .await
            .unwrap(),
        active_before
    );

    // Feed vazio: válido, mas a inativação fica suspensa.
    let empty = std::env::temp_dir().join(format!("crab-vrsync-{}.xml", Uuid::new_v4()));
    std::fs::write(
        &empty,
        r#"<ListingDataFeed xmlns="http://www.vivareal.com/schemas/1.0/VRSync"><Listings/></ListingDataFeed>"#,
    )
    .unwrap();
    let r = sync_file(&ingestor, &p, empty.clone()).await;
    std::fs::remove_file(&empty).ok();
    assert_eq!(r.status, SyncRunStatus::Succeeded);
    let d = r.deactivation.unwrap();
    assert!(!d.applied && d.missing > 0, "{d:?}");

    // Execuções registradas com métricas.
    let runs = repo.recent_runs(50).await.unwrap();
    let mine: Vec<_> = runs.iter().filter(|r| r.partner_id == Some(p.id)).collect();
    assert_eq!(mine.len(), 8);
    assert!(mine
        .iter()
        .any(|r| r.status == "FAILED" && r.error.is_some()));
}

// ------------------------------------------------------------ comparador

fn raw(source: PropertySource, external: &str, neighborhood: &str, price: &str) -> RawProperty {
    RawProperty {
        source: Some(source),
        external_id: Some(external.into()),
        transaction: Some("sale".into()),
        property_type: Some("apartment".into()),
        sale_price: Some(price.into()),
        living_area: Some("80".into()),
        neighborhood: Some(neighborhood.into()),
        municipality: Some("São Bernardo do Campo".into()),
        state: Some("SP".into()),
        latitude: Some("-23.656".into()),
        longitude: Some("-46.573".into()),
        ..Default::default()
    }
}

#[tokio::test]
async fn comparator_ignores_origin() {
    let Some(pool) = pool().await else { return };
    let ingestor = ingestor(&pool);
    let hood = format!("Bairro Teste {}", Uuid::new_v4().simple());
    let mut ids = Vec::new();
    for (source, price) in [
        (PropertySource::Manual, "480000"),
        (PropertySource::Vrsync, "500000"),
        (PropertySource::Api, "520000"),
    ] {
        let mut r = raw(source, &Uuid::new_v4().to_string(), &hood, price);
        if source == PropertySource::Manual {
            r.external_id = None;
        }
        let out = ingestor.ingest(r, Utc::now()).await.unwrap();
        ids.push((out.property_id, out.listings[0].listing_id));
    }
    let app = crab_api::router(crab_api::AppState::new(pool.clone()));
    for (property_id, _) in &ids {
        let (status, detail) = get(&app, &format!("/properties/{property_id}")).await;
        assert_eq!(status, StatusCode::OK);
        let cmp = &detail["listings"][0]["price_comparison"];
        assert_eq!(cmp["sample_size"], 2, "{detail}");
        assert!(cmp["median_price_per_m2"].as_f64().is_some());
        let (status, _) = get(&app, &format!("/properties/{property_id}/intelligence")).await;
        assert_eq!(status, StatusCode::OK);
    }
    // As rotas antigas de anúncio também valem para qualquer origem.
    let listing_ids: Vec<String> = ids.iter().map(|(_, l)| l.to_string()).collect();
    let (status, body) = get(&app, &format!("/listings/{}", listing_ids[2])).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["price_comparison"]["sample_size"], 2);
    let (status, body) = get(
        &app,
        &format!("/security/compare?listings={}", listing_ids.join(",")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

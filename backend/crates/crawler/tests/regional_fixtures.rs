//! Providers regionais lendo as fixtures de `fixtures/regional/` (fictícias,
//! no formato documentado de cada fonte).

use crab_crawler::regional::geosampa::{self, GeoSampaInput, GeoSampaProvider};
use crab_crawler::regional::ibge::{IbgeBasicAggregatesProvider, IbgeSectorMeshProvider};
use crab_crawler::regional::seade::SeadeIpvsProvider;
use crab_crawler::regional::sgb::{SgbInput, SgbRiskProvider};
use crab_crawler::regional::{
    CensusSectorProvider, DemographicDataProvider, EnvironmentalRiskProvider,
    InfrastructureDataProvider, RegionalDataProvider, RegionalScope,
};
use crab_domain::regional::{Capability, RawGeometry, SectorIndicator, ServiceCategory};
use crab_domain::DataSource;

fn fixture(name: &str) -> String {
    format!(
        "{}/../../fixtures/regional/{name}",
        env!("CARGO_MANIFEST_DIR")
    )
}

fn sp() -> RegionalScope {
    RegionalScope::parse("35").unwrap()
}

fn sbc() -> RegionalScope {
    RegionalScope::parse("3548708").unwrap()
}

#[tokio::test]
async fn ibge_mesh_reads_geopackage() {
    let provider = IbgeSectorMeshProvider::new(fixture("ibge_setores_sample.gpkg"));
    assert!(provider.descriptor().provides(Capability::CensusSectors));
    let batch = provider.sectors(&sp()).await.unwrap();
    // 4 setores válidos + 1 inválido (passa o filtro de prefixo) + 1 vazio.
    assert_eq!(batch.report.read, 6);
    assert_eq!(batch.report.skipped, 1, "{:?}", batch.report.issues);
    assert_eq!(batch.records.len(), 5);
    let first = &batch.records[0];
    assert_eq!(first.code, "354870805000012P");
    assert_eq!(first.area_km2, Some(0.36));
    assert!(matches!(
        first.geometry,
        RawGeometry::Wkb { srid: 4674, .. }
    ));
    assert!(batch
        .provenance
        .dataset_version
        .starts_with("ibge_setores_sample.gpkg@sha256:"));

    let only_sbc = provider.sectors(&sbc()).await.unwrap();
    assert!(only_sbc
        .records
        .iter()
        .all(|s| s.code.starts_with("3548708")));
    assert_eq!(only_sbc.records.len(), 4);
}

#[tokio::test]
async fn ibge_aggregates_read_zipped_latin1_csv() {
    let provider = IbgeBasicAggregatesProvider::new(fixture("ibge_agregados_basico_sample.zip"));
    let batch = provider.sector_values(&sbc()).await.unwrap();
    assert_eq!(batch.report.read, 4);
    // 4 setores × 7 colunas V (V0001 a V0007).
    assert_eq!(batch.records.len(), 28);
    assert_eq!(
        provider.indicator_for("V0001"),
        Some(SectorIndicator::Population)
    );
    assert_eq!(provider.indicator_for("V0006"), None);
    assert_eq!(batch.provenance.source, DataSource::IbgeCensoSetores);
}

#[tokio::test]
async fn seade_ipvs_detects_columns() {
    let provider = SeadeIpvsProvider::new(fixture("seade_ipvs_sample.csv"), None, None);
    let batch = provider.sector_values(&sbc()).await.unwrap();
    assert_eq!(batch.records.len(), 2);
    assert!(batch.records.iter().all(|r| r.variable == "grupo_ipvs"));
    assert_eq!(provider.capability(), Capability::SocialVulnerability);

    let rio = RegionalScope::parse("33").unwrap();
    assert!(provider.sector_values(&rio).await.is_err());
}

#[tokio::test]
async fn sgb_reads_arcgis_geojson() {
    let provider = SgbRiskProvider::new(SgbInput::File(fixture("sgb_risco_sample.geojson").into()));
    let batch = provider.risk_areas(&sbc()).await.unwrap();
    assert_eq!(batch.records.len(), 3);
    assert!(batch.records.iter().any(|r| r.geometry.is_none()));
    let other = provider
        .risk_areas(&RegionalScope::parse("3547809").unwrap())
        .await
        .unwrap();
    assert!(other.records.is_empty());
}

#[tokio::test]
async fn geosampa_reads_wfs_geojson_and_refuses_other_municipalities() {
    let layer = geosampa::layer("equipamento_cultura_bibliotecas").unwrap();
    let provider = GeoSampaProvider::new(
        layer,
        GeoSampaInput::File(fixture("geosampa_bibliotecas_sample.geojson").into()),
    );
    let batch = provider
        .services(&RegionalScope::parse("3550308").unwrap())
        .await
        .unwrap();
    assert_eq!(batch.records.len(), 2);
    assert_eq!(batch.coverage, vec!["3550308".to_string()]);
    assert!(batch
        .records
        .iter()
        .all(|s| s.category == ServiceCategory::Culture));
    assert!(provider.services(&sbc()).await.is_err());
}

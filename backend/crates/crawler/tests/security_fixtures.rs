//! Providers de segurança lendo os arquivos de `fixtures/` (fictícios).

use crab_crawler::security::sinesp::{SinespVdeProvider, VdeInput};
use crab_crawler::security::ssp_sp::SspSpCsvProvider;
use crab_crawler::{SecurityDataProvider, SecurityScope};
use crab_domain::{CountingUnit, DataSource};

fn fixture(name: &str) -> String {
    format!("{}/../../fixtures/{name}", env!("CARGO_MANIFEST_DIR"))
}

#[tokio::test]
async fn ssp_csv_fixture_filters_by_municipality() {
    let provider = SspSpCsvProvider::new(fixture("ssp_sp_sample.csv"));
    let all = provider.fetch(&SecurityScope::state("SP")).await.unwrap();
    let sbc = provider
        .fetch(&SecurityScope::municipality("SP", "3548708"))
        .await
        .unwrap();
    assert!(sbc.len() < all.len());
    assert!(sbc
        .iter()
        .all(|r| r.municipality_ibge_code.as_deref() == Some("3548708")));
    assert!(sbc.iter().any(|r| r.police_unit.is_some()));
    assert!(sbc.iter().all(|r| r.source == DataSource::SspSp));
    assert_eq!(sbc[0].dataset_version.as_deref(), Some("ssp_sp_sample.csv"));

    let other_state = provider.fetch(&SecurityScope::state("RJ")).await.unwrap();
    assert!(other_state.is_empty());
}

#[tokio::test]
async fn sinesp_xlsx_fixture_reads_real_spreadsheet() {
    let provider = SinespVdeProvider::new(VdeInput::File(fixture("sinesp_vde_sample.xlsx").into()));
    let records = provider.fetch(&SecurityScope::state("SP")).await.unwrap();
    // 12 meses × 6 linhas municipais de SP (a linha estadual e a do RJ ficam de fora).
    assert_eq!(records.len(), 72);
    let homicide = records
        .iter()
        .find(|r| r.label == "Homicídio doloso")
        .unwrap();
    assert_eq!(homicide.counting_unit, Some(CountingUnit::Victims));
    assert_eq!(homicide.year, 2025);
    assert_eq!(
        homicide.municipality_name.as_deref(),
        Some("SÃO BERNARDO DO CAMPO")
    );
    let months: std::collections::BTreeSet<u32> = records.iter().map(|r| r.month).collect();
    assert_eq!(months.len(), 12);
}

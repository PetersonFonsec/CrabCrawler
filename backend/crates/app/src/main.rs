//! Binário único do monólito: CLI para migrar, coletar e servir a API.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use clap::{Parser, Subcommand};
use crab_crawler::http::HttpClient;
use crab_crawler::regional::geosampa::{self, GeoSampaInput, GeoSampaProvider};
use crab_crawler::regional::ibge::{IbgeBasicAggregatesProvider, IbgeSectorMeshProvider};
use crab_crawler::regional::raw::RawStore;
use crab_crawler::regional::seade::SeadeIpvsProvider;
use crab_crawler::regional::sgb::{SgbInput, SgbRiskProvider};
use crab_crawler::regional::{
    CensusSectorProvider, DemographicDataProvider, EnvironmentalRiskProvider,
    InfrastructureDataProvider, RegionalDataProvider, RegionalScope,
};
use crab_crawler::security::sinesp::{SinespVdeProvider, VdeInput};
use crab_crawler::security::ssp_sp::SspSpCsvProvider;
use crab_crawler::sources::{fixture::FixtureListingSource, ibge::IbgeClient};
use crab_crawler::{IndicatorSource, ListingSource, SecurityDataProvider, SecurityScope};
use crab_domain::regional::{DatasetProvenance, ImportReport};
use crab_domain::DataSource;
use crab_persistence::{
    DatasetImport, IndicatorRepository, ListingRepository, RegionalRepository, SecurityRepository,
    StoreOutcome,
};
use crab_processing::regional::{normalize_risk_areas, normalize_sector_values, normalize_sectors};
use crab_processing::security::normalize_crime_records;
use crab_processing::{normalize_listing, Gazetteer};
use tracing_subscriber::EnvFilter;

/// Código IBGE de São Bernardo do Campo - SP, região do MVP.
const MVP_MUNICIPALITY: &str = "3548708";

#[derive(Parser)]
#[command(
    name = "crabcrawler",
    version,
    about = "Imóvel + região, com dados abertos"
)]
struct Cli {
    #[arg(long, env = "DATABASE_URL")]
    database_url: String,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Aplica as migrations do banco.
    Migrate,
    /// Coleta e persiste dados.
    Crawl {
        #[command(subcommand)]
        target: CrawlTarget,
    },
    /// Sobe a API HTTP.
    Serve {
        #[arg(long, env = "API_ADDR", default_value = "0.0.0.0:3000")]
        addr: String,
    },
}

#[derive(Subcommand)]
enum CrawlTarget {
    /// Anúncios de um arquivo JSON local.
    Listings {
        #[arg(long, default_value = "fixtures/listings.json")]
        file: PathBuf,
    },
    /// População do município (API do IBGE).
    Ibge {
        #[arg(long, default_value = MVP_MUNICIPALITY)]
        municipality: String,
    },
    /// Ocorrências criminais da SSP-SP (Dados Mensais exportados para CSV).
    SspSp {
        #[arg(long, default_value = "fixtures/ssp_sp_sample.csv")]
        file: PathBuf,
        /// Restringe a um município (código IBGE). Sem ele, importa todo o arquivo.
        #[arg(long)]
        municipality: Option<String>,
    },
    /// Sinesp VDE (MJSP): planilha local ou download do gov.br por ano.
    Sinesp {
        /// Planilha bancovde-<ano>.xlsx já baixada.
        #[arg(long, conflicts_with = "year")]
        file: Option<PathBuf>,
        /// Ano para baixar direto do gov.br.
        #[arg(long)]
        year: Option<i32>,
        #[arg(long, default_value = "SP")]
        state: String,
        /// Restringe a um município (código IBGE).
        #[arg(long)]
        municipality: Option<String>,
        /// Resolve nomes de município pela API do IBGE (senão usa a lista do MVP).
        #[arg(long)]
        ibge_gazetteer: bool,
    },
    /// Malha de setores censitários do Censo 2022 (GeoPackage do IBGE).
    IbgeSetores {
        /// Ex.: SP_setores_CD2022.gpkg
        #[arg(long)]
        file: PathBuf,
        /// Código IBGE do município (7 dígitos) ou da UF (2 dígitos).
        #[arg(long, default_value = MVP_MUNICIPALITY)]
        scope: String,
    },
    /// Agregados por setor do Censo 2022, arquivo Básico (CSV ou ZIP do IBGE).
    IbgeAgregados {
        /// Ex.: Agregados_por_setores_basico_BR_20260520.zip
        #[arg(long)]
        file: PathBuf,
        #[arg(long, default_value = MVP_MUNICIPALITY)]
        scope: String,
    },
    /// IPVS 2022 da Fundação Seade (CSV por setor censitário).
    SeadeIpvs {
        #[arg(long)]
        file: PathBuf,
        #[arg(long, default_value = MVP_MUNICIPALITY)]
        scope: String,
        /// Nome da coluna do setor (padrão: CD_SETOR ou a primeira com "setor").
        #[arg(long)]
        sector_column: Option<String>,
        /// Nome da coluna do grupo (padrão: a primeira com "ipvs" ou "grupo").
        #[arg(long)]
        group_column: Option<String>,
    },
    /// Equipamentos urbanos do GeoSampa (município de São Paulo) via WFS.
    Geosampa {
        /// Camadas WFS (padrão: todas as confirmadas). Repita a opção para várias.
        #[arg(long)]
        layer: Vec<String>,
        /// GeoJSON já baixado (exige exatamente uma --layer).
        #[arg(long)]
        file: Option<PathBuf>,
        #[arg(long, env = "RAW_DATA_DIR", default_value = "data/raw")]
        raw_dir: PathBuf,
    },
    /// Setorização de risco do Serviço Geológico do Brasil (API ArcGIS REST).
    SgbRisco {
        #[arg(long, default_value = MVP_MUNICIPALITY)]
        municipality: String,
        /// GeoJSON já baixado, em vez de consultar a API.
        #[arg(long)]
        file: Option<PathBuf>,
        #[arg(long, env = "RAW_DATA_DIR", default_value = "data/raw")]
        raw_dir: PathBuf,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let cli = Cli::parse();
    let pool = crab_persistence::connect(&cli.database_url)
        .await
        .context("conectando ao banco")?;

    match cli.command {
        Command::Migrate => {
            crab_persistence::migrate(&pool).await?;
            tracing::info!("migrations aplicadas");
        }
        Command::Crawl { target } => crawl(target, pool).await?,
        Command::Serve { addr } => {
            let app = crab_api::router(crab_api::AppState::new(pool));
            let listener = tokio::net::TcpListener::bind(&addr).await?;
            tracing::info!(%addr, "API no ar");
            axum::serve(listener, app).await?;
        }
    }
    Ok(())
}

async fn crawl(target: CrawlTarget, pool: sqlx::PgPool) -> anyhow::Result<()> {
    match target {
        CrawlTarget::Listings { file } => {
            let source = FixtureListingSource::new(file);
            let raw = source.fetch().await?;
            let gazetteer = Gazetteer::mvp();
            let repo = ListingRepository::new(pool);
            let total = raw.len();
            for item in raw {
                let listing = normalize_listing(item, DataSource::ListingFixture, &gazetteer);
                if listing.location.municipality_ibge_code.is_none() {
                    tracing::warn!(external_id = %listing.external_id, "município não resolvido");
                }
                repo.upsert(&listing).await?;
            }
            tracing::info!(total, source = source.name(), "anúncios persistidos");
        }
        CrawlTarget::Ibge { municipality } => {
            let http = Arc::new(HttpClient::new(Duration::from_millis(500), 3)?);
            persist_indicators(&IbgeClient::new(http), &municipality, pool).await?;
        }
        CrawlTarget::SspSp { file, municipality } => {
            let scope = match &municipality {
                Some(code) => SecurityScope::municipality("SP", code),
                None => SecurityScope::state("SP"),
            };
            let provider = SspSpCsvProvider::new(file);
            ingest_security(&provider, &scope, &Gazetteer::mvp(), pool).await?;
        }
        CrawlTarget::Sinesp {
            file,
            year,
            state,
            municipality,
            ibge_gazetteer,
        } => {
            let http = Arc::new(HttpClient::new(Duration::from_millis(500), 3)?);
            let input = match (file, year) {
                (Some(path), _) => VdeInput::File(path),
                (None, Some(year)) => VdeInput::Download {
                    year,
                    http: http.clone(),
                },
                (None, None) => anyhow::bail!("informe --file ou --year"),
            };
            let gazetteer = if ibge_gazetteer {
                ibge_gazetteer_for(&state, http).await
            } else {
                Gazetteer::mvp()
            };
            let scope = match &municipality {
                Some(code) => SecurityScope::municipality(&state, code),
                None => SecurityScope::state(&state),
            };
            let provider = SinespVdeProvider::new(input);
            ingest_security(&provider, &scope, &gazetteer, pool).await?;
        }
        CrawlTarget::IbgeSetores { file, scope } => {
            let scope = RegionalScope::parse(&scope)?;
            let provider = IbgeSectorMeshProvider::new(file);
            let repo = RegionalRepository::new(pool);
            run_import(&repo, &provider, &scope, async {
                let batch =
                    normalize_sectors(provider.sectors(&scope).await?, scope.municipality());
                let outcome = repo.store_sectors(&batch).await?;
                Ok((batch.provenance, batch.report, outcome))
            })
            .await?;
        }
        CrawlTarget::IbgeAgregados { file, scope } => {
            let scope = RegionalScope::parse(&scope)?;
            let provider = IbgeBasicAggregatesProvider::new(file);
            ingest_sector_values(&provider, &scope, RegionalRepository::new(pool)).await?;
        }
        CrawlTarget::SeadeIpvs {
            file,
            scope,
            sector_column,
            group_column,
        } => {
            let scope = RegionalScope::parse(&scope)?;
            let provider = SeadeIpvsProvider::new(file, sector_column, group_column);
            ingest_sector_values(&provider, &scope, RegionalRepository::new(pool)).await?;
        }
        CrawlTarget::Geosampa {
            layer,
            file,
            raw_dir,
        } => {
            let layers: Vec<_> = if layer.is_empty() {
                geosampa::LAYERS.iter().collect()
            } else {
                layer
                    .iter()
                    .map(|name| {
                        geosampa::layer(name).ok_or_else(|| {
                            anyhow::anyhow!(
                                "camada desconhecida: {name}. Confirmadas: {}",
                                geosampa::LAYERS
                                    .iter()
                                    .map(|l| l.name)
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            )
                        })
                    })
                    .collect::<anyhow::Result<_>>()?
            };
            if file.is_some() && layers.len() != 1 {
                anyhow::bail!("--file exige exatamente uma --layer");
            }
            let http = Arc::new(HttpClient::new(Duration::from_millis(1000), 3)?);
            let scope = RegionalScope::parse(geosampa::SAO_PAULO)?;
            let repo = RegionalRepository::new(pool);
            let mut failures = 0;
            for layer in layers {
                let input = match &file {
                    Some(path) => GeoSampaInput::File(path.clone()),
                    None => GeoSampaInput::Wfs {
                        http: http.clone(),
                        raw: RawStore::new(&raw_dir),
                    },
                };
                let provider = GeoSampaProvider::new(layer, input);
                let result = run_import(&repo, &provider, &scope, async {
                    let batch = provider.services(&scope).await?;
                    let outcome = repo.store_services(&batch).await?;
                    Ok((batch.provenance, batch.report, outcome))
                })
                .await;
                // Uma camada com problema não impede as outras.
                if let Err(err) = result {
                    failures += 1;
                    tracing::error!(layer = layer.name, error = %format!("{err:#}"), "camada falhou");
                }
            }
            if failures > 0 {
                anyhow::bail!("{failures} camada(s) do GeoSampa falharam; veja o log");
            }
        }
        CrawlTarget::SgbRisco {
            municipality,
            file,
            raw_dir,
        } => {
            let scope = RegionalScope::parse(&municipality)?;
            let input = match file {
                Some(path) => SgbInput::File(path),
                None => SgbInput::Api {
                    http: Arc::new(HttpClient::new(Duration::from_millis(1000), 3)?),
                    raw: RawStore::new(raw_dir),
                },
            };
            let provider = SgbRiskProvider::new(input);
            let repo = RegionalRepository::new(pool);
            run_import(&repo, &provider, &scope, async {
                let batch =
                    normalize_risk_areas(provider.risk_areas(&scope).await?, scope.municipality());
                let outcome = repo.store_risk_areas(&batch).await?;
                Ok((batch.provenance, batch.report, outcome))
            })
            .await?;
        }
    }
    Ok(())
}

/// Registra a importação (início, fim, falha) em `regional_imports`.
async fn run_import(
    repo: &RegionalRepository,
    provider: &dyn RegionalDataProvider,
    scope: &RegionalScope,
    work: impl std::future::Future<
        Output = anyhow::Result<(DatasetProvenance, ImportReport, StoreOutcome)>,
    >,
) -> anyhow::Result<()> {
    let source = provider.descriptor().source.as_str();
    let dataset = provider.dataset_name();
    let id = repo.start_import(source, &dataset, scope.prefix()).await?;
    match work.await {
        Ok((provenance, report, outcome)) => {
            repo.finish_import_ok(id, &provenance, &report, &outcome)
                .await?;
            tracing::info!(
                source,
                dataset,
                version = %provenance.dataset_version,
                read = report.read,
                stored = outcome.stored,
                skipped = report.skipped + outcome.rejected,
                removed = outcome.removed,
                "dataset regional importado"
            );
            if !report.issues.is_empty() || !outcome.issues.is_empty() {
                tracing::warn!(issues = ?report.issues.iter().chain(&outcome.issues).collect::<Vec<_>>(), "registros ignorados");
            }
            Ok(())
        }
        Err(err) => {
            repo.finish_import_failed(id, &format!("{err:#}")).await?;
            Err(err.context(format!("importando {source}/{dataset}")))
        }
    }
}

async fn ingest_sector_values(
    provider: &dyn DemographicDataProvider,
    scope: &RegionalScope,
    repo: RegionalRepository,
) -> anyhow::Result<()> {
    run_import(&repo, provider, scope, async {
        let batch = normalize_sector_values(
            provider.sector_values(scope).await?,
            scope.municipality(),
            |v| provider.indicator_for(v),
        );
        let outcome = repo
            .store_sector_indicators(&batch, provider.capability())
            .await?;
        Ok((batch.provenance, batch.report, outcome))
    })
    .await
}

/// coleta → normalização → persistência, com registro da importação.
async fn ingest_security(
    provider: &dyn SecurityDataProvider,
    scope: &SecurityScope,
    gazetteer: &Gazetteer,
    pool: sqlx::PgPool,
) -> anyhow::Result<()> {
    let source = provider.source();
    let raw = provider
        .fetch(scope)
        .await
        .with_context(|| format!("coletando {}", source.as_str()))?;
    let source_url = raw.first().and_then(|r| r.source_url.clone());
    let dataset_version = raw.first().and_then(|r| r.dataset_version.clone());

    let (stats, report) =
        normalize_crime_records(raw, gazetteer, scope.municipality_ibge_code.as_deref());
    if !report.unmapped_labels.is_empty() {
        tracing::info!(labels = ?report.unmapped_labels, "rótulos sem mapeamento (ignorados)");
    }
    if !report.unresolved_municipalities.is_empty() {
        tracing::warn!(
            municipalities = ?report.unresolved_municipalities,
            "municípios não resolvidos para código IBGE (ignorados)"
        );
    }

    let repo = SecurityRepository::new(pool);
    let stored = repo.upsert_statistics(&stats).await?;
    let scope_label = match &scope.municipality_ibge_code {
        Some(code) => format!("{}/{code}", scope.state),
        None => scope.state.clone(),
    };
    repo.record_import(&DatasetImport {
        source,
        source_url,
        dataset_version,
        scope: scope_label,
        records_read: report.input,
        records_stored: stored,
        records_skipped: report.skipped(),
        report: serde_json::to_value(&report)?,
    })
    .await?;
    tracing::info!(
        source = source.as_str(),
        read = report.input,
        stored,
        skipped = report.skipped(),
        filtered_out = report.filtered_out,
        "estatísticas de segurança persistidas"
    );
    Ok(())
}

async fn ibge_gazetteer_for(uf: &str, http: Arc<HttpClient>) -> Gazetteer {
    let mut gazetteer = Gazetteer::mvp();
    match IbgeClient::new(http).municipalities(uf).await {
        Ok(list) => {
            for m in &list {
                gazetteer.insert(uf, &m.nome, &m.id.to_string());
            }
            tracing::info!(uf, total = list.len(), "municípios carregados do IBGE");
        }
        Err(err) => {
            tracing::warn!(%err, "IBGE indisponível; usando apenas os municípios do MVP")
        }
    }
    gazetteer
}

async fn persist_indicators(
    source: &dyn IndicatorSource,
    municipality: &str,
    pool: sqlx::PgPool,
) -> anyhow::Result<()> {
    let items = source.fetch(municipality).await?;
    let repo = IndicatorRepository::new(pool);
    for item in &items {
        repo.upsert(item).await?;
    }
    tracing::info!(
        total = items.len(),
        source = source.name(),
        "indicadores persistidos"
    );
    Ok(())
}

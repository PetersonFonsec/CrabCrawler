//! Binário único do monólito: CLI para migrar, coletar e servir a API.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use clap::{Parser, Subcommand};
use crab_crawler::http::HttpClient;
use crab_crawler::security::sinesp::{SinespVdeProvider, VdeInput};
use crab_crawler::security::ssp_sp::SspSpCsvProvider;
use crab_crawler::sources::{fixture::FixtureListingSource, ibge::IbgeClient};
use crab_crawler::{IndicatorSource, ListingSource, SecurityDataProvider, SecurityScope};
use crab_domain::DataSource;
use crab_persistence::{DatasetImport, IndicatorRepository, ListingRepository, SecurityRepository};
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
    }
    Ok(())
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

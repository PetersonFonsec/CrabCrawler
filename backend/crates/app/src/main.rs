//! Binário único do monólito: CLI para migrar, coletar e servir a API.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use clap::{Parser, Subcommand};
use crab_crawler::http::HttpClient;
use crab_crawler::sources::{
    fixture::FixtureListingSource, ibge::IbgeClient, ssp_sp::SspSpCsvSource,
};
use crab_crawler::{IndicatorSource, ListingSource};
use crab_domain::DataSource;
use crab_persistence::{IndicatorRepository, ListingRepository};
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
    /// Ocorrências criminais da SSP-SP a partir de CSV.
    SspSp {
        #[arg(long, default_value = "fixtures/ssp_sp_sample.csv")]
        file: PathBuf,
        #[arg(long, default_value = MVP_MUNICIPALITY)]
        municipality: String,
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
            persist_indicators(&SspSpCsvSource::new(file), &municipality, pool).await?;
        }
    }
    Ok(())
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

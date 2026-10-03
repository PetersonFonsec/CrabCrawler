//! Binário único do monólito: CLI para migrar, coletar e servir a API.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use clap::{Parser, Subcommand};
use crab_crawler::geocoding::nominatim::Nominatim;
use crab_crawler::geocoding::viacep::ViaCep;
use crab_crawler::http::HttpClient;
use crab_crawler::http::HttpOptions;
use crab_crawler::property_sources::vrsync::{validate_feed_url, VrsyncInput, VrsyncProvider};
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
use crab_crawler::sources::ibge::IbgeClient;
use crab_crawler::{IndicatorSource, SecurityDataProvider, SecurityScope};
use crab_domain::regional::{DatasetProvenance, ImportReport};
use crab_domain::{PartnerStatus, PartnerType, PropertySource, PropertySourcePartner};
use crab_ingest::fixture::FixtureProvider;
use crab_ingest::{PropertyIngestor, SyncOptions, SyncReport};
use crab_persistence::{
    DatasetImport, IndicatorRepository, PropertyRepository, RegionalRepository, SecurityRepository,
    StoreOutcome,
};
use crab_processing::regional::{normalize_risk_areas, normalize_sector_values, normalize_sectors};
use crab_processing::security::normalize_crime_records;
use crab_processing::{Gazetteer, PropertyNormalizer};
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
        #[command(flatten)]
        enrichment: Enrichment,
    },
    /// Parceiros que fornecem imóveis (imobiliárias, corretores, CRMs).
    Partner {
        #[command(subcommand)]
        action: PartnerAction,
    },
    /// Sincroniza imóveis de um parceiro ou de um feed local.
    Sync {
        #[command(subcommand)]
        target: SyncTarget,
    },
    /// Últimas sincronizações de imóveis, com métricas.
    SyncRuns {
        #[arg(long, default_value_t = 20)]
        limit: i64,
    },
}

/// Enriquecimento de endereço (opcional). Os dois serviços são externos e
/// recebem o endereço do imóvel, por isso ficam desligados por padrão.
#[derive(clap::Args, Clone)]
struct Enrichment {
    /// Completa endereço pelo CEP: `viacep` ou `none`.
    #[arg(long, env = "ADDRESS_LOOKUP", default_value = "none")]
    address_lookup: String,
    /// Endereço → coordenada: `nominatim` ou `none`.
    #[arg(long, env = "GEOCODER", default_value = "none")]
    geocoder: String,
    /// E-mail de contato enviado ao Nominatim (recomendado pela política).
    #[arg(long, env = "NOMINATIM_EMAIL")]
    nominatim_email: Option<String>,
    /// Intervalo entre consultas ao Nominatim. A política pede no máximo 1/s,
    /// e 4/min para scripts recorrentes (use 15000 em sincronizações).
    #[arg(long, env = "GEOCODER_MIN_INTERVAL_MS", default_value_t = 1100)]
    geocoder_min_interval_ms: u64,
}

#[derive(Subcommand)]
enum PartnerAction {
    /// Cadastra um parceiro. Credenciais nunca vão para o banco: informe só
    /// o NOME da variável de ambiente que guarda o cabeçalho de autorização.
    Add {
        /// Identificador curto: letras minúsculas, números e hífen.
        #[arg(long)]
        slug: String,
        #[arg(long)]
        name: String,
        /// REAL_ESTATE_AGENCY, BROKER, CRM ou MARKETPLACE.
        #[arg(long = "type")]
        partner_type: String,
        /// VRSYNC (API: só depois de confirmar a integração; ver docs).
        #[arg(long, default_value = "VRSYNC")]
        provider: String,
        /// URL do feed VRSync autorizado pelo parceiro.
        #[arg(long)]
        feed_url: Option<String>,
        /// Nome da variável de ambiente com o valor do cabeçalho
        /// `Authorization` do feed (ex.: PARTNER_A_FEED_AUTH).
        #[arg(long)]
        authorization_env: Option<String>,
    },
    /// Lista parceiros.
    List,
    /// Muda o status (ACTIVE, PAUSED, DISABLED).
    SetStatus {
        #[arg(long)]
        slug: String,
        #[arg(long)]
        status: String,
    },
}

#[derive(Subcommand)]
enum SyncTarget {
    /// `sync properties --partner <slug>`: um job independente por parceiro.
    Properties {
        #[arg(long)]
        partner: String,
        /// Lê o feed de um arquivo local em vez da URL cadastrada.
        #[arg(long)]
        file: Option<PathBuf>,
        /// Inativa mesmo que mais da metade dos anúncios tenha sumido.
        #[arg(long)]
        allow_mass_deactivation: bool,
        #[command(flatten)]
        enrichment: Enrichment,
    },
    /// Feed VRSync local sem parceiro cadastrado (testes e desenvolvimento).
    Vrsync {
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        allow_mass_deactivation: bool,
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
        // Logs no stderr: o stdout fica com a saída JSON dos comandos.
        .with_writer(std::io::stderr)
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
        Command::Partner { action } => partner(action, pool).await?,
        Command::Sync { target } => sync(target, pool).await?,
        Command::SyncRuns { limit } => {
            for run in PropertyRepository::new(pool).recent_runs(limit).await? {
                println!("{}", serde_json::to_string(&run)?);
            }
        }
        Command::Serve { addr, enrichment } => {
            let ingestor = build_ingestor(&pool, &enrichment)?;
            let app = crab_api::router(crab_api::AppState::with_ingestor(pool, ingestor));
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
            // Mesmo pipeline de qualquer fonte: normaliza, grava imóvel +
            // anúncio + histórico de preço e registra a execução.
            let ingestor = build_ingestor(&pool, &Enrichment::disabled())?;
            let report = ingestor
                .sync(&FixtureProvider::new(file), None, &SyncOptions::default())
                .await?;
            finish_sync(report)?;
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

impl Enrichment {
    fn disabled() -> Self {
        Self {
            address_lookup: "none".into(),
            geocoder: "none".into(),
            nominatim_email: None,
            geocoder_min_interval_ms: 1100,
        }
    }
}

/// Pipeline de ingestão com os serviços de endereço escolhidos.
fn build_ingestor(pool: &sqlx::PgPool, e: &Enrichment) -> anyhow::Result<PropertyIngestor> {
    let mut ingestor = PropertyIngestor::new(
        PropertyRepository::new(pool.clone()),
        PropertyNormalizer::new(Gazetteer::mvp()),
    );
    match e.address_lookup.as_str() {
        "none" => {}
        "viacep" => {
            let http = Arc::new(HttpClient::with_options(HttpOptions {
                min_interval: Duration::from_millis(300),
                max_retries: 2,
                timeout: Duration::from_secs(10),
                ..HttpOptions::default()
            })?);
            ingestor = ingestor.with_postal_lookup(Arc::new(ViaCep::new(http)));
            tracing::info!("consulta de CEP ativada (ViaCEP)");
        }
        other => anyhow::bail!("ADDRESS_LOOKUP desconhecido: {other} (use viacep ou none)"),
    }
    match e.geocoder.as_str() {
        "none" => {}
        "nominatim" => {
            let http = Arc::new(HttpClient::with_options(HttpOptions {
                // A política do Nominatim é de no máximo 1 requisição/s.
                min_interval: Duration::from_millis(e.geocoder_min_interval_ms.max(1000)),
                max_retries: 1,
                timeout: Duration::from_secs(15),
                ..HttpOptions::default()
            })?);
            ingestor =
                ingestor.with_geocoder(Arc::new(Nominatim::new(http, e.nominatim_email.clone())));
            tracing::info!("geocoding ativado (Nominatim/OpenStreetMap)");
        }
        other => anyhow::bail!("GEOCODER desconhecido: {other} (use nominatim ou none)"),
    }
    Ok(ingestor)
}

async fn partner(action: PartnerAction, pool: sqlx::PgPool) -> anyhow::Result<()> {
    let repo = PropertyRepository::new(pool);
    match action {
        PartnerAction::Add {
            slug,
            name,
            partner_type,
            provider,
            feed_url,
            authorization_env,
        } => {
            let partner_type = PartnerType::parse(&partner_type)
                .ok_or_else(|| anyhow::anyhow!("tipo inválido: {partner_type}"))?;
            let provider = PropertySource::parse(&provider)
                .ok_or_else(|| anyhow::anyhow!("provider inválido: {provider}"))?;
            let mut configuration = serde_json::Map::new();
            match provider {
                PropertySource::Vrsync => {
                    let url = feed_url.ok_or_else(|| anyhow::anyhow!("VRSYNC exige --feed-url"))?;
                    validate_feed_url(&url)?;
                    configuration.insert("feed_url".into(), url.into());
                }
                PropertySource::Api => anyhow::bail!(
                    "nenhuma API oficial está integrada ainda; ver docs/property-sources/API.md"
                ),
                PropertySource::Manual | PropertySource::Fixture => {
                    anyhow::bail!("{} não usa parceiro", provider.as_str())
                }
            }
            if let Some(var) = authorization_env {
                let valid = !var.is_empty()
                    && var
                        .chars()
                        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
                if !valid {
                    anyhow::bail!("--authorization-env deve ser o NOME da variável (ex.: PARTNER_A_FEED_AUTH)");
                }
                configuration.insert("authorization_env".into(), var.into());
            }
            let now = chrono::Utc::now();
            let p = PropertySourcePartner {
                id: uuid::Uuid::new_v4(),
                slug,
                name,
                partner_type,
                status: PartnerStatus::Active,
                provider,
                configuration: serde_json::Value::Object(configuration),
                created_at: now,
                updated_at: now,
            };
            repo.create_partner(&p).await?;
            println!("{}", serde_json::to_string(&p)?);
        }
        PartnerAction::List => {
            for p in repo.partners().await? {
                println!("{}", serde_json::to_string(&p)?);
            }
        }
        PartnerAction::SetStatus { slug, status } => {
            let status = PartnerStatus::parse(&status)
                .ok_or_else(|| anyhow::anyhow!("status inválido: {status}"))?;
            if !repo.set_partner_status(&slug, status).await? {
                anyhow::bail!("parceiro não encontrado: {slug}");
            }
        }
    }
    Ok(())
}

async fn sync(target: SyncTarget, pool: sqlx::PgPool) -> anyhow::Result<()> {
    match target {
        SyncTarget::Properties {
            partner,
            file,
            allow_mass_deactivation,
            enrichment,
        } => {
            let repo = PropertyRepository::new(pool.clone());
            let p = repo
                .partner(&partner)
                .await?
                .ok_or_else(|| anyhow::anyhow!("parceiro não encontrado: {partner}"))?;
            if p.status != PartnerStatus::Active {
                anyhow::bail!("parceiro {} está {}", p.slug, p.status.as_str());
            }
            let provider = match p.provider {
                PropertySource::Vrsync => {
                    let input = match file {
                        Some(path) => VrsyncInput::File(path),
                        None => {
                            let url = p.configuration["feed_url"]
                                .as_str()
                                .ok_or_else(|| anyhow::anyhow!("parceiro sem feed_url"))?
                                .to_string();
                            // O segredo é lido só agora, do ambiente.
                            let authorization = match p.configuration["authorization_env"].as_str()
                            {
                                Some(var) => Some(std::env::var(var).with_context(|| {
                                    format!("variável de ambiente {var} não definida")
                                })?),
                                None => None,
                            };
                            VrsyncInput::Url {
                                http: Arc::new(HttpClient::with_options(HttpOptions {
                                    min_interval: Duration::from_millis(1000),
                                    max_retries: 2,
                                    timeout: Duration::from_secs(20 * 60),
                                    ..HttpOptions::default()
                                })?),
                                url,
                                authorization,
                            }
                        }
                    };
                    VrsyncProvider::new(input, Some(p.id))
                }
                other => anyhow::bail!("sincronização de {} não implementada", other.as_str()),
            };
            let ingestor = build_ingestor(&pool, &enrichment)?;
            let options = SyncOptions {
                allow_mass_deactivation,
                ..SyncOptions::default()
            };
            finish_sync(ingestor.sync(&provider, Some(&p), &options).await?)?;
        }
        SyncTarget::Vrsync {
            file,
            allow_mass_deactivation,
        } => {
            let ingestor = build_ingestor(&pool, &Enrichment::disabled())?;
            let options = SyncOptions {
                allow_mass_deactivation,
                ..SyncOptions::default()
            };
            let provider = VrsyncProvider::new(VrsyncInput::File(file), None);
            finish_sync(ingestor.sync(&provider, None, &options).await?)?;
        }
    }
    Ok(())
}

/// Imprime o relatório e falha o processo se a fonte inteira falhou.
fn finish_sync(report: SyncReport) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(&report)?);
    if report.status == crab_domain::SyncRunStatus::Failed {
        anyhow::bail!("sincronização {} falhou", report.run_id);
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

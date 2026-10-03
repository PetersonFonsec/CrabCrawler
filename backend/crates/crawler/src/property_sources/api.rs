//! Infraestrutura genérica para APIs oficiais de parceiros.
//!
//! Nenhuma API real está integrada: cada integração só entra depois de
//! confirmar documentação, autenticação, permissões, limites e termos (ver
//! `docs/property-sources/API.md`). Este módulo dá a base comum:
//!
//! - timeout por requisição e retry com backoff exponencial, com teto;
//! - rate limiting (intervalo mínimo entre chamadas);
//! - 429 com `Retry-After` respeitado até um limite; acima dele, erro;
//! - indisponibilidade (5xx, rede) com novas tentativas limitadas;
//! - credencial lida de variável de ambiente na hora da chamada, enviada em
//!   cabeçalho e nunca registrada em log;
//! - mapeamento do JSON da API para [`RawProperty`] por integração.
//!
//! Não há fallback para scraping: se a API não oferece um dado, ele fica
//! ausente e a limitação é documentada.

use std::time::Duration;

use crab_domain::RawProperty;
use reqwest::header::{HeaderName, HeaderValue, AUTHORIZATION};
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::ItemReadError;
use crate::http::{HttpClient, HttpOptions};
use crate::CrawlError;

/// Como a API autentica. Só o **nome** da variável de ambiente fica na
/// configuração do parceiro.
#[derive(Debug, Clone, PartialEq)]
pub enum ApiAuth {
    None,
    /// `Authorization: Bearer <valor da variável>`.
    BearerFromEnv {
        env_var: String,
    },
    /// Cabeçalho próprio (ex.: `X-Api-Key: <valor da variável>`).
    HeaderFromEnv {
        header: String,
        env_var: String,
    },
}

#[derive(Debug, Clone)]
pub struct ApiSourceConfig {
    pub base_url: String,
    pub auth: ApiAuth,
    pub http: HttpOptions,
}

impl ApiSourceConfig {
    /// Padrões conservadores para APIs de terceiros.
    pub fn new(base_url: impl Into<String>, auth: ApiAuth) -> Self {
        Self {
            base_url: base_url.into(),
            auth,
            http: HttpOptions {
                min_interval: Duration::from_millis(1000),
                max_retries: 3,
                timeout: Duration::from_secs(20),
                max_retry_after: Duration::from_secs(60),
                base_backoff: Duration::from_millis(500),
            },
        }
    }
}

/// Cliente de uma API oficial.
pub struct ApiClient {
    http: HttpClient,
    base: url::Url,
    auth: ApiAuth,
}

impl ApiClient {
    pub fn new(config: ApiSourceConfig) -> Result<Self, CrawlError> {
        let base = url::Url::parse(&config.base_url)
            .map_err(|_| CrawlError::Config("base_url inválida".into()))?;
        if base.scheme() != "https" && !is_loopback(&base) {
            return Err(CrawlError::Config("APIs externas precisam de https".into()));
        }
        Ok(Self {
            http: HttpClient::with_options(config.http)?,
            base,
            auth: config.auth,
        })
    }

    fn headers(&self) -> Result<Vec<(HeaderName, HeaderValue)>, CrawlError> {
        let read = |var: &str| {
            std::env::var(var)
                .ok()
                .filter(|v| !v.trim().is_empty())
                .ok_or_else(|| {
                    CrawlError::Config(format!("variável de ambiente {var} não definida"))
                })
        };
        let sensitive = |value: String| {
            let mut v = HeaderValue::from_str(&value)
                .map_err(|_| CrawlError::Config("credencial com caracteres inválidos".into()))?;
            v.set_sensitive(true);
            Ok::<_, CrawlError>(v)
        };
        Ok(match &self.auth {
            ApiAuth::None => vec![],
            ApiAuth::BearerFromEnv { env_var } => {
                vec![(
                    AUTHORIZATION,
                    sensitive(format!("Bearer {}", read(env_var)?))?,
                )]
            }
            ApiAuth::HeaderFromEnv { header, env_var } => {
                let name = HeaderName::from_bytes(header.as_bytes())
                    .map_err(|_| CrawlError::Config(format!("cabeçalho inválido: {header}")))?;
                vec![(name, sensitive(read(env_var)?)?)]
            }
        })
    }

    /// `GET base_url/path?query` e decodifica o JSON.
    pub async fn get_json<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<T, CrawlError> {
        let mut url = self
            .base
            .join(path.trim_start_matches('/'))
            .map_err(|_| CrawlError::Config(format!("caminho inválido: {path}")))?;
        if !query.is_empty() {
            url.query_pairs_mut().extend_pairs(query);
        }
        self.http
            .get_json_with(url.as_str(), &self.headers()?)
            .await
    }
}

fn is_loopback(url: &url::Url) -> bool {
    matches!(
        url.host_str(),
        Some("127.0.0.1") | Some("localhost") | Some("[::1]")
    )
}

/// Converte um item do JSON de uma API específica em [`RawProperty`].
/// Cada integração real implementa o seu.
pub trait ApiMapping: Send + Sync {
    fn map(&self, item: &Value) -> Result<RawProperty, ItemReadError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    use axum::extract::State;
    use axum::http::{HeaderMap, StatusCode};
    use axum::response::IntoResponse;
    use axum::routing::get;
    use axum::Router;

    #[derive(Clone, Default)]
    struct Hits(Arc<AtomicU32>);

    async fn serve(router: Router) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        format!("http://{addr}/")
    }

    fn fast(base: String, auth: ApiAuth) -> ApiClient {
        ApiClient::new(ApiSourceConfig {
            base_url: base,
            auth,
            http: HttpOptions {
                min_interval: Duration::from_millis(0),
                max_retries: 2,
                timeout: Duration::from_millis(300),
                max_retry_after: Duration::from_secs(2),
                base_backoff: Duration::from_millis(10),
            },
        })
        .unwrap()
    }

    async fn flaky(State(h): State<Hits>) -> impl IntoResponse {
        if h.0.fetch_add(1, Ordering::SeqCst) == 0 {
            (StatusCode::SERVICE_UNAVAILABLE, "fora").into_response()
        } else {
            axum::Json(serde_json::json!({"ok": true})).into_response()
        }
    }

    async fn limited(State(h): State<Hits>) -> impl IntoResponse {
        if h.0.fetch_add(1, Ordering::SeqCst) == 0 {
            (
                StatusCode::TOO_MANY_REQUESTS,
                [("retry-after", "1")],
                "devagar",
            )
                .into_response()
        } else {
            axum::Json(serde_json::json!({"ok": true})).into_response()
        }
    }

    async fn auth(headers: HeaderMap) -> impl IntoResponse {
        match headers.get("authorization").and_then(|v| v.to_str().ok()) {
            Some("Bearer segredo-de-teste") => {
                axum::Json(serde_json::json!({"ok": true})).into_response()
            }
            _ => StatusCode::UNAUTHORIZED.into_response(),
        }
    }

    fn router(hits: Hits) -> Router {
        Router::new()
            .route("/flaky", get(flaky))
            .route("/limited", get(limited))
            .route(
                "/slow",
                get(|| async {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    "tarde"
                }),
            )
            .route(
                "/blocked",
                get(|| async {
                    (
                        StatusCode::TOO_MANY_REQUESTS,
                        [("retry-after", "3600")],
                        "amanhã",
                    )
                }),
            )
            .route("/down", get(|| async { StatusCode::BAD_GATEWAY }))
            .route("/garbage", get(|| async { "<html>não é json" }))
            .route("/auth", get(auth))
            .with_state(hits)
    }

    #[tokio::test]
    async fn retries_unavailable_then_succeeds() {
        let hits = Hits::default();
        let client = fast(serve(router(hits.clone())).await, ApiAuth::None);
        let v: Value = client.get_json("flaky", &[]).await.unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(hits.0.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn honours_retry_after_on_429() {
        let hits = Hits::default();
        let client = fast(serve(router(hits.clone())).await, ApiAuth::None);
        let start = std::time::Instant::now();
        let v: Value = client.get_json("limited", &[]).await.unwrap();
        assert_eq!(v["ok"], true);
        assert!(start.elapsed() >= Duration::from_millis(900));
    }

    #[tokio::test]
    async fn gives_up_when_retry_after_is_too_long() {
        let client = fast(serve(router(Hits::default())).await, ApiAuth::None);
        let err = client.get_json::<Value>("blocked", &[]).await.unwrap_err();
        assert!(
            matches!(
                err,
                CrawlError::RateLimited {
                    retry_after_secs: Some(3600),
                    ..
                }
            ),
            "{err:?}"
        );
    }

    #[tokio::test]
    async fn timeout_is_reported() {
        let client = fast(serve(router(Hits::default())).await, ApiAuth::None);
        let err = client.get_json::<Value>("slow", &[]).await.unwrap_err();
        assert!(
            matches!(err, CrawlError::Timeout { .. } | CrawlError::Http(_)),
            "{err:?}"
        );
    }

    #[tokio::test]
    async fn remote_error_after_bounded_retries() {
        let client = fast(serve(router(Hits::default())).await, ApiAuth::None);
        let err = client.get_json::<Value>("down", &[]).await.unwrap_err();
        assert!(
            matches!(err, CrawlError::Status { status: 502, .. }),
            "{err:?}"
        );
    }

    #[tokio::test]
    async fn invalid_data_is_a_parse_error() {
        let client = fast(serve(router(Hits::default())).await, ApiAuth::None);
        let err = client.get_json::<Value>("garbage", &[]).await.unwrap_err();
        assert!(matches!(err, CrawlError::Parse(_)), "{err:?}");
    }

    #[tokio::test]
    async fn credential_comes_from_env_and_is_required() {
        let base = serve(router(Hits::default())).await;
        let missing = fast(
            base.clone(),
            ApiAuth::BearerFromEnv {
                env_var: "CRAB_TEST_TOKEN_AUSENTE".into(),
            },
        );
        let err = missing.get_json::<Value>("auth", &[]).await.unwrap_err();
        assert!(err.to_string().contains("CRAB_TEST_TOKEN_AUSENTE"));

        std::env::set_var("CRAB_TEST_TOKEN", "segredo-de-teste");
        let ok = fast(
            base,
            ApiAuth::BearerFromEnv {
                env_var: "CRAB_TEST_TOKEN".into(),
            },
        );
        let v: Value = ok.get_json("auth", &[]).await.unwrap();
        assert_eq!(v["ok"], true);
    }

    #[test]
    fn requires_https_for_remote_hosts() {
        let cfg = ApiSourceConfig::new("http://api.example.com/", ApiAuth::None);
        assert!(ApiClient::new(cfg).is_err());
    }
}

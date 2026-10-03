use std::time::Duration;

use reqwest::StatusCode;
use serde::de::DeserializeOwned;
use tokio::sync::Mutex;
use tokio::time::Instant;

use crate::CrawlError;

/// Cliente HTTP compartilhado pelos crawlers.
///
/// - rate limiting: no máximo uma requisição a cada `min_interval`;
/// - retry: até `max_retries` novas tentativas com backoff exponencial em
///   erros de rede, 429 e 5xx;
/// - logs: cada tentativa é registrada via `tracing`.
pub struct HttpClient {
    inner: reqwest::Client,
    min_interval: Duration,
    max_retries: u32,
    last_request: Mutex<Option<Instant>>,
}

impl HttpClient {
    pub fn new(min_interval: Duration, max_retries: u32) -> Result<Self, CrawlError> {
        let inner = reqwest::Client::builder()
            .user_agent(concat!(
                "CrabCrawler/",
                env!("CARGO_PKG_VERSION"),
                " (+https://github.com/PetersonFonsec/CrabCrawler)"
            ))
            .timeout(Duration::from_secs(30))
            .build()?;
        Ok(Self {
            inner,
            min_interval,
            max_retries,
            last_request: Mutex::new(None),
        })
    }

    pub async fn get_json<T: DeserializeOwned>(&self, url: &str) -> Result<T, CrawlError> {
        let body = self.get_text(url).await?;
        Ok(serde_json::from_str(&body)?)
    }

    pub async fn get_text(&self, url: &str) -> Result<String, CrawlError> {
        Ok(self.send(url).await?.text().await?)
    }

    /// Download binário (planilhas XLSX, por exemplo).
    pub async fn get_bytes(&self, url: &str) -> Result<Vec<u8>, CrawlError> {
        Ok(self.send(url).await?.bytes().await?.to_vec())
    }

    async fn send(&self, url: &str) -> Result<reqwest::Response, CrawlError> {
        let mut attempt = 0;
        loop {
            self.wait_turn().await;
            tracing::debug!(url, attempt, "GET");
            let result = self.inner.get(url).send().await;
            let retryable = match result {
                Ok(resp) if resp.status().is_success() => return Ok(resp),
                Ok(resp) => {
                    let status = resp.status();
                    if !is_retryable(status) || attempt >= self.max_retries {
                        return Err(CrawlError::Status {
                            url: url.to_string(),
                            status: status.as_u16(),
                        });
                    }
                    format!("status {status}")
                }
                Err(err)
                    if attempt < self.max_retries && (err.is_timeout() || err.is_connect()) =>
                {
                    err.to_string()
                }
                Err(err) => return Err(err.into()),
            };
            let backoff = Duration::from_millis(500 * 2u64.pow(attempt));
            tracing::warn!(url, attempt, reason = %retryable, ?backoff, "tentando novamente");
            tokio::time::sleep(backoff).await;
            attempt += 1;
        }
    }

    async fn wait_turn(&self) {
        let mut last = self.last_request.lock().await;
        if let Some(prev) = *last {
            let next = prev + self.min_interval;
            if next > Instant::now() {
                tokio::time::sleep_until(next).await;
            }
        }
        *last = Some(Instant::now());
    }
}

fn is_retryable(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

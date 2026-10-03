use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderName, HeaderValue, RETRY_AFTER};
use reqwest::StatusCode;
use serde::de::DeserializeOwned;
use tokio::sync::Mutex;
use tokio::time::Instant;

use crate::CrawlError;

/// Opções do cliente HTTP. Os padrões são os que os crawlers já usavam.
#[derive(Debug, Clone)]
pub struct HttpOptions {
    /// Intervalo mínimo entre requisições (rate limiting).
    pub min_interval: Duration,
    /// Novas tentativas após a primeira, em erros de rede, 429 e 5xx.
    pub max_retries: u32,
    /// Tempo máximo de cada requisição, incluindo o corpo.
    pub timeout: Duration,
    /// Maior espera aceita num `Retry-After`. Acima disso a chamada falha
    /// com [`CrawlError::RateLimited`] em vez de ficar parada.
    pub max_retry_after: Duration,
    /// Primeira espera do backoff exponencial.
    pub base_backoff: Duration,
}

impl Default for HttpOptions {
    fn default() -> Self {
        Self {
            min_interval: Duration::from_millis(500),
            max_retries: 3,
            timeout: Duration::from_secs(30),
            max_retry_after: Duration::from_secs(120),
            base_backoff: Duration::from_millis(500),
        }
    }
}

/// Cliente HTTP compartilhado pelos crawlers.
///
/// - rate limiting: no máximo uma requisição a cada `min_interval`;
/// - retry: até `max_retries` novas tentativas com backoff exponencial em
///   erros de rede, 429 e 5xx; um `Retry-After` em segundos é respeitado
///   até `max_retry_after`. Nunca tenta indefinidamente;
/// - logs: cada tentativa é registrada via `tracing`, com a query string
///   mascarada (tokens nunca vão para o log).
pub struct HttpClient {
    inner: reqwest::Client,
    options: HttpOptions,
    last_request: Mutex<Option<Instant>>,
}

impl HttpClient {
    pub fn new(min_interval: Duration, max_retries: u32) -> Result<Self, CrawlError> {
        Self::with_options(HttpOptions {
            min_interval,
            max_retries,
            ..HttpOptions::default()
        })
    }

    pub fn with_options(options: HttpOptions) -> Result<Self, CrawlError> {
        let inner = reqwest::Client::builder()
            .user_agent(concat!(
                "CrabCrawler/",
                env!("CARGO_PKG_VERSION"),
                " (+https://github.com/PetersonFonsec/CrabCrawler)"
            ))
            .timeout(options.timeout)
            .connect_timeout(options.timeout.min(Duration::from_secs(15)))
            .build()?;
        Ok(Self {
            inner,
            options,
            last_request: Mutex::new(None),
        })
    }

    pub async fn get_json<T: DeserializeOwned>(&self, url: &str) -> Result<T, CrawlError> {
        let body = self.get_text(url).await?;
        Ok(serde_json::from_str(&body)?)
    }

    /// GET com cabeçalhos extras (ex.: autenticação). Os valores dos
    /// cabeçalhos nunca são registrados.
    pub async fn get_json_with<T: DeserializeOwned>(
        &self,
        url: &str,
        headers: &[(HeaderName, HeaderValue)],
    ) -> Result<T, CrawlError> {
        let resp = self.send(url, headers).await?;
        let body = resp.text().await.map_err(|e| map_reqwest(url, e))?;
        serde_json::from_str(&body).map_err(|e| CrawlError::Parse(format!("JSON inválido: {e}")))
    }

    pub async fn get_text(&self, url: &str) -> Result<String, CrawlError> {
        let resp = self.send(url, &[]).await?;
        resp.text().await.map_err(|e| map_reqwest(url, e))
    }

    /// Download binário (planilhas XLSX, por exemplo).
    pub async fn get_bytes(&self, url: &str) -> Result<Vec<u8>, CrawlError> {
        let resp = self.send(url, &[]).await?;
        Ok(resp
            .bytes()
            .await
            .map_err(|e| map_reqwest(url, e))?
            .to_vec())
    }

    /// Download com limite de tamanho: para assim que passar de `max_bytes`,
    /// sem carregar o resto (feeds de terceiros são entrada não confiável).
    pub async fn get_bytes_limited(
        &self,
        url: &str,
        headers: &[(HeaderName, HeaderValue)],
        max_bytes: usize,
    ) -> Result<Vec<u8>, CrawlError> {
        let mut resp = self.send(url, headers).await?;
        if resp
            .content_length()
            .is_some_and(|len| len as usize > max_bytes)
        {
            return Err(CrawlError::TooLarge { max_bytes });
        }
        let mut body = Vec::new();
        while let Some(chunk) = resp.chunk().await.map_err(|e| map_reqwest(url, e))? {
            if body.len() + chunk.len() > max_bytes {
                return Err(CrawlError::TooLarge { max_bytes });
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }

    async fn send(
        &self,
        url: &str,
        headers: &[(HeaderName, HeaderValue)],
    ) -> Result<reqwest::Response, CrawlError> {
        let safe_url = redact_url(url);
        let mut header_map = HeaderMap::new();
        for (name, value) in headers {
            header_map.insert(name.clone(), value.clone());
        }
        let mut attempt = 0;
        loop {
            self.wait_turn().await;
            tracing::debug!(url = %safe_url, attempt, "GET");
            let result = self.inner.get(url).headers(header_map.clone()).send().await;
            let (reason, wait) = match result {
                Ok(resp) if resp.status().is_success() => return Ok(resp),
                Ok(resp) => {
                    let status = resp.status();
                    let retry_after = retry_after(resp.headers());
                    if status == StatusCode::TOO_MANY_REQUESTS {
                        let too_long =
                            retry_after.is_some_and(|d| d > self.options.max_retry_after);
                        if too_long || attempt >= self.options.max_retries {
                            tracing::warn!(url = %safe_url, ?retry_after, "limite de requisições atingido");
                            return Err(CrawlError::RateLimited {
                                url: safe_url,
                                retry_after_secs: retry_after.map(|d| d.as_secs()),
                            });
                        }
                    } else if !is_retryable(status) || attempt >= self.options.max_retries {
                        return Err(CrawlError::Status {
                            url: safe_url,
                            status: status.as_u16(),
                        });
                    }
                    (format!("status {status}"), retry_after)
                }
                Err(err)
                    if attempt < self.options.max_retries
                        && (err.is_timeout() || err.is_connect()) =>
                {
                    (err.without_url().to_string(), None)
                }
                Err(err) => return Err(map_reqwest(url, err)),
            };
            let backoff = wait.unwrap_or(self.options.base_backoff * 2u32.pow(attempt));
            tracing::warn!(url = %safe_url, attempt, reason = %reason, ?backoff, "tentando novamente");
            tokio::time::sleep(backoff).await;
            attempt += 1;
        }
    }

    async fn wait_turn(&self) {
        let mut last = self.last_request.lock().await;
        if let Some(prev) = *last {
            let next = prev + self.options.min_interval;
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

/// `Retry-After` em segundos. A forma com data HTTP não é interpretada (o
/// backoff padrão vale nesse caso).
fn retry_after(headers: &HeaderMap) -> Option<Duration> {
    let value = headers.get(RETRY_AFTER)?.to_str().ok()?;
    value.trim().parse::<u64>().ok().map(Duration::from_secs)
}

fn map_reqwest(url: &str, err: reqwest::Error) -> CrawlError {
    if err.is_timeout() {
        CrawlError::Timeout {
            url: redact_url(url),
        }
    } else {
        CrawlError::Http(err.without_url())
    }
}

/// Remove valores da query string e credenciais da URL antes de logar:
/// `https://x.com/a?token=abc&page=2` → `https://x.com/a?token=***&page=***`.
pub fn redact_url(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(mut parsed) => {
            let _ = parsed.set_username("");
            let _ = parsed.set_password(None);
            let keys: Vec<String> = parsed.query_pairs().map(|(k, _)| k.into_owned()).collect();
            if keys.is_empty() {
                parsed.set_query(None);
            } else {
                let masked: Vec<String> = keys.iter().map(|k| format!("{k}=***")).collect();
                parsed.set_query(Some(&masked.join("&")));
            }
            parsed.to_string()
        }
        Err(_) => "<url inválida>".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_query_and_credentials() {
        assert_eq!(
            redact_url("https://user:pw@api.example.com/v1/items?token=abc&page=2"),
            "https://api.example.com/v1/items?token=***&page=***"
        );
        assert_eq!(redact_url("https://a.com/x"), "https://a.com/x");
        assert_eq!(redact_url("nada"), "<url inválida>");
    }
}

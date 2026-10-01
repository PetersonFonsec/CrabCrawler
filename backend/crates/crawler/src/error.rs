#[derive(Debug, thiserror::Error)]
pub enum CrawlError {
    #[error("falha HTTP: {0}")]
    Http(#[from] reqwest::Error),
    #[error("resposta inesperada de {url}: status {status}")]
    Status { url: String, status: u16 },
    #[error("falha ao ler arquivo: {0}")]
    Io(#[from] std::io::Error),
    #[error("falha ao interpretar dados: {0}")]
    Parse(String),
}

impl From<serde_json::Error> for CrawlError {
    fn from(err: serde_json::Error) -> Self {
        Self::Parse(err.to_string())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CrawlError {
    #[error("falha HTTP: {0}")]
    Http(#[from] reqwest::Error),
    #[error("resposta inesperada de {url}: status {status}")]
    Status { url: String, status: u16 },
    #[error("tempo esgotado em {url}")]
    Timeout { url: String },
    #[error("limite de requisições atingido em {url} (Retry-After: {retry_after_secs:?}s)")]
    RateLimited {
        url: String,
        retry_after_secs: Option<u64>,
    },
    #[error("conteúdo maior que o limite de {max_bytes} bytes")]
    TooLarge { max_bytes: usize },
    #[error("operação não suportada por este provider: {0}")]
    Unsupported(&'static str),
    #[error("configuração inválida: {0}")]
    Config(String),
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

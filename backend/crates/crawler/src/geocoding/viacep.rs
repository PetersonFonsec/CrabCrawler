//! ViaCEP (viacep.com.br): CEP → endereço e código IBGE do município.
//!
//! Conferido em 2026-10-03: `GET /ws/{cep}/json/` com 8 dígitos; formato
//! inválido devolve 400; CEP inexistente devolve `{"erro": "true"}`. A
//! página avisa que uso massivo para validar bases locais pode bloquear o
//! acesso, então o CrabCrawler só consulta CEPs de imóveis que precisam.

use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;

use super::{PostalAddress, PostalCodeLookup};
use crate::http::HttpClient;
use crate::CrawlError;

pub const BASE_URL: &str = "https://viacep.com.br";

pub struct ViaCep {
    http: Arc<HttpClient>,
    base_url: String,
}

impl ViaCep {
    pub fn new(http: Arc<HttpClient>) -> Self {
        Self::with_base_url(http, BASE_URL)
    }

    pub fn with_base_url(http: Arc<HttpClient>, base_url: impl Into<String>) -> Self {
        Self {
            http,
            base_url: base_url.into().trim_end_matches('/').to_string(),
        }
    }
}

#[derive(Deserialize)]
struct ViaCepResponse {
    #[serde(default)]
    erro: Option<serde_json::Value>,
    cep: Option<String>,
    logradouro: Option<String>,
    bairro: Option<String>,
    localidade: Option<String>,
    uf: Option<String>,
    ibge: Option<String>,
}

fn non_empty(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

#[async_trait]
impl PostalCodeLookup for ViaCep {
    fn name(&self) -> &'static str {
        "viacep"
    }

    async fn lookup(&self, postal_code: &str) -> Result<Option<PostalAddress>, CrawlError> {
        if postal_code.len() != 8 || !postal_code.chars().all(|c| c.is_ascii_digit()) {
            return Ok(None);
        }
        let url = format!("{}/ws/{postal_code}/json/", self.base_url);
        let resp: ViaCepResponse = match self.http.get_json(&url).await {
            Ok(r) => r,
            Err(CrawlError::Status { status: 400, .. }) => return Ok(None),
            Err(e) => return Err(e),
        };
        if resp.erro.is_some() {
            return Ok(None);
        }
        let ibge =
            non_empty(resp.ibge).filter(|c| c.len() == 7 && c.chars().all(|d| d.is_ascii_digit()));
        Ok(Some(PostalAddress {
            postal_code: non_empty(resp.cep)
                .map(|c| c.chars().filter(char::is_ascii_digit).collect())
                .unwrap_or_else(|| postal_code.to_string()),
            street: non_empty(resp.logradouro),
            neighborhood: non_empty(resp.bairro),
            municipality: non_empty(resp.localidade),
            state: non_empty(resp.uf),
            municipality_ibge_code: ibge,
        }))
    }
}

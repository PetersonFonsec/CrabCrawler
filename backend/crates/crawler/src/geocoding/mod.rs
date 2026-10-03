//! Geocoding desacoplado das fontes de imóveis.
//!
//! Dois passos independentes, ambos opcionais:
//! 1. [`PostalCodeLookup`]: CEP → logradouro, bairro, município, UF e código
//!    IBGE (ViaCEP). Completa endereços que só têm CEP;
//! 2. [`Geocoder`]: endereço → coordenada, com precisão (Nominatim/OSM).
//!
//! Nenhum provider de imóveis chama geocoding: quem decide é o pipeline de
//! ingestão, depois da normalização, e só quando a fonte não mandou
//! coordenada.

pub mod nominatim;
pub mod viacep;

use async_trait::async_trait;
use crab_domain::{Address, CoordinatePrecision, GeoPoint};
use serde::Serialize;

use crate::CrawlError;

/// Endereço a geocodificar.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct GeocodeQuery {
    pub street: Option<String>,
    pub number: Option<String>,
    pub neighborhood: Option<String>,
    pub municipality: Option<String>,
    pub state: Option<String>,
    pub postal_code: Option<String>,
}

impl GeocodeQuery {
    pub fn from_address(a: &Address) -> Self {
        Self {
            street: a.street.clone(),
            number: a.number.clone(),
            neighborhood: a.neighborhood.clone(),
            municipality: a.municipality.clone(),
            state: a.state.clone(),
            postal_code: a.postal_code.clone(),
        }
    }

    /// Só vale a pena consultar com rua, município e UF.
    pub fn is_usable(&self) -> bool {
        self.street.is_some() && self.municipality.is_some() && self.state.is_some()
    }

    /// Chave estável para cache (sem acento, minúscula).
    pub fn cache_key(&self, provider: &str) -> String {
        let part = |v: &Option<String>| {
            v.as_deref()
                .map(|s| {
                    s.to_lowercase()
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_default()
        };
        format!(
            "{provider}|{}|{}|{}|{}|{}",
            part(&self.street),
            part(&self.number),
            part(&self.municipality),
            part(&self.state),
            part(&self.postal_code)
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GeocodeResult {
    pub point: GeoPoint,
    pub precision: CoordinatePrecision,
    /// Descrição devolvida pelo serviço, para conferência.
    pub label: Option<String>,
}

#[async_trait]
pub trait Geocoder: Send + Sync {
    fn name(&self) -> &'static str;
    async fn geocode(&self, query: &GeocodeQuery) -> Result<Option<GeocodeResult>, CrawlError>;
}

/// Endereço oficial de um CEP.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct PostalAddress {
    pub postal_code: String,
    pub street: Option<String>,
    pub neighborhood: Option<String>,
    pub municipality: Option<String>,
    pub state: Option<String>,
    pub municipality_ibge_code: Option<String>,
}

#[async_trait]
pub trait PostalCodeLookup: Send + Sync {
    fn name(&self) -> &'static str;
    /// `Ok(None)` quando o CEP não existe.
    async fn lookup(&self, postal_code: &str) -> Result<Option<PostalAddress>, CrawlError>;
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use axum::extract::Path;
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use axum::routing::get;
    use axum::Router;

    use super::nominatim::Nominatim;
    use super::viacep::ViaCep;
    use super::*;
    use crate::http::{HttpClient, HttpOptions};

    async fn serve(router: Router) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        format!("http://{addr}")
    }

    fn http() -> Arc<HttpClient> {
        Arc::new(
            HttpClient::with_options(HttpOptions {
                min_interval: Duration::from_millis(0),
                max_retries: 0,
                ..HttpOptions::default()
            })
            .unwrap(),
        )
    }

    async fn viacep(Path(cep): Path<String>) -> impl IntoResponse {
        match cep.as_str() {
            "09640000" => axum::Json(serde_json::json!({
                "cep": "09640-000", "logradouro": "Rua Exemplo", "bairro": "Rudge Ramos",
                "localidade": "São Bernardo do Campo", "uf": "SP", "ibge": "3548708"
            }))
            .into_response(),
            "99999999" => axum::Json(serde_json::json!({ "erro": "true" })).into_response(),
            _ => StatusCode::BAD_REQUEST.into_response(),
        }
    }

    #[tokio::test]
    async fn viacep_lookup() {
        let base = serve(Router::new().route("/ws/{cep}/json/", get(viacep))).await;
        let client = ViaCep::with_base_url(http(), base);
        let found = client.lookup("09640000").await.unwrap().unwrap();
        assert_eq!(found.municipality_ibge_code.as_deref(), Some("3548708"));
        assert_eq!(found.postal_code, "09640000");
        assert_eq!(client.lookup("99999999").await.unwrap(), None);
        assert_eq!(client.lookup("12345678").await.unwrap(), None);
        assert_eq!(client.lookup("123").await.unwrap(), None);
    }

    #[tokio::test]
    async fn nominatim_geocode_and_precision() {
        let router = Router::new().route(
            "/search",
            get(
                |q: axum::extract::Query<std::collections::HashMap<String, String>>| async move {
                    assert_eq!(q.get("countrycodes").map(String::as_str), Some("br"));
                    if q.get("street").is_some_and(|s| s.contains("Inexistente")) {
                        return axum::Json(serde_json::json!([]));
                    }
                    axum::Json(serde_json::json!([{
                        "lat": "-23.6560", "lon": "-46.5730", "display_name": "Rua Exemplo, 100",
                        "place_rank": 30, "addresstype": "building"
                    }]))
                },
            ),
        );
        let base = serve(router).await;
        let g = Nominatim::with_base_url(http(), base, None);
        let mut q = GeocodeQuery {
            street: Some("Rua Exemplo".into()),
            number: Some("100".into()),
            municipality: Some("São Bernardo do Campo".into()),
            state: Some("SP".into()),
            ..Default::default()
        };
        let r = g.geocode(&q).await.unwrap().unwrap();
        assert_eq!(r.precision, CoordinatePrecision::Exact);
        q.number = None;
        let r = g.geocode(&q).await.unwrap().unwrap();
        assert_eq!(
            r.precision,
            CoordinatePrecision::Street,
            "sem número não é exato"
        );
        q.street = Some("Rua Inexistente".into());
        assert_eq!(g.geocode(&q).await.unwrap(), None);
        // Sem rua não consulta.
        let empty = GeocodeQuery::default();
        assert!(!empty.is_usable());
        assert_eq!(g.geocode(&empty).await.unwrap(), None);
    }
}

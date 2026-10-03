//! Nominatim (OpenStreetMap): endereço → coordenada.
//!
//! Política de uso conferida em 2026-10-03
//! (operations.osmfoundation.org/policies/nominatim/): no máximo 1
//! requisição por segundo (4 por minuto para scripts recorrentes), User-Agent
//! identificando a aplicação, resultados em cache, sem autocomplete, sem
//! consultas sistemáticas, atribuição ao OpenStreetMap e nada de dados
//! pessoais. Por isso fica desligado por padrão e passa pelo cache do
//! pipeline. Dados © contribuidores do OpenStreetMap, licença ODbL.

use std::sync::Arc;

use async_trait::async_trait;
use crab_domain::{CoordinatePrecision, GeoPoint};
use serde::Deserialize;

use super::{GeocodeQuery, GeocodeResult, Geocoder};
use crate::http::HttpClient;
use crate::CrawlError;

pub const BASE_URL: &str = "https://nominatim.openstreetmap.org";

pub struct Nominatim {
    http: Arc<HttpClient>,
    base_url: String,
    /// E-mail de contato, recomendado pela política para uso regular.
    email: Option<String>,
}

impl Nominatim {
    /// `http` deve ter `min_interval` de pelo menos 1 s.
    pub fn new(http: Arc<HttpClient>, email: Option<String>) -> Self {
        Self::with_base_url(http, BASE_URL, email)
    }

    pub fn with_base_url(
        http: Arc<HttpClient>,
        base_url: impl Into<String>,
        email: Option<String>,
    ) -> Self {
        Self {
            http,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            email,
        }
    }
}

#[derive(Deserialize)]
struct Place {
    lat: String,
    lon: String,
    display_name: Option<String>,
    #[serde(default)]
    place_rank: Option<u32>,
    #[serde(default)]
    addresstype: Option<String>,
}

/// `place_rank` do Nominatim: 30 = edifício/número, 26–27 = rua.
fn precision(place: &Place, had_number: bool) -> CoordinatePrecision {
    match (place.place_rank, place.addresstype.as_deref()) {
        (_, Some("postcode")) => CoordinatePrecision::PostalCode,
        (Some(r), _) if r >= 30 && had_number => CoordinatePrecision::Exact,
        (Some(r), _) if r >= 26 => CoordinatePrecision::Street,
        _ => CoordinatePrecision::Approximate,
    }
}

#[async_trait]
impl Geocoder for Nominatim {
    fn name(&self) -> &'static str {
        "nominatim"
    }

    async fn geocode(&self, q: &GeocodeQuery) -> Result<Option<GeocodeResult>, CrawlError> {
        if !q.is_usable() {
            return Ok(None);
        }
        let street = match (&q.number, &q.street) {
            (Some(n), Some(s)) => format!("{n} {s}"),
            (None, Some(s)) => s.clone(),
            _ => return Ok(None),
        };
        let mut url = url::Url::parse(&format!("{}/search", self.base_url))
            .map_err(|_| CrawlError::Config("URL do Nominatim inválida".into()))?;
        {
            let mut qp = url.query_pairs_mut();
            qp.append_pair("format", "jsonv2")
                .append_pair("limit", "1")
                .append_pair("countrycodes", "br")
                .append_pair("street", &street);
            if let Some(city) = &q.municipality {
                qp.append_pair("city", city);
            }
            if let Some(state) = &q.state {
                qp.append_pair("state", state);
            }
            if let Some(email) = &self.email {
                qp.append_pair("email", email);
            }
        }
        let places: Vec<Place> = self.http.get_json(url.as_str()).await?;
        let Some(place) = places.into_iter().next() else {
            return Ok(None);
        };
        let (Ok(lat), Ok(lon)) = (place.lat.parse::<f64>(), place.lon.parse::<f64>()) else {
            return Err(CrawlError::Parse("coordenada inválida do Nominatim".into()));
        };
        let Some(point) = GeoPoint::new(lat, lon) else {
            return Err(CrawlError::Parse("coordenada inválida do Nominatim".into()));
        };
        Ok(Some(GeocodeResult {
            point,
            precision: precision(&place, q.number.is_some()),
            label: place.display_name,
        }))
    }
}

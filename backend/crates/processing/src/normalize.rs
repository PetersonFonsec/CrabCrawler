use std::collections::HashMap;

use chrono::Utc;
use crab_domain::{DataSource, GeoPoint, Listing, Location, Provenance, RawListing};
use unicode_normalization::UnicodeNormalization;

/// Remove acentos, deixa minúsculo e troca separadores por `-`.
/// `"São Bernardo do Campo"` → `"sao-bernardo-do-campo"`.
pub fn slugify(text: &str) -> String {
    let ascii: String = text
        .nfd()
        .filter(|c| c.is_ascii())
        .collect::<String>()
        .to_lowercase();
    ascii
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Resultado do parsing do texto livre de localização de um anúncio.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ParsedLocation {
    pub municipality: Option<String>,
    pub state: Option<String>,
    pub neighborhood: Option<String>,
}

/// Interpreta textos como `"São Bernardo do Campo - SP / Rudge Ramos"` ou
/// `"Rudge Ramos, São Bernardo do Campo - SP"`.
pub fn parse_location_text(text: &str) -> ParsedLocation {
    let (city_part, neighborhood) = if let Some((city, hood)) = text.split_once('/') {
        (city.trim(), Some(hood.trim()))
    } else if let Some((hood, city)) = text.split_once(',') {
        (city.trim(), Some(hood.trim()))
    } else {
        (text.trim(), None)
    };

    let (municipality, state) = match city_part.rsplit_once(" - ") {
        Some((city, uf)) if uf.trim().len() == 2 => (
            Some(city.trim().to_string()),
            Some(uf.trim().to_uppercase()),
        ),
        _ => (Some(city_part.to_string()).filter(|s| !s.is_empty()), None),
    };

    ParsedLocation {
        municipality,
        state,
        neighborhood: neighborhood.filter(|s| !s.is_empty()).map(str::to_string),
    }
}

/// Tabela nome normalizado do município → código IBGE.
///
/// Pode ser populada a partir da API de Localidades do IBGE
/// (`IbgeClient::municipalities`). [`Gazetteer::mvp`] traz só a região do MVP.
#[derive(Debug, Clone, Default)]
pub struct Gazetteer {
    by_key: HashMap<(String, String), String>,
}

impl Gazetteer {
    pub fn mvp() -> Self {
        let mut g = Self::default();
        g.insert("SP", "São Bernardo do Campo", "3548708");
        g.insert("SP", "Santo André", "3547809");
        g.insert("SP", "São Caetano do Sul", "3548807");
        g.insert("SP", "Diadema", "3513801");
        g.insert("SP", "São Paulo", "3550308");
        g
    }

    pub fn insert(&mut self, uf: &str, municipality: &str, ibge_code: &str) {
        self.by_key.insert(
            (uf.to_uppercase(), slugify(municipality)),
            ibge_code.to_string(),
        );
    }

    pub fn lookup(&self, uf: &str, municipality: &str) -> Option<&str> {
        self.by_key
            .get(&(uf.to_uppercase(), slugify(municipality)))
            .map(String::as_str)
    }
}

/// Converte um anúncio bruto no modelo de domínio, resolvendo o município
/// para o código IBGE, que é a chave de junção com os datasets públicos.
pub fn normalize_listing(raw: RawListing, source: DataSource, gazetteer: &Gazetteer) -> Listing {
    let parsed = parse_location_text(&raw.location_text);
    let municipality_ibge_code = match (&parsed.state, &parsed.municipality) {
        (Some(uf), Some(city)) => gazetteer.lookup(uf, city).map(str::to_string),
        _ => None,
    };
    let point = match (raw.lat, raw.lon) {
        (Some(lat), Some(lon)) => GeoPoint::new(lat, lon),
        _ => None,
    };
    let postal_code = raw
        .postal_code
        .map(|cep| cep.chars().filter(char::is_ascii_digit).collect::<String>())
        .filter(|cep| cep.len() == 8);

    let now = Utc::now();
    Listing {
        id: Listing::stable_id(source.as_str(), &raw.external_id),
        external_id: raw.external_id,
        title: raw.title,
        transaction: raw.transaction,
        kind: raw.kind,
        price_brl: raw.price_brl,
        area_m2: raw.area_m2,
        bedrooms: raw.bedrooms,
        bathrooms: raw.bathrooms,
        parking_spots: raw.parking_spots,
        location: Location {
            state: parsed.state,
            municipality: parsed.municipality,
            municipality_ibge_code,
            neighborhood_slug: parsed.neighborhood.as_deref().map(slugify),
            neighborhood: parsed.neighborhood,
            street: None,
            postal_code,
            point,
        },
        provenance: Provenance {
            source,
            url: raw.url,
            collected_at: now,
        },
        updated_at: now,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_strips_accents() {
        assert_eq!(slugify("São Bernardo do Campo"), "sao-bernardo-do-campo");
        assert_eq!(slugify("  Rudge   Ramos "), "rudge-ramos");
    }

    #[test]
    fn parses_city_slash_neighborhood() {
        let p = parse_location_text("São Bernardo do Campo - SP / Rudge Ramos");
        assert_eq!(p.municipality.as_deref(), Some("São Bernardo do Campo"));
        assert_eq!(p.state.as_deref(), Some("SP"));
        assert_eq!(p.neighborhood.as_deref(), Some("Rudge Ramos"));
    }

    #[test]
    fn parses_neighborhood_comma_city() {
        let p = parse_location_text("Centro, Santo André - sp");
        assert_eq!(p.municipality.as_deref(), Some("Santo André"));
        assert_eq!(p.state.as_deref(), Some("SP"));
        assert_eq!(p.neighborhood.as_deref(), Some("Centro"));
    }

    #[test]
    fn gazetteer_matches_regardless_of_accents() {
        let g = Gazetteer::mvp();
        assert_eq!(g.lookup("sp", "SAO BERNARDO DO CAMPO"), Some("3548708"));
    }
}

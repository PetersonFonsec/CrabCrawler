//! Leitura de GeoJSON (WFS do GeoSampa, ArcGIS REST do SGB).
//!
//! A geometria é mantida como texto GeoJSON; o PostGIS faz o parse e a
//! reprojeção. O SRID vem do membro `crs` quando a fonte o informa (o
//! GeoServer informa); sem `crs`, o padrão do GeoJSON é WGS84 (4326).

use serde_json::{Map, Value};

use crate::CrawlError;

#[derive(Debug, Clone, PartialEq)]
pub struct Feature {
    /// `id` do Feature, quando a fonte informa.
    pub id: Option<String>,
    pub properties: Map<String, Value>,
    /// Geometria serializada; `None` quando a fonte manda `null`.
    pub geometry: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FeatureCollection {
    pub srid: i32,
    pub features: Vec<Feature>,
}

pub fn parse_feature_collection(bytes: &[u8]) -> Result<FeatureCollection, CrawlError> {
    let root: Value = serde_json::from_slice(bytes)?;
    if let Some(err) = root.get("error") {
        return Err(CrawlError::Parse(format!("a fonte devolveu erro: {err}")));
    }
    if root.get("type").and_then(Value::as_str) != Some("FeatureCollection") {
        return Err(CrawlError::Parse(
            "esperado um GeoJSON FeatureCollection".into(),
        ));
    }
    let srid = root
        .pointer("/crs/properties/name")
        .and_then(Value::as_str)
        .and_then(parse_crs_name)
        .unwrap_or(4326);
    let features = root
        .get("features")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|f| Feature {
            id: match f.get("id") {
                Some(Value::String(s)) => Some(s.clone()),
                Some(Value::Number(n)) => Some(n.to_string()),
                _ => None,
            },
            properties: f
                .get("properties")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default(),
            geometry: f
                .get("geometry")
                .filter(|g| !g.is_null())
                .map(Value::to_string),
        })
        .collect();
    Ok(FeatureCollection { srid, features })
}

/// `EPSG:31983`, `urn:ogc:def:crs:EPSG::31983`, `.../EPSG/0/31983`.
/// `CRS84` é WGS84 em ordem lon/lat (4326 no PostGIS).
pub fn parse_crs_name(name: &str) -> Option<i32> {
    if name.ends_with("CRS84") {
        return Some(4326);
    }
    let digits: String = name
        .chars()
        .rev()
        .take_while(char::is_ascii_digit)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    digits.parse().ok()
}

/// Valor de uma propriedade como texto (números viram texto), sem
/// diferenciar maiúsculas no nome.
pub fn prop_str(props: &Map<String, Value>, key: &str) -> Option<String> {
    let value = props
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v)?;
    match value {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_crs_and_null_geometry() {
        let json = br#"{"type":"FeatureCollection",
            "crs":{"type":"name","properties":{"name":"urn:ogc:def:crs:EPSG::31983"}},
            "features":[
              {"type":"Feature","id":"layer.1","properties":{"NM":"A","n":3},
               "geometry":{"type":"Point","coordinates":[333000,7390000]}},
              {"type":"Feature","id":7,"properties":{},"geometry":null}]}"#;
        let fc = parse_feature_collection(json).unwrap();
        assert_eq!(fc.srid, 31983);
        assert_eq!(fc.features[0].id.as_deref(), Some("layer.1"));
        assert_eq!(
            prop_str(&fc.features[0].properties, "nm").as_deref(),
            Some("A")
        );
        assert_eq!(
            prop_str(&fc.features[0].properties, "n").as_deref(),
            Some("3")
        );
        assert_eq!(fc.features[1].id.as_deref(), Some("7"));
        assert!(fc.features[1].geometry.is_none());
    }

    #[test]
    fn crs_defaults_to_wgs84_and_rejects_errors() {
        let fc =
            parse_feature_collection(br#"{"type":"FeatureCollection","features":[]}"#).unwrap();
        assert_eq!(fc.srid, 4326);
        assert_eq!(parse_crs_name("EPSG:4674"), Some(4674));
        assert_eq!(parse_crs_name("urn:ogc:def:crs:OGC:1.3:CRS84"), Some(4326));
        assert!(parse_feature_collection(br#"{"error":{"code":400}}"#).is_err());
        assert!(parse_feature_collection(b"not json").is_err());
    }
}

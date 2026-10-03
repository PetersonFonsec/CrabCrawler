//! GeoSampa — equipamentos urbanos do município de São Paulo pelo WFS
//! oficial (`wfs.geosampa.prefeitura.sp.gov.br/geoserver/geoportal/wfs`).
//!
//! Só entram camadas cujo nome WFS foi confirmado no GetCapabilities em
//! 2026-10-03 ([`LAYERS`]). Outras camadas citadas no catálogo de metadados
//! (UBS, hospitais, risco geológico/hidrológico) existem, mas o nome WFS
//! não pôde ser lido daqui; ver `docs/data-sources/GEOSAMPA.md`.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use crab_domain::regional::{
    provider_descriptor, DatasetBatch, DatasetProvenance, GeographicGranularity, ImportReport,
    ProviderDescriptor, RawGeometry, ServiceCategory, UrbanService,
};
use crab_domain::DataSource;
use serde_json::Value;

use super::geojson::{parse_feature_collection, prop_str};
use super::raw::{dataset_version, file_name, RawStore};
use super::{InfrastructureDataProvider, RegionalDataProvider, RegionalScope};
use crate::http::HttpClient;
use crate::CrawlError;

/// Código IBGE do município de São Paulo.
pub const SAO_PAULO: &str = "3550308";
pub const WFS_URL: &str = "https://wfs.geosampa.prefeitura.sp.gov.br/geoserver/geoportal/wfs";

/// Camada WFS e como ela entra no modelo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GeoSampaLayer {
    pub name: &'static str,
    pub category: ServiceCategory,
    pub subcategory: &'static str,
}

/// Camadas confirmadas no GetCapabilities (2026-10-03).
pub const LAYERS: &[GeoSampaLayer] = &[
    GeoSampaLayer {
        name: "equipamento_saude_ambulatorios_especializados",
        category: ServiceCategory::Health,
        subcategory: "Ambulatório especializado",
    },
    GeoSampaLayer {
        name: "equipamento_saude_saude_mental",
        category: ServiceCategory::Health,
        subcategory: "Saúde mental",
    },
    GeoSampaLayer {
        name: "equipamento_educacao_ceu",
        category: ServiceCategory::Education,
        subcategory: "CEU",
    },
    GeoSampaLayer {
        name: "equipamento_educacao_outros",
        category: ServiceCategory::Education,
        subcategory: "Educação (outros)",
    },
    GeoSampaLayer {
        name: "equipamento_cultura_bibliotecas",
        category: ServiceCategory::Culture,
        subcategory: "Biblioteca",
    },
    GeoSampaLayer {
        name: "equipamento_cultura_outros",
        category: ServiceCategory::Culture,
        subcategory: "Cultura (outros)",
    },
    GeoSampaLayer {
        name: "equipamento_esporte_centro_esportivo",
        category: ServiceCategory::Sport,
        subcategory: "Centro esportivo",
    },
    GeoSampaLayer {
        name: "equipamento_esporte_clubes",
        category: ServiceCategory::Sport,
        subcategory: "Clube",
    },
    GeoSampaLayer {
        name: "equipamento_esporte_clubesdacomunidade",
        category: ServiceCategory::Sport,
        subcategory: "Clube da comunidade",
    },
];

pub fn layer(name: &str) -> Option<&'static GeoSampaLayer> {
    let name = name.trim_start_matches("geoportal:");
    LAYERS.iter().find(|l| l.name == name)
}

pub fn wfs_url(layer: &str) -> String {
    format!(
        "{WFS_URL}?service=WFS&version=1.0.0&request=GetFeature\
         &typeName=geoportal:{layer}&outputFormat=application/json"
    )
}

pub enum GeoSampaInput {
    /// GeoJSON já baixado.
    File(PathBuf),
    /// Baixa do WFS e grava o bruto.
    Wfs {
        http: Arc<HttpClient>,
        raw: RawStore,
    },
}

pub struct GeoSampaProvider {
    layer: &'static GeoSampaLayer,
    input: GeoSampaInput,
}

impl GeoSampaProvider {
    pub fn new(layer: &'static GeoSampaLayer, input: GeoSampaInput) -> Self {
        Self { layer, input }
    }
}

impl RegionalDataProvider for GeoSampaProvider {
    fn descriptor(&self) -> &'static ProviderDescriptor {
        provider_descriptor(DataSource::GeoSampa).expect("GeoSampa registrado")
    }
    fn dataset_name(&self) -> String {
        self.layer.name.into()
    }
}

#[async_trait]
impl InfrastructureDataProvider for GeoSampaProvider {
    async fn services(
        &self,
        scope: &RegionalScope,
    ) -> Result<DatasetBatch<UrbanService>, CrawlError> {
        if !scope.contains(SAO_PAULO) {
            return Err(CrawlError::Parse(format!(
                "o GeoSampa só cobre o município de São Paulo ({SAO_PAULO})"
            )));
        }
        let url = wfs_url(self.layer.name);
        let (bytes, published_name) = match &self.input {
            GeoSampaInput::File(path) => (tokio::fs::read(path).await?, file_name(path)),
            GeoSampaInput::Wfs { http, raw } => {
                let bytes = http.get_bytes(&url).await?;
                let name = format!("{}.geojson", self.layer.name);
                raw.save("geosampa", &name, &bytes).await?;
                (bytes, name)
            }
        };
        let (records, report) = parse_services(&bytes, self.layer)?;
        Ok(DatasetBatch {
            provenance: DatasetProvenance {
                source: DataSource::GeoSampa,
                dataset_name: self.dataset_name(),
                dataset_version: dataset_version(&published_name, &bytes),
                source_url: Some(url),
                reference_date: None,
                collected_at: Utc::now(),
                granularity: GeographicGranularity::Point,
            },
            coverage: vec![SAO_PAULO.to_string()],
            records,
            report,
        })
    }
}

/// Primeiro valor não vazio entre as propriedades cujo nome passa no filtro.
fn first_prop(
    props: &serde_json::Map<String, Value>,
    matches: impl Fn(&str) -> bool,
) -> Option<String> {
    props
        .keys()
        .filter(|k| matches(&k.to_ascii_lowercase()))
        .find_map(|k| prop_str(props, k))
}

/// Converte o GeoJSON de uma camada em equipamentos. O nome vem da primeira
/// propriedade `nm_*`/`nome*` preenchida e o endereço da primeira com
/// "endereco"/"logradouro"; todas as propriedades originais ficam em
/// `attributes`.
pub fn parse_services(
    bytes: &[u8],
    layer: &GeoSampaLayer,
) -> Result<(Vec<UrbanService>, ImportReport), CrawlError> {
    let fc = parse_feature_collection(bytes)?;
    let mut report = ImportReport {
        read: fc.features.len(),
        ..Default::default()
    };
    let mut out = Vec::new();
    for (i, f) in fc.features.into_iter().enumerate() {
        let external_id =
            f.id.clone()
                .unwrap_or_else(|| format!("{}.{i}", layer.name));
        let Some(geometry) = f.geometry else {
            report.skip(format!("{external_id}: sem geometria"));
            continue;
        };
        let name = first_prop(&f.properties, |k| {
            k.starts_with("nm_") || k.starts_with("nome")
        });
        if name.is_none() {
            report.note(format!("{external_id}: sem nome"));
        }
        out.push(UrbanService {
            external_id,
            name,
            category: layer.category,
            subcategory: Some(layer.subcategory.to_string()),
            address: first_prop(&f.properties, |k| {
                k.contains("endereco") || k.contains("logradouro")
            }),
            municipality_ibge_code: SAO_PAULO.to_string(),
            geometry: RawGeometry::GeoJson {
                json: geometry,
                srid: fc.srid,
            },
            attributes: Value::Object(f.properties),
        });
    }
    Ok((out, report))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_layer_features() {
        let json = br#"{"type":"FeatureCollection",
          "crs":{"type":"name","properties":{"name":"urn:ogc:def:crs:EPSG::31983"}},
          "features":[
            {"type":"Feature","id":"equipamento_cultura_bibliotecas.1",
             "properties":{"nm_equipamento":"Biblioteca X","tx_endereco":"Rua A, 1"},
             "geometry":{"type":"Point","coordinates":[333000.0,7394000.0]}},
            {"type":"Feature","id":"equipamento_cultura_bibliotecas.2",
             "properties":{},"geometry":null}]}"#;
        let l = layer("geoportal:equipamento_cultura_bibliotecas").unwrap();
        let (services, report) = parse_services(json, l).unwrap();
        assert_eq!(services.len(), 1);
        assert_eq!(report.skipped, 1);
        let s = &services[0];
        assert_eq!(s.name.as_deref(), Some("Biblioteca X"));
        assert_eq!(s.address.as_deref(), Some("Rua A, 1"));
        assert_eq!(s.category, ServiceCategory::Culture);
        assert!(matches!(
            s.geometry,
            RawGeometry::GeoJson { srid: 31983, .. }
        ));
    }

    #[test]
    fn unknown_layer_is_rejected() {
        assert!(layer("equipamento_inventado").is_none());
        assert!(wfs_url("x").contains("typeName=geoportal:x"));
    }
}

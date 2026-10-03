//! Imóveis vindos de fontes autorizadas (cadastro manual, feeds VRSync,
//! APIs oficiais).
//!
//! Separação central do módulo:
//! - [`RawProperty`]: o que o provider recebeu, com os valores originais em
//!   texto e o payload bruto. Nada é interpretado aqui;
//! - [`PropertyDetails`] / [`ListingDetails`]: resultado da normalização;
//! - [`Property`]: o imóvel físico (tipo, áreas, endereço, coordenada);
//! - [`PropertyListing`]: um anúncio daquele imóvel numa fonte. Um imóvel
//!   pode ter vários anúncios (fontes diferentes, venda e aluguel).
//!
//! Regional Intelligence, segurança e comparador só olham para o imóvel
//! (coordenada, município) e para o anúncio (preço, finalidade); a fonte
//! nunca muda o tratamento.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{GeoPoint, PropertyType, TransactionType};

/// Origem de um anúncio.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PropertySource {
    /// Cadastrado pelo usuário no CrabCrawler.
    Manual,
    /// Feed XML no padrão VRSync, publicado por um parceiro autorizado.
    Vrsync,
    /// API oficial de um parceiro.
    Api,
    /// Arquivo local de desenvolvimento (`fixtures/listings.json`).
    Fixture,
}

/// O que um provider consegue fazer. Nenhum provider é obrigado a
/// implementar o que a fonte não oferece.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProviderCapability {
    /// Recebe imóveis digitados por uma pessoa.
    ManualEntry,
    /// Busca por filtros na fonte.
    Search,
    /// Consulta um anúncio pelo id externo.
    FetchById,
    /// Entrega o catálogo inteiro de uma vez (ex.: feed XML).
    BulkImport,
    /// Entrega só o que mudou desde a última sincronização.
    IncrementalSync,
}

impl PropertySource {
    pub const ALL: [PropertySource; 4] = [Self::Manual, Self::Vrsync, Self::Api, Self::Fixture];

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Manual => "MANUAL",
            Self::Vrsync => "VRSYNC",
            Self::Api => "API",
            Self::Fixture => "FIXTURE",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|s| s.as_str().eq_ignore_ascii_case(value.trim()))
    }

    /// Capacidades implementadas hoje para cada categoria de fonte.
    pub fn capabilities(&self) -> &'static [ProviderCapability] {
        match self {
            Self::Manual => &[ProviderCapability::ManualEntry],
            Self::Vrsync => &[ProviderCapability::BulkImport],
            // Depende da API real; a infraestrutura genérica não promete nada.
            Self::Api => &[],
            Self::Fixture => &[ProviderCapability::BulkImport],
        }
    }

    /// `true` quando cada sincronização traz o catálogo completo do parceiro,
    /// e portanto a ausência de um anúncio é informação.
    pub fn is_full_snapshot(&self) -> bool {
        matches!(self, Self::Vrsync)
    }
}

/// Estado do anúncio na fonte. Anúncios nunca são apagados por sumirem de
/// uma sincronização: viram `Inactive` e o histórico fica.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ListingStatus {
    Active,
    /// Sumiu de um feed completo, ou a fonte informou que não está mais no ar.
    Inactive,
    /// Removido explicitamente (pelo usuário ou pela fonte).
    Removed,
    Unknown,
}

impl ListingStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "ACTIVE",
            Self::Inactive => "INACTIVE",
            Self::Removed => "REMOVED",
            Self::Unknown => "UNKNOWN",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        [Self::Active, Self::Inactive, Self::Removed, Self::Unknown]
            .into_iter()
            .find(|s| s.as_str().eq_ignore_ascii_case(value.trim()))
    }
}

/// De onde veio a coordenada do imóvel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CoordinateSource {
    /// Informada pela fonte do anúncio (feed, API).
    PropertySource,
    /// Calculada por um serviço de geocoding a partir do endereço.
    Geocoding,
    /// Centroide do CEP: aproximação.
    PostalCodeCentroid,
    /// Digitada pelo usuário.
    Manual,
}

impl CoordinateSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PropertySource => "PROPERTY_SOURCE",
            Self::Geocoding => "GEOCODING",
            Self::PostalCodeCentroid => "POSTAL_CODE_CENTROID",
            Self::Manual => "MANUAL",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        [
            Self::PropertySource,
            Self::Geocoding,
            Self::PostalCodeCentroid,
            Self::Manual,
        ]
        .into_iter()
        .find(|s| s.as_str().eq_ignore_ascii_case(value.trim()))
    }
}

/// Quão perto do imóvel a coordenada está. Serve para a interface separar
/// coordenada exata de aproximação.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CoordinatePrecision {
    /// Número/edifício.
    Exact,
    /// Trecho de rua (sem número).
    Street,
    /// Área do CEP.
    PostalCode,
    /// Bairro, município ou resultado de baixa confiança.
    Approximate,
    /// Como a fonte informou, sem indicação de precisão.
    Reported,
}

impl CoordinatePrecision {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Exact => "EXACT",
            Self::Street => "STREET",
            Self::PostalCode => "POSTAL_CODE",
            Self::Approximate => "APPROXIMATE",
            Self::Reported => "REPORTED",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        [
            Self::Exact,
            Self::Street,
            Self::PostalCode,
            Self::Approximate,
            Self::Reported,
        ]
        .into_iter()
        .find(|s| s.as_str().eq_ignore_ascii_case(value.trim()))
    }

    /// Precisa o bastante para análises por coordenada (setor censitário,
    /// serviços num raio, polígonos de risco).
    pub fn is_point_level(&self) -> bool {
        matches!(self, Self::Exact | Self::Reported)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Coordinates {
    pub point: GeoPoint,
    pub source: CoordinateSource,
    pub precision: CoordinatePrecision,
}

/// Imóvel como chegou do provider, antes da normalização.
///
/// Os campos são texto de propósito: o normalizer é o único lugar que
/// interpreta "R$ 620.000,00", "80 m²" ou "Residential / Apartment", e o
/// valor original continua disponível para rastreabilidade.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct RawProperty {
    pub source: Option<PropertySource>,
    pub partner_id: Option<Uuid>,
    /// Id do anúncio na fonte. Obrigatório para fontes externas.
    pub external_id: Option<String>,
    pub source_url: Option<String>,

    pub title: Option<String>,
    pub description: Option<String>,
    pub notes: Option<String>,

    /// "Venda", "For Rent", "Sale/Rent"...
    pub transaction: Option<String>,
    /// "Apartamento", "Residential / Apartment", "apto"...
    pub property_type: Option<String>,
    pub usage_type: Option<String>,

    pub currency: Option<String>,
    pub sale_price: Option<String>,
    pub rent_price: Option<String>,
    /// Período do aluguel ("Monthly"...). Só mensal é aceito hoje.
    pub rent_period: Option<String>,
    pub condominium_fee: Option<String>,
    pub property_tax: Option<String>,
    /// "Yearly" ou "Monthly". Sem valor, IPTU é tratado como anual.
    pub property_tax_period: Option<String>,

    pub living_area: Option<String>,
    pub lot_area: Option<String>,
    /// Unidade das áreas ("square metres", "m²"...).
    pub area_unit: Option<String>,
    pub bedrooms: Option<String>,
    pub bathrooms: Option<String>,
    pub suites: Option<String>,
    pub parking_spaces: Option<String>,

    pub street: Option<String>,
    pub street_number: Option<String>,
    pub complement: Option<String>,
    pub neighborhood: Option<String>,
    pub municipality: Option<String>,
    pub state: Option<String>,
    pub postal_code: Option<String>,
    pub country: Option<String>,

    pub latitude: Option<String>,
    pub longitude: Option<String>,
    /// Quem forneceu a coordenada, quando há (manual ou fonte).
    pub coordinates_origin: Option<CoordinateSource>,

    /// URLs de imagens. Nunca são baixadas pelo CrabCrawler.
    pub images: Vec<String>,
    /// Payload original do item (XML convertido, JSON da API, formulário).
    pub raw_payload: Value,
}

/// Endereço normalizado do imóvel.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Address {
    pub street: Option<String>,
    pub number: Option<String>,
    pub complement: Option<String>,
    pub neighborhood: Option<String>,
    pub neighborhood_slug: Option<String>,
    pub municipality: Option<String>,
    pub municipality_ibge_code: Option<String>,
    /// UF com duas letras.
    pub state: Option<String>,
    /// CEP com 8 dígitos, sem hífen.
    pub postal_code: Option<String>,
}

/// Características físicas do imóvel, já normalizadas.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct PropertyDetails {
    pub property_type: PropertyType,
    pub area_m2: Option<f64>,
    pub lot_area_m2: Option<f64>,
    pub bedrooms: Option<u16>,
    pub bathrooms: Option<u16>,
    pub suites: Option<u16>,
    pub parking_spaces: Option<u16>,
    pub address: Address,
    pub coordinates: Option<Coordinates>,
}

/// Dados de um anúncio, já normalizados. Valores monetários em BRL.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ListingDetails {
    pub transaction: TransactionType,
    pub price_brl: f64,
    pub condominium_fee_brl: Option<f64>,
    /// IPTU anual.
    pub property_tax_brl: Option<f64>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub notes: Option<String>,
    pub source_url: Option<String>,
    pub images: Vec<String>,
}

/// Imóvel físico.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Property {
    pub id: Uuid,
    #[serde(flatten)]
    pub details: PropertyDetails,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Anúncio de um imóvel numa fonte, com proveniência completa.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PropertyListing {
    pub id: Uuid,
    pub property_id: Uuid,
    pub source: PropertySource,
    pub partner_id: Option<Uuid>,
    pub external_id: String,
    #[serde(flatten)]
    pub details: ListingDetails,
    pub status: ListingStatus,
    /// Hash do conteúdo normalizado: muda quando o anúncio muda.
    pub content_hash: String,
    /// Valores originais usados na normalização (tipo, finalidade...).
    pub original: Value,
    pub first_seen_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub imported_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Um preço observado. Só há nova entrada quando o preço muda.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PriceObservation {
    pub listing_id: Uuid,
    pub price_brl: f64,
    pub observed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PartnerType {
    RealEstateAgency,
    Broker,
    Crm,
    Marketplace,
}

impl PartnerType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::RealEstateAgency => "REAL_ESTATE_AGENCY",
            Self::Broker => "BROKER",
            Self::Crm => "CRM",
            Self::Marketplace => "MARKETPLACE",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        [
            Self::RealEstateAgency,
            Self::Broker,
            Self::Crm,
            Self::Marketplace,
        ]
        .into_iter()
        .find(|s| s.as_str().eq_ignore_ascii_case(value.trim()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PartnerStatus {
    Active,
    Paused,
    Disabled,
}

impl PartnerStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "ACTIVE",
            Self::Paused => "PAUSED",
            Self::Disabled => "DISABLED",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        [Self::Active, Self::Paused, Self::Disabled]
            .into_iter()
            .find(|s| s.as_str().eq_ignore_ascii_case(value.trim()))
    }
}

/// Quem fornece imóveis ao CrabCrawler (imobiliária, corretor, CRM...).
///
/// `configuration` guarda só dados não sensíveis (URL do feed, limites).
/// Credenciais nunca vão para o banco: a configuração aponta o **nome** de
/// uma variável de ambiente (`credentials_env`), lida só na hora da chamada.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PropertySourcePartner {
    pub id: Uuid,
    /// Identificador curto usado na CLI (`sync properties --partner <slug>`).
    pub slug: String,
    pub name: String,
    pub partner_type: PartnerType,
    pub status: PartnerStatus,
    pub provider: PropertySource,
    pub configuration: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Contadores de uma sincronização.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SyncMetrics {
    /// Itens lidos da fonte (válidos ou não).
    pub received: u32,
    pub created: u32,
    pub updated: u32,
    pub unchanged: u32,
    /// Itens recusados pela validação/normalização.
    pub invalid: u32,
    /// Anúncios marcados como inativos por sumirem de um feed completo.
    pub deactivated: u32,
    /// Itens válidos que falharam ao persistir.
    pub failed: u32,
}

/// Por que um item foi recusado ou falhou.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncItemError {
    pub external_id: Option<String>,
    /// `parse`, `normalize` ou `persist`.
    pub stage: String,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SyncRunStatus {
    Running,
    /// Terminou; itens individuais podem ter sido recusados.
    Succeeded,
    /// A fonte inteira falhou (download, XML ilegível...).
    Failed,
}

impl SyncRunStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Running => "RUNNING",
            Self::Succeeded => "SUCCEEDED",
            Self::Failed => "FAILED",
        }
    }
}

/// Dados de um cadastro manual, como chegam da interface.
///
/// Campos mínimos: finalidade, preço e um ponto de partida de localização
/// (CEP válido, ou município + UF, ou coordenada). Todo o resto é opcional.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ManualPropertyInput {
    /// `sale`/`rent` (aceita também "venda", "aluguel").
    pub transaction: Option<String>,
    pub property_type: Option<String>,
    pub price: Option<f64>,
    pub postal_code: Option<String>,
    pub street: Option<String>,
    pub number: Option<String>,
    pub complement: Option<String>,
    pub neighborhood: Option<String>,
    pub municipality: Option<String>,
    pub state: Option<String>,
    pub area_m2: Option<f64>,
    pub bedrooms: Option<u32>,
    pub bathrooms: Option<u32>,
    pub suites: Option<u32>,
    pub parking_spaces: Option<u32>,
    pub condominium_fee: Option<f64>,
    /// IPTU anual.
    pub property_tax: Option<f64>,
    /// Link do anúncio original. É só referência: nunca é acessado.
    pub source_url: Option<String>,
    pub notes: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}

impl ManualPropertyInput {
    /// Converte para o formato comum dos providers. O payload original fica
    /// guardado como veio.
    pub fn into_raw(self) -> RawProperty {
        let raw_payload = serde_json::to_value(&self).unwrap_or(Value::Null);
        let num = |v: Option<f64>| v.map(|n| n.to_string());
        let int = |v: Option<u32>| v.map(|n| n.to_string());
        let has_coordinates = self.latitude.is_some() && self.longitude.is_some();
        RawProperty {
            source: Some(PropertySource::Manual),
            partner_id: None,
            external_id: None,
            source_url: self.source_url,
            title: None,
            description: None,
            notes: self.notes,
            transaction: self.transaction,
            property_type: self.property_type,
            usage_type: None,
            currency: Some("BRL".into()),
            sale_price: None,
            rent_price: None,
            rent_period: None,
            condominium_fee: num(self.condominium_fee),
            property_tax: num(self.property_tax),
            property_tax_period: Some("Yearly".into()),
            living_area: num(self.area_m2),
            lot_area: None,
            area_unit: Some("m2".into()),
            bedrooms: int(self.bedrooms),
            bathrooms: int(self.bathrooms),
            suites: int(self.suites),
            parking_spaces: int(self.parking_spaces),
            street: self.street,
            street_number: self.number,
            complement: self.complement,
            neighborhood: self.neighborhood,
            municipality: self.municipality,
            state: self.state,
            postal_code: self.postal_code,
            country: Some("BR".into()),
            latitude: num(self.latitude),
            longitude: num(self.longitude),
            coordinates_origin: has_coordinates.then_some(CoordinateSource::Manual),
            images: vec![],
            raw_payload,
        }
        .with_price(num(self.price))
    }
}

impl RawProperty {
    /// Coloca um preço sem finalidade definida no campo certo: o manual tem
    /// um único preço, que vale para a finalidade escolhida.
    fn with_price(mut self, price: Option<String>) -> Self {
        let rent = self
            .transaction
            .as_deref()
            .map(|t| {
                let t = t.to_lowercase();
                t.contains("rent") || t.contains("alug") || t.contains("loca")
            })
            .unwrap_or(false);
        if rent {
            self.rent_price = price;
        } else {
            self.sale_price = price;
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_input_puts_price_on_chosen_transaction() {
        let sale = ManualPropertyInput {
            transaction: Some("sale".into()),
            price: Some(620_000.0),
            ..Default::default()
        }
        .into_raw();
        assert_eq!(sale.sale_price.as_deref(), Some("620000"));
        assert_eq!(sale.rent_price, None);

        let rent = ManualPropertyInput {
            transaction: Some("Aluguel".into()),
            price: Some(2500.0),
            ..Default::default()
        }
        .into_raw();
        assert_eq!(rent.rent_price.as_deref(), Some("2500"));
        assert_eq!(rent.source, Some(PropertySource::Manual));
    }

    #[test]
    fn enums_round_trip() {
        for s in PropertySource::ALL {
            assert_eq!(PropertySource::parse(s.as_str()), Some(s));
        }
        assert_eq!(
            ListingStatus::parse("inactive"),
            Some(ListingStatus::Inactive)
        );
        assert_eq!(
            CoordinateSource::parse("POSTAL_CODE_CENTROID"),
            Some(CoordinateSource::PostalCodeCentroid)
        );
    }
}

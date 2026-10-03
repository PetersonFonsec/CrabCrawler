use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Fontes de dados conhecidas. Cada dado persistido aponta para uma delas,
/// para que a interface possa mostrar "de onde veio este número".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataSource {
    /// Anúncios carregados de um arquivo local (desenvolvimento).
    ListingFixture,
    /// API de Localidades do IBGE.
    IbgeLocalidades,
    /// Estatísticas de criminalidade da Secretaria da Segurança Pública de SP.
    SspSp,
    /// Sinesp VDE — Dados Nacionais de Segurança Pública (MJSP).
    SinespVde,
}

impl DataSource {
    pub const ALL: [DataSource; 4] = [
        Self::ListingFixture,
        Self::IbgeLocalidades,
        Self::SspSp,
        Self::SinespVde,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ListingFixture => "listing_fixture",
            Self::IbgeLocalidades => "ibge_localidades",
            Self::SspSp => "ssp_sp",
            Self::SinespVde => "sinesp_vde",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str() == value)
    }

    pub fn reference_url(&self) -> &'static str {
        self.metadata().reference_url
    }

    /// Metadados de transparência: o que a interface precisa mostrar junto
    /// de qualquer número vindo desta fonte.
    pub fn metadata(&self) -> SourceMetadata {
        match self {
            Self::ListingFixture => SourceMetadata {
                id: self.as_str(),
                name: "Anúncios de exemplo (arquivo local)",
                publisher: "CrabCrawler",
                reference_url: "fixtures/listings.json",
                methodology: "Arquivo JSON local usado em desenvolvimento.",
                limitations: &["Dados fictícios."],
            },
            Self::IbgeLocalidades => SourceMetadata {
                id: self.as_str(),
                name: "IBGE — Localidades e Agregados (SIDRA)",
                publisher: "Instituto Brasileiro de Geografia e Estatística",
                reference_url: "https://servicodados.ibge.gov.br/api/docs/localidades",
                methodology: "População residente do Censo Demográfico 2022 \
                    (agregado 4709, variável 93), por município.",
                limitations: &[
                    "Contagem pontual de 2022; para outros anos a população não é estimada.",
                    "Distritos do IBGE não coincidem com áreas de delegacia.",
                ],
            },
            Self::SspSp => SourceMetadata {
                id: self.as_str(),
                name: "SSP-SP — Dados Mensais (Resolução SSP 160/2001)",
                publisher: "Secretaria da Segurança Pública do Estado de São Paulo",
                reference_url: "https://www.ssp.sp.gov.br/estatistica/dados-mensais",
                methodology: "Ocorrências registradas pela Polícia Civil, informadas \
                    mensalmente por cada delegacia, por natureza. Homicídio e latrocínio \
                    também são contados por vítima.",
                limitations: &[
                    "Só inclui crimes registrados em boletim; a subnotificação varia por tipo de crime.",
                    "Valores podem ser revisados retroativamente.",
                    "Granularidade mínima é a delegacia; não há malha pública das áreas de DP nem dado por bairro.",
                    "Sem API pública documentada: a exportação da página oficial é feita manualmente.",
                ],
            },
            Self::SinespVde => SourceMetadata {
                id: self.as_str(),
                name: "Sinesp VDE — Dados Nacionais de Segurança Pública",
                publisher: "Ministério da Justiça e Segurança Pública",
                reference_url: "https://www.gov.br/mj/pt-br/assuntos/sua-seguranca/seguranca-publica/estatistica",
                methodology: "Base anual (bancovde-<ano>.xlsx) com registros mensais \
                    validados pelos gestores estaduais no Validador de Dados Estatísticos.",
                limitations: &[
                    "Só parte dos eventos é publicada por município; o restante é apenas por UF.",
                    "Não traz código IBGE: o município é casado pelo nome.",
                    "Vazio, zero e 'não reportado' têm significados diferentes na fonte.",
                    "Metodologia de contagem segue cada estado e pode divergir da SSP-SP.",
                ],
            },
        }
    }
}

/// Descrição de uma fonte para a interface (fonte, metodologia, limitações).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SourceMetadata {
    pub id: &'static str,
    pub name: &'static str,
    pub publisher: &'static str,
    pub reference_url: &'static str,
    pub methodology: &'static str,
    pub limitations: &'static [&'static str],
}

/// Procedência de um dado: fonte, URL exata e momento da coleta.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Provenance {
    pub source: DataSource,
    pub url: Option<String>,
    pub collected_at: DateTime<Utc>,
}

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
    /// IBGE — Censo 2022: malha e agregados por setor censitário.
    IbgeCensoSetores,
    /// Fundação Seade — Índice Paulista de Vulnerabilidade Social.
    SeadeIpvs,
    /// GeoSampa — mapa digital da cidade de São Paulo (WFS).
    GeoSampa,
    /// Serviço Geológico do Brasil — Setorização de Risco.
    SgbRiskSectors,
}

impl DataSource {
    pub const ALL: [DataSource; 8] = [
        Self::ListingFixture,
        Self::IbgeLocalidades,
        Self::SspSp,
        Self::SinespVde,
        Self::IbgeCensoSetores,
        Self::SeadeIpvs,
        Self::GeoSampa,
        Self::SgbRiskSectors,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ListingFixture => "listing_fixture",
            Self::IbgeLocalidades => "ibge_localidades",
            Self::SspSp => "ssp_sp",
            Self::SinespVde => "sinesp_vde",
            Self::IbgeCensoSetores => "ibge_censo2022_setores",
            Self::SeadeIpvs => "seade_ipvs",
            Self::GeoSampa => "geosampa",
            Self::SgbRiskSectors => "sgb_setorizacao_risco",
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
            Self::IbgeCensoSetores => SourceMetadata {
                id: self.as_str(),
                name: "IBGE — Censo 2022: malha e agregados por setor censitário",
                publisher: "Instituto Brasileiro de Geografia e Estatística",
                reference_url: "https://ftp.ibge.gov.br/Censos/Censo_Demografico_2022/Agregados_por_Setores_Censitarios/",
                methodology: "Malha de setores censitários (GeoPackage por UF) e arquivo \
                    Básico dos Agregados por Setores (V0001 a V0007: pessoas, domicílios, \
                    média de moradores). Data de referência do Censo: 2022-08-01.",
                limitations: &[
                    "Retrato de 2022; não há atualização intercensitária por setor.",
                    "Valores com sigilo aparecem como 'X' e ficam sem número.",
                    "Densidade é calculada pelo CrabCrawler (pessoas / área do setor na malha); não é publicada no arquivo Básico.",
                    "Renda por setor não está disponível na pasta atual do IBGE.",
                ],
            },
            Self::SeadeIpvs => SourceMetadata {
                id: self.as_str(),
                name: "Seade — Índice Paulista de Vulnerabilidade Social (IPVS) versão 2022",
                publisher: "Fundação Sistema Estadual de Análise de Dados",
                reference_url: "https://dadosabertos.sp.gov.br/dataset/seade-ipvs-versao-2022",
                methodology: "Classificação dos setores censitários do Estado de SP em grupos \
                    de vulnerabilidade, a partir de renda e ciclo de vida das famílias \
                    (Censo 2022). O grupo é gravado como a fonte publica.",
                limitations: &[
                    "Só cobre o Estado de São Paulo.",
                    "É uma classificação relativa entre setores, não uma medida de renda.",
                    "Setores sem população suficiente podem não ter grupo.",
                ],
            },
            Self::GeoSampa => SourceMetadata {
                id: self.as_str(),
                name: "GeoSampa — Mapa Digital da Cidade de São Paulo",
                publisher: "Prefeitura do Município de São Paulo",
                reference_url: "https://geosampa.prefeitura.sp.gov.br",
                methodology: "Camadas de equipamentos urbanos lidas pelo serviço WFS oficial \
                    (wfs.geosampa.prefeitura.sp.gov.br), uma camada por tipo de equipamento.",
                limitations: &[
                    "Só cobre o município de São Paulo.",
                    "Cada camada tem a própria data de atualização; nem todo tipo de equipamento foi importado.",
                    "Equipamentos de municípios vizinhos não aparecem, mesmo perto da divisa.",
                ],
            },
            Self::SgbRiskSectors => SourceMetadata {
                id: self.as_str(),
                name: "SGB — Setorização de Áreas de Risco Geológico",
                publisher: "Serviço Geológico do Brasil",
                reference_url: "https://geoportal.sgb.gov.br/server/rest/services/gestaoterritorial/risco/MapServer/0",
                methodology: "Polígonos de setores de risco alto e muito alto em áreas ocupadas, \
                    mapeados em campo pelo SGB, com tipologias COBRADE e grau de risco da \
                    própria fonte.",
                limitations: &[
                    "Mapeia setores de risco alto e muito alto em áreas com moradias; não é um mapa de suscetibilidade do município inteiro.",
                    "Cada município tem a própria data de mapeamento (São Bernardo do Campo: 2014).",
                    "Estar fora de um setor mapeado não significa ausência de risco.",
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

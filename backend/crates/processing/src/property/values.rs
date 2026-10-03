//! Interpretação de valores soltos vindos das fontes: dinheiro, área,
//! contagens, UF, CEP, endereço, coordenada, URL. Funções puras, cada uma
//! devolvendo o motivo quando recusa um valor.

use crab_domain::{GeoPoint, PropertyType, TransactionType};

use crate::normalize::slugify;

/// Limites de sanidade. Valores fora deles são tratados como erro de
/// digitação ou de integração, não como imóvel real.
pub mod limits {
    pub const SALE_PRICE: (f64, f64) = (1_000.0, 1_000_000_000.0);
    pub const RENT_PRICE: (f64, f64) = (50.0, 10_000_000.0);
    pub const CONDOMINIUM_FEE: (f64, f64) = (0.0, 1_000_000.0);
    pub const PROPERTY_TAX_YEARLY: (f64, f64) = (0.0, 50_000_000.0);
    pub const LIVING_AREA_M2: (f64, f64) = (1.0, 1_000_000.0);
    pub const LOT_AREA_M2: (f64, f64) = (1.0, 100_000_000.0);
    pub const ROOMS: u16 = 50;
    pub const PARKING: u16 = 200;
    pub const URL_LEN: usize = 2048;
    pub const TITLE_LEN: usize = 300;
    pub const DESCRIPTION_LEN: usize = 10_000;
    pub const NOTES_LEN: usize = 5_000;
    pub const IMAGES: usize = 100;
}

fn clean(text: &str) -> Option<String> {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (!collapsed.is_empty()).then_some(collapsed)
}

/// Texto livre: tira espaços repetidos; `None` se vazio.
pub fn clean_text(text: Option<&str>) -> Option<String> {
    text.and_then(clean)
}

/// Texto longo (descrição, observações): mantém quebras de linha, tira
/// espaços repetidos em cada linha e linhas vazias em excesso.
pub fn clean_text_keep_lines(text: Option<&str>) -> Option<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text?.lines() {
        let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if line.is_empty() && out.last().is_none_or(|l| l.is_empty()) {
            continue;
        }
        out.push(line);
    }
    while out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
    }
    (!out.is_empty()).then(|| out.join("\n"))
}

/// Finalidade(s) de um anúncio. "Sale/Rent" vira as duas.
pub fn parse_transaction(text: &str) -> Option<Vec<TransactionType>> {
    let slug = slugify(text);
    let sale = [
        "venda", "vende", "vender", "comprar", "compra", "sale", "for-sale", "sell",
    ];
    let rent = [
        "aluguel", "alugar", "aluga", "locacao", "rent", "for-rent", "rental", "lease",
    ];
    let both = [
        "sale-rent",
        "for-sale-for-rent",
        "venda-aluguel",
        "venda-e-aluguel",
        "venda-locacao",
        "venda-e-locacao",
    ];
    if both.contains(&slug.as_str()) {
        Some(vec![TransactionType::Sale, TransactionType::Rent])
    } else if sale.contains(&slug.as_str()) {
        Some(vec![TransactionType::Sale])
    } else if rent.contains(&slug.as_str()) {
        Some(vec![TransactionType::Rent])
    } else {
        None
    }
}

/// Tipo do imóvel. Devolve `None` quando o texto não é reconhecido (o
/// chamador decide entre `Other` e aviso).
///
/// Cobre os rótulos do VRSync ("Residential / Apartment"...) e os termos
/// comuns em português e inglês.
pub fn parse_property_type(text: &str) -> Option<PropertyType> {
    let slug = slugify(text);
    // VRSync usa "Residential / X" e "Commercial / X".
    let (prefix, body) = match slug.split_once('-') {
        Some((p @ ("residential" | "commercial"), rest)) => (Some(p), rest.to_string()),
        _ => (None, slug.clone()),
    };
    let t = match body.as_str() {
        "apartamento" | "apartment" | "apto" | "apt" | "ap" | "apartamento-padrao" => {
            PropertyType::Apartment
        }
        "casa" | "house" | "home" | "sobrado" | "casa-terrea" | "village-house"
        | "casa-de-vila" | "edicula" => PropertyType::House,
        "condo" | "casa-de-condominio" | "casa-em-condominio" | "condominio" => {
            PropertyType::CondoHouse
        }
        "cobertura" | "penthouse" => PropertyType::Penthouse,
        "studio" | "estudio" | "kitnet" | "kitinete" | "quitinete" | "loft" => PropertyType::Studio,
        "flat" | "apart-hotel" => PropertyType::Flat,
        "terreno" | "lote" | "land" | "land-lot" | "lot" => PropertyType::Land,
        "fazenda" | "sitio" | "chacara" | "farm" | "farm-ranch" | "agricultural" | "rural" => {
            PropertyType::Rural
        }
        "sala"
        | "sala-comercial"
        | "loja"
        | "escritorio"
        | "office"
        | "business"
        | "consultorio"
        | "galpao"
        | "industrial"
        | "building"
        | "garage"
        | "hotel"
        | "predio"
        | "edificio-comercial"
        | "edificio-residencial"
        | "ponto-comercial"
        | "comercial"
        | "commercial" => PropertyType::Commercial,
        _ => return None,
    };
    // "Commercial / Land Lot" é terreno comercial: continua terreno.
    Some(match (prefix, t) {
        (Some("commercial"), PropertyType::Apartment | PropertyType::House) => {
            PropertyType::Commercial
        }
        _ => t,
    })
}

/// Moeda: só BRL é aceita. Não há conversão de câmbio.
pub fn check_currency(text: Option<&str>) -> Result<(), String> {
    match text.map(|t| t.trim().to_uppercase()) {
        None => Ok(()),
        Some(c) if c.is_empty() || c == "BRL" || c == "R$" || c == "REAL" || c == "REAIS" => Ok(()),
        Some(c) => Err(format!("moeda não suportada: {c} (só BRL)")),
    }
}

/// Número em formato brasileiro ou internacional.
///
/// `"620000"`, `"620.000"`, `"620.000,00"`, `"R$ 620.000,00"`,
/// `"620,000.00"`, `"80,5"` → valor numérico.
pub fn parse_decimal(text: &str) -> Result<f64, String> {
    let original = text.trim();
    let mut s: String = original
        .trim_start_matches("R$")
        .trim_start_matches("BRL")
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '\u{a0}')
        .collect();
    if s.is_empty() {
        return Err("valor vazio".into());
    }
    if s.starts_with('-') {
        return Err(format!("valor negativo: {original}"));
    }
    if !s
        .chars()
        .all(|c| c.is_ascii_digit() || c == '.' || c == ',')
    {
        return Err(format!("valor não numérico: {original}"));
    }
    let last_dot = s.rfind('.');
    let last_comma = s.rfind(',');
    s = match (last_dot, last_comma) {
        // os dois: o que vem por último é o separador decimal
        (Some(d), Some(c)) if c > d => s.replace('.', "").replace(',', "."),
        (Some(_), Some(_)) => s.replace(',', ""),
        (None, Some(c)) => {
            let decimals = s.len() - c - 1;
            if s.matches(',').count() == 1 && decimals != 3 {
                s.replace(',', ".")
            } else {
                s.replace(',', "")
            }
        }
        (Some(d), None) => {
            let decimals = s.len() - d - 1;
            if s.matches('.').count() == 1 && decimals != 3 {
                s
            } else {
                s.replace('.', "")
            }
        }
        (None, None) => s,
    };
    s.parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .ok_or_else(|| format!("valor não numérico: {original}"))
}

/// Valor em reais dentro de um intervalo plausível.
pub fn parse_money(text: &str, bounds: (f64, f64)) -> Result<f64, String> {
    let value = parse_decimal(text)?;
    if value < bounds.0 || value > bounds.1 {
        return Err(format!(
            "valor fora do intervalo plausível ({} a {}): {}",
            bounds.0,
            bounds.1,
            text.trim()
        ));
    }
    Ok(value)
}

/// Área em m². Aceita `"80"`, `"80 m²"`, `"80,5m2"`; `unit` pode ser
/// `"square metres"` (VRSync), `"m2"`, `"m²"` ou `"hectare"`.
pub fn parse_area(text: &str, unit: Option<&str>, bounds: (f64, f64)) -> Result<f64, String> {
    let lower = text.trim().to_lowercase();
    let (number, inline_unit) = match lower.find(|c: char| c.is_alphabetic() || c == '²') {
        Some(i) => (lower[..i].trim(), Some(lower[i..].trim().to_string())),
        None => (lower.as_str(), None),
    };
    let unit = inline_unit.or_else(|| unit.map(|u| u.trim().to_lowercase()));
    let factor = match unit.as_deref().map(slugify).as_deref() {
        None
        | Some("")
        | Some("m2")
        | Some("m")
        | Some("square-metres")
        | Some("square-meters")
        | Some("metros-quadrados")
        | Some("sqm") => 1.0,
        Some("ha") | Some("hectare") | Some("hectares") => 10_000.0,
        Some(other) => return Err(format!("unidade de área não suportada: {other}")),
    };
    let value = parse_decimal(number)? * factor;
    if value < bounds.0 || value > bounds.1 {
        return Err(format!("área fora do intervalo plausível: {}", text.trim()));
    }
    Ok(value)
}

/// Contagem (quartos, vagas...). Aceita `"2"` e `"2 quartos"`.
pub fn parse_count(text: &str, max: u16) -> Result<u16, String> {
    let digits: String = text
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    let value: u16 = digits
        .parse()
        .map_err(|_| format!("contagem inválida: {}", text.trim()))?;
    if value > max {
        return Err(format!("contagem acima do plausível ({max}): {value}"));
    }
    Ok(value)
}

const UFS: [(&str, &str); 27] = [
    ("AC", "acre"),
    ("AL", "alagoas"),
    ("AP", "amapa"),
    ("AM", "amazonas"),
    ("BA", "bahia"),
    ("CE", "ceara"),
    ("DF", "distrito-federal"),
    ("ES", "espirito-santo"),
    ("GO", "goias"),
    ("MA", "maranhao"),
    ("MT", "mato-grosso"),
    ("MS", "mato-grosso-do-sul"),
    ("MG", "minas-gerais"),
    ("PA", "para"),
    ("PB", "paraiba"),
    ("PR", "parana"),
    ("PE", "pernambuco"),
    ("PI", "piaui"),
    ("RJ", "rio-de-janeiro"),
    ("RN", "rio-grande-do-norte"),
    ("RS", "rio-grande-do-sul"),
    ("RO", "rondonia"),
    ("RR", "roraima"),
    ("SC", "santa-catarina"),
    ("SP", "sao-paulo"),
    ("SE", "sergipe"),
    ("TO", "tocantins"),
];

/// UF a partir da sigla ou do nome: `"sp"`, `"São Paulo"`, `"Sao Paulo"` → `"SP"`.
pub fn parse_uf(text: &str) -> Option<&'static str> {
    let trimmed = text.trim();
    let upper = trimmed.to_uppercase();
    let slug = slugify(trimmed);
    UFS.iter()
        .find(|(code, name)| *code == upper || *name == slug)
        .map(|(code, _)| *code)
}

/// CEP com 8 dígitos (`"09640-000"` → `"09640000"`).
pub fn parse_cep(text: &str) -> Result<String, String> {
    let trimmed = text.trim();
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_digit() || c == '-' || c == '.' || c == ' ')
    {
        return Err(format!("CEP inválido: {trimmed}"));
    }
    let digits: String = trimmed.chars().filter(char::is_ascii_digit).collect();
    if digits.len() != 8 || digits == "00000000" {
        return Err(format!("CEP inválido: {trimmed}"));
    }
    Ok(digits)
}

/// Expande abreviações comuns no início do logradouro.
/// `"R. Bela Cintra"` → `"Rua Bela Cintra"`, `"av  kennedy"` → `"Avenida kennedy"`.
pub fn normalize_street(text: &str) -> Option<String> {
    let cleaned = clean(text)?;
    let (first, rest) = cleaned.split_once(' ').unwrap_or((cleaned.as_str(), ""));
    let expanded = match first.trim_end_matches('.').to_lowercase().as_str() {
        "r" => Some("Rua"),
        "av" => Some("Avenida"),
        "al" => Some("Alameda"),
        "trav" | "tv" => Some("Travessa"),
        "pc" | "pca" | "pç" | "pça" => Some("Praça"),
        "estr" => Some("Estrada"),
        "rod" => Some("Rodovia"),
        _ => None,
    };
    match (expanded, rest.is_empty()) {
        (Some(word), false) => Some(format!("{word} {rest}")),
        _ => Some(cleaned),
    }
}

/// Número do imóvel; "s/n" e equivalentes viram `None`.
pub fn normalize_number(text: &str) -> Option<String> {
    let cleaned = clean(text)?;
    let slug = slugify(&cleaned);
    if matches!(slug.as_str(), "s-n" | "sn" | "sem-numero" | "0") {
        return None;
    }
    Some(cleaned)
}

/// Latitude/longitude em texto (aceita vírgula decimal). Recusa (0, 0) e
/// pontos fora do Brasil, que costumam ser erro de integração.
pub fn parse_point(lat: &str, lon: &str) -> Result<GeoPoint, String> {
    let parse = |v: &str| {
        v.trim()
            .replace(',', ".")
            .parse::<f64>()
            .ok()
            .filter(|n| n.is_finite())
    };
    let (Some(lat_v), Some(lon_v)) = (parse(lat), parse(lon)) else {
        return Err(format!("coordenada inválida: {lat}, {lon}"));
    };
    if lat_v == 0.0 && lon_v == 0.0 {
        return Err("coordenada (0, 0)".into());
    }
    let point =
        GeoPoint::new(lat_v, lon_v).ok_or_else(|| format!("coordenada inválida: {lat}, {lon}"))?;
    // Caixa que envolve o território brasileiro.
    if !(-34.0..=5.5).contains(&lat_v) || !(-74.5..=-28.5).contains(&lon_v) {
        return Err(format!("coordenada fora do Brasil: {lat_v}, {lon_v}"));
    }
    Ok(point)
}

/// URL externa guardada só como referência: http(s), sem credenciais,
/// tamanho limitado. Nunca é acessada pelo CrabCrawler.
pub fn parse_external_url(text: &str) -> Result<String, String> {
    let trimmed = text.trim();
    if trimmed.len() > limits::URL_LEN {
        return Err("URL longa demais".into());
    }
    let url = url::Url::parse(trimmed).map_err(|_| format!("URL inválida: {trimmed}"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(format!("URL precisa ser http ou https: {trimmed}"));
    }
    if url.host_str().is_none_or(|h| !h.contains('.')) {
        return Err(format!("URL sem domínio válido: {trimmed}"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("URL com credenciais não é aceita".into());
    }
    Ok(url.to_string())
}

/// Corta texto longo num limite de caracteres. Devolve se cortou.
pub fn truncate(text: String, max_chars: usize) -> (String, bool) {
    if text.chars().count() <= max_chars {
        (text, false)
    } else {
        (text.chars().take(max_chars).collect(), true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn property_types_from_many_spellings() {
        for t in [
            "APARTAMENTO",
            "Apartment",
            "apto",
            "APT",
            "Residential / Apartment",
        ] {
            assert_eq!(parse_property_type(t), Some(PropertyType::Apartment), "{t}");
        }
        assert_eq!(
            parse_property_type("Residential / Sobrado"),
            Some(PropertyType::House)
        );
        assert_eq!(
            parse_property_type("Residential / Condo"),
            Some(PropertyType::CondoHouse)
        );
        assert_eq!(parse_property_type("Kitnet"), Some(PropertyType::Studio));
        assert_eq!(
            parse_property_type("Commercial / Land Lot"),
            Some(PropertyType::Land)
        );
        assert_eq!(
            parse_property_type("Commercial / Office"),
            Some(PropertyType::Commercial)
        );
        assert_eq!(
            parse_property_type("Cobertura"),
            Some(PropertyType::Penthouse)
        );
        assert_eq!(parse_property_type("barco"), None);
    }

    #[test]
    fn transactions() {
        assert_eq!(
            parse_transaction("For Sale"),
            Some(vec![TransactionType::Sale])
        );
        assert_eq!(
            parse_transaction("VENDA"),
            Some(vec![TransactionType::Sale])
        );
        assert_eq!(
            parse_transaction("Locação"),
            Some(vec![TransactionType::Rent])
        );
        assert_eq!(
            parse_transaction("Sale/Rent"),
            Some(vec![TransactionType::Sale, TransactionType::Rent])
        );
        assert_eq!(parse_transaction("permuta"), None);
    }

    #[test]
    fn money_formats() {
        assert_eq!(parse_decimal("620000").unwrap(), 620_000.0);
        assert_eq!(parse_decimal("620.000").unwrap(), 620_000.0);
        assert_eq!(parse_decimal("R$ 620.000,00").unwrap(), 620_000.0);
        assert_eq!(parse_decimal("620,000.00").unwrap(), 620_000.0);
        assert_eq!(parse_decimal("1.250.000").unwrap(), 1_250_000.0);
        assert_eq!(parse_decimal("980,50").unwrap(), 980.5);
        assert_eq!(parse_decimal("620000.5").unwrap(), 620_000.5);
        assert!(parse_decimal("-10").is_err());
        assert!(parse_decimal("abc").is_err());
        assert!(parse_decimal("").is_err());
        assert!(parse_money("5", limits::SALE_PRICE).is_err());
        assert!(parse_money("99999999999", limits::SALE_PRICE).is_err());
        assert!(check_currency(Some("USD")).is_err());
        assert!(check_currency(Some("brl")).is_ok());
    }

    #[test]
    fn areas() {
        assert_eq!(
            parse_area("80", Some("square metres"), limits::LIVING_AREA_M2).unwrap(),
            80.0
        );
        assert_eq!(
            parse_area("80,5 m²", None, limits::LIVING_AREA_M2).unwrap(),
            80.5
        );
        assert_eq!(
            parse_area("2 ha", None, limits::LOT_AREA_M2).unwrap(),
            20_000.0
        );
        assert!(parse_area("80", Some("square feet"), limits::LIVING_AREA_M2).is_err());
        assert!(parse_area("0", None, limits::LIVING_AREA_M2).is_err());
    }

    #[test]
    fn counts() {
        assert_eq!(parse_count("2", limits::ROOMS).unwrap(), 2);
        assert_eq!(parse_count("3 quartos", limits::ROOMS).unwrap(), 3);
        assert!(parse_count("muitos", limits::ROOMS).is_err());
        assert!(parse_count("500", limits::ROOMS).is_err());
    }

    #[test]
    fn uf_and_cep() {
        assert_eq!(parse_uf("sp"), Some("SP"));
        assert_eq!(parse_uf("São Paulo"), Some("SP"));
        assert_eq!(parse_uf("Sao Paulo"), Some("SP"));
        assert_eq!(parse_uf("XX"), None);
        assert_eq!(parse_cep("09640-000").unwrap(), "09640000");
        assert_eq!(parse_cep("01415.003").unwrap(), "01415003");
        assert!(parse_cep("0964-000").is_err());
        assert!(parse_cep("ABCDE-123").is_err());
        assert!(parse_cep("00000-000").is_err());
    }

    #[test]
    fn addresses() {
        assert_eq!(
            normalize_street("R.  Bela   Cintra").unwrap(),
            "Rua Bela Cintra"
        );
        assert_eq!(normalize_street("av kennedy").unwrap(), "Avenida kennedy");
        assert_eq!(normalize_street("Rua Joá").unwrap(), "Rua Joá");
        assert_eq!(normalize_number("s/n"), None);
        assert_eq!(normalize_number(" 539 ").as_deref(), Some("539"));
    }

    #[test]
    fn points_and_urls() {
        assert!(parse_point("-23,5531", "-46,6598").is_ok());
        assert!(parse_point("0", "0").is_err());
        assert!(parse_point("48.85", "2.35").is_err());
        assert!(parse_point("abc", "1").is_err());
        assert!(parse_external_url("https://www.zapimoveis.com.br/imovel/123").is_ok());
        assert!(parse_external_url("javascript:alert(1)").is_err());
        assert!(parse_external_url("ftp://example.com/x").is_err());
        assert!(parse_external_url("https://user:pass@example.com/").is_err());
        assert!(parse_external_url("http://localhost/x").is_err());
    }
}

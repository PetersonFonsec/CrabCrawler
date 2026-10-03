//! Leitura defensiva de XML de terceiros.
//!
//! - DTD (`<!DOCTYPE`) é recusado: sem DTD não há entidades externas (XXE)
//!   nem expansão de entidades em cascata ("billion laughs");
//! - só as 5 entidades predefinidas do XML e referências numéricas são
//!   aceitas;
//! - profundidade, número de elementos e tamanho de texto têm limite;
//! - nada no conteúdo é executado ou buscado.
//!
//! O documento é lido em fluxo e entregue elemento a elemento pelo
//! `on_element`, então um feed grande não precisa caber inteiro em memória
//! como árvore.

use quick_xml::escape::resolve_predefined_entity;
use quick_xml::events::Event;
use quick_xml::{Reader, XmlVersion};
use serde_json::{Map, Value};

use crate::CrawlError;

/// Limites de leitura.
#[derive(Debug, Clone, Copy)]
pub struct XmlLimits {
    pub max_depth: usize,
    pub max_elements: usize,
    pub max_text_bytes: usize,
}

impl Default for XmlLimits {
    fn default() -> Self {
        Self {
            max_depth: 32,
            max_elements: 20_000_000,
            max_text_bytes: 1_000_000,
        }
    }
}

/// Elemento XML já lido (nome local, sem prefixo de namespace).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Element {
    pub name: String,
    pub attributes: Vec<(String, String)>,
    pub children: Vec<Element>,
    pub text: String,
}

impl Element {
    pub fn child(&self, name: &str) -> Option<&Element> {
        self.children.iter().find(|c| c.name == name)
    }

    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Element> {
        self.children.iter().filter(move |c| c.name == name)
    }

    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    /// Texto do elemento, sem espaços nas pontas; `None` se vazio.
    pub fn text_trimmed(&self) -> Option<String> {
        let t = self.text.trim();
        (!t.is_empty()).then(|| t.to_string())
    }

    /// Texto de um filho direto.
    pub fn child_text(&self, name: &str) -> Option<String> {
        self.child(name).and_then(Element::text_trimmed)
    }

    /// Representação JSON para guardar o payload original: atributos como
    /// `@nome`, texto como `#text` (ou o próprio valor, se não houver mais
    /// nada) e filhos repetidos como lista.
    pub fn to_json(&self) -> Value {
        let text = self.text.trim();
        if self.attributes.is_empty() && self.children.is_empty() {
            return Value::String(text.to_string());
        }
        let mut map = Map::new();
        for (k, v) in &self.attributes {
            map.insert(format!("@{k}"), Value::String(v.clone()));
        }
        if !text.is_empty() {
            map.insert("#text".into(), Value::String(text.to_string()));
        }
        for child in &self.children {
            let value = child.to_json();
            match map.get_mut(&child.name) {
                Some(Value::Array(list)) => list.push(value),
                Some(existing) => {
                    let first = existing.take();
                    *existing = Value::Array(vec![first, value]);
                }
                None => {
                    map.insert(child.name.clone(), value);
                }
            }
        }
        Value::Object(map)
    }
}

/// Lê o documento e chama `on_element(caminho, elemento)` sempre que um
/// elemento cujo caminho (nomes locais desde a raiz) está em `capture`
/// termina. Elementos capturados não ficam presos na memória do pai.
///
/// Devolve o nome local e o namespace da raiz.
pub fn read_document(
    bytes: &[u8],
    capture: &[&[&str]],
    limits: XmlLimits,
    mut on_element: impl FnMut(&[String], Element) -> Result<(), CrawlError>,
) -> Result<(String, Option<String>), CrawlError> {
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(false);
    reader.config_mut().check_end_names = true;

    let mut buf = Vec::new();
    let mut stack: Vec<Element> = Vec::new();
    let mut path: Vec<String> = Vec::new();
    let mut root: Option<(String, Option<String>)> = None;
    let mut elements = 0usize;

    let err = |reader: &Reader<&[u8]>, msg: String| {
        CrawlError::Parse(format!(
            "XML inválido (posição {}): {msg}",
            reader.buffer_position()
        ))
    };

    loop {
        let event = reader
            .read_event_into(&mut buf)
            .map_err(|e| err(&reader, e.to_string()))?;
        match event {
            Event::DocType(_) => {
                return Err(err(&reader, "DTD (<!DOCTYPE>) não é aceito".into()));
            }
            Event::Start(ref start) | Event::Empty(ref start) => {
                let is_empty = matches!(event, Event::Empty(_));
                elements += 1;
                if elements > limits.max_elements {
                    return Err(err(&reader, "elementos demais".into()));
                }
                if stack.len() >= limits.max_depth {
                    return Err(err(&reader, "aninhamento profundo demais".into()));
                }
                let name = String::from_utf8_lossy(start.local_name().as_ref()).into_owned();
                let mut attributes = Vec::new();
                let mut namespace = None;
                for attr in start.attributes() {
                    let attr = attr.map_err(|e| err(&reader, e.to_string()))?;
                    let key = String::from_utf8_lossy(attr.key.as_ref()).into_owned();
                    let value = attr
                        .decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())
                        .map_err(|e| err(&reader, e.to_string()))?
                        .into_owned();
                    if key == "xmlns" {
                        namespace = Some(value.clone());
                    }
                    let local =
                        String::from_utf8_lossy(attr.key.local_name().as_ref()).into_owned();
                    attributes.push((local, value));
                }
                if root.is_none() {
                    root = Some((name.clone(), namespace));
                } else if stack.is_empty() {
                    return Err(err(&reader, "mais de um elemento raiz".into()));
                }
                path.push(name.clone());
                stack.push(Element {
                    name,
                    attributes,
                    children: vec![],
                    text: String::new(),
                });
                if is_empty {
                    close(&mut stack, &mut path, capture, &mut on_element)?;
                }
            }
            Event::End(_) => {
                close(&mut stack, &mut path, capture, &mut on_element)?;
            }
            Event::Text(text) => {
                let t = text
                    .xml10_content()
                    .map_err(|e| err(&reader, e.to_string()))?;
                push_text(&mut stack, &t, limits, &reader)?;
            }
            Event::CData(data) => {
                let t = data.decode().map_err(|e| err(&reader, e.to_string()))?;
                push_text(&mut stack, &t, limits, &reader)?;
            }
            Event::GeneralRef(r) => {
                let resolved = match r
                    .resolve_char_ref()
                    .map_err(|e| err(&reader, e.to_string()))?
                {
                    Some(c) => c.to_string(),
                    None => {
                        let name = r.decode().map_err(|e| err(&reader, e.to_string()))?;
                        resolve_predefined_entity(&name)
                            .ok_or_else(|| {
                                err(&reader, format!("entidade desconhecida: &{name};"))
                            })?
                            .to_string()
                    }
                };
                push_text(&mut stack, &resolved, limits, &reader)?;
            }
            Event::Eof => break,
            Event::Decl(_) | Event::PI(_) | Event::Comment(_) => {}
        }
        buf.clear();
    }
    if !stack.is_empty() {
        return Err(CrawlError::Parse(
            "XML inválido: documento terminou antes de fechar os elementos".into(),
        ));
    }
    root.ok_or_else(|| CrawlError::Parse("XML inválido: documento vazio".into()))
}

fn push_text(
    stack: &mut [Element],
    text: &str,
    limits: XmlLimits,
    reader: &Reader<&[u8]>,
) -> Result<(), CrawlError> {
    match stack.last_mut() {
        Some(el) => {
            if el.text.len() + text.len() > limits.max_text_bytes {
                return Err(CrawlError::Parse(format!(
                    "XML inválido (posição {}): texto longo demais em <{}>",
                    reader.buffer_position(),
                    el.name
                )));
            }
            el.text.push_str(text);
            Ok(())
        }
        // Espaço fora da raiz é permitido; qualquer outra coisa não.
        None if text.trim().is_empty() => Ok(()),
        None => Err(CrawlError::Parse(
            "XML inválido: texto fora do elemento raiz".into(),
        )),
    }
}

fn close(
    stack: &mut Vec<Element>,
    path: &mut Vec<String>,
    capture: &[&[&str]],
    on_element: &mut impl FnMut(&[String], Element) -> Result<(), CrawlError>,
) -> Result<(), CrawlError> {
    let element = stack
        .pop()
        .ok_or_else(|| CrawlError::Parse("XML inválido: fechamento sem abertura".into()))?;
    let captured = capture
        .iter()
        .any(|c| c.len() == path.len() && c.iter().zip(path.iter()).all(|(a, b)| a == b));
    if captured {
        on_element(path, element)?;
    } else if let Some(parent) = stack.last_mut() {
        parent.children.push(element);
    }
    path.pop();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect(xml: &str) -> Result<Vec<Element>, CrawlError> {
        let mut out = vec![];
        read_document(
            xml.as_bytes(),
            &[&["root", "item"]],
            XmlLimits::default(),
            |_, e| {
                out.push(e);
                Ok(())
            },
        )?;
        Ok(out)
    }

    #[test]
    fn reads_items_with_entities_and_cdata() {
        let items = collect(
            r#"<?xml version="1.0"?><root xmlns="urn:x"><item a="1 &amp; 2"><t>A &amp; B &#233;</t><d><![CDATA[<b>x</b>]]></d></item><item/></root>"#,
        )
        .unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].attr("a"), Some("1 & 2"));
        assert_eq!(items[0].child_text("t").as_deref(), Some("A & B é"));
        assert_eq!(items[0].child_text("d").as_deref(), Some("<b>x</b>"));
    }

    #[test]
    fn rejects_doctype_and_xxe() {
        let xxe = r#"<?xml version="1.0"?><!DOCTYPE root [<!ENTITY xxe SYSTEM "file:///etc/passwd">]><root><item>&xxe;</item></root>"#;
        let err = collect(xxe).unwrap_err().to_string();
        assert!(err.contains("DTD"), "{err}");
    }

    #[test]
    fn rejects_unknown_entities_and_malformed() {
        assert!(collect("<root><item>&foo;</item></root>").is_err());
        assert!(collect("<root><item></root>").is_err());
        assert!(collect("<root><item>").is_err());
        assert!(collect("").is_err());
        assert!(collect("<root/><other/>").is_err());
    }

    #[test]
    fn enforces_depth_limit() {
        let deep = format!("<root>{}{}</root>", "<a>".repeat(40), "</a>".repeat(40));
        assert!(collect(&deep).is_err());
    }

    #[test]
    fn json_payload_keeps_attributes_and_lists() {
        let mut items = vec![];
        read_document(
            br#"<root><item><M><I k="v">u1</I><I>u2</I></M><N>1</N></item></root>"#,
            &[&["root", "item"]],
            XmlLimits::default(),
            |_, e| {
                items.push(e);
                Ok(())
            },
        )
        .unwrap();
        let json = items[0].to_json();
        assert_eq!(json["N"], "1");
        assert_eq!(json["M"]["I"][0]["@k"], "v");
        assert_eq!(json["M"]["I"][0]["#text"], "u1");
        assert_eq!(json["M"]["I"][1], "u2");
    }
}

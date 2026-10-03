//! Leitura de tabelas delimitadas (CSV do IBGE e do Seade), dentro ou fora
//! de um ZIP. As fontes variam em separador (`;` ou `,`) e codificação
//! (UTF-8 ou Latin-1); ambos são detectados.

use std::io::Read;
use std::path::Path;

use crate::CrawlError;

/// Tabela já decodificada: cabeçalho + linhas.
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub header: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

impl Table {
    /// Índice de uma coluna pelo nome, sem diferenciar maiúsculas.
    pub fn column(&self, name: &str) -> Option<usize> {
        self.header
            .iter()
            .position(|h| h.trim().eq_ignore_ascii_case(name))
    }
}

/// Lê o conteúdo bruto de um arquivo. Se for ZIP, devolve a primeira entrada
/// com a extensão pedida.
pub fn read_maybe_zipped(path: &Path, extension: &str) -> Result<(Vec<u8>, String), CrawlError> {
    let bytes = std::fs::read(path)?;
    if !bytes.starts_with(b"PK\x03\x04") {
        return Ok((bytes, super::raw::file_name(path)));
    }
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| CrawlError::Parse(format!("zip inválido: {e}")))?;
    let suffix = format!(".{}", extension.to_ascii_lowercase());
    let index = (0..archive.len())
        .find(|&i| {
            archive
                .by_index(i)
                .map(|f| f.name().to_ascii_lowercase().ends_with(&suffix))
                .unwrap_or(false)
        })
        .ok_or_else(|| CrawlError::Parse(format!("nenhum arquivo {suffix} dentro do zip")))?;
    let mut entry = archive
        .by_index(index)
        .map_err(|e| CrawlError::Parse(format!("zip: {e}")))?;
    let name = entry.name().to_string();
    let mut out = Vec::new();
    entry.read_to_end(&mut out)?;
    Ok((out, name))
}

/// UTF-8 quando válido; senão Latin-1 (cada byte é um caractere).
pub fn decode_text(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes.iter().map(|&b| b as char).collect(),
    }
}

/// Faz o parse de um texto delimitado. O separador é o que mais aparece no
/// cabeçalho entre `;`, `,` e tab. Aspas duplas protegem separadores.
/// `keep` decide, pela linha já separada, se ela entra (filtra cedo para não
/// guardar o Brasil inteiro em memória).
pub fn parse_delimited(text: &str, keep: impl Fn(&[String]) -> bool) -> Result<Table, CrawlError> {
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let header_line = lines
        .next()
        .ok_or_else(|| CrawlError::Parse("arquivo vazio".into()))?;
    let sep = [';', ',', '\t']
        .into_iter()
        .max_by_key(|c| header_line.matches(*c).count())
        .unwrap_or(';');
    let header = split_line(header_line, sep);
    let mut rows = Vec::new();
    for line in lines {
        let row = split_line(line, sep);
        if keep(&row) {
            rows.push(row);
        }
    }
    Ok(Table { header, rows })
}

fn split_line(line: &str, sep: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            c if c == sep && !quoted => out.push(std::mem::take(&mut field).trim().to_string()),
            c => field.push(c),
        }
    }
    out.push(field.trim().trim_end_matches('\r').to_string());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_separator_and_quotes() {
        let t = parse_delimited("a;b;c\n1;\"x;y\";3\n\n4;5;6\n", |_| true).unwrap();
        assert_eq!(t.header, vec!["a", "b", "c"]);
        assert_eq!(t.rows[0], vec!["1", "x;y", "3"]);
        assert_eq!(t.rows.len(), 2);
        let t = parse_delimited("A,B\n1,\"he said \"\"hi\"\"\"\n", |_| true).unwrap();
        assert_eq!(t.column("b"), Some(1));
        assert_eq!(t.rows[0][1], "he said \"hi\"");
    }

    #[test]
    fn decodes_latin1_and_bom() {
        assert_eq!(decode_text(b"S\xe3o Paulo"), "São Paulo");
        assert_eq!(decode_text("\u{feff}São".as_bytes()), "São");
    }

    #[test]
    fn filters_rows_early() {
        let t = parse_delimited("k;v\n35;1\n33;2\n", |r| r[0].starts_with("35")).unwrap();
        assert_eq!(t.rows.len(), 1);
    }
}

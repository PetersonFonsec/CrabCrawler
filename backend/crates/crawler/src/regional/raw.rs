//! Arquivos brutos: tudo que é baixado é gravado em disco antes do parse,
//! para que a importação possa ser repetida sem rede e auditada depois.
//! A versão de um dataset inclui o SHA-256 do conteúdo bruto.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::CrawlError;

/// SHA-256 em hexadecimal.
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Versão de um dataset: `<nome publicado>@sha256:<12 primeiros hex>`.
/// O mesmo arquivo gera sempre a mesma versão; conteúdo novo gera outra.
pub fn dataset_version(published_name: &str, bytes: &[u8]) -> String {
    format!("{published_name}@sha256:{}", &sha256_hex(bytes)[..12])
}

/// Nome do arquivo sem diretório.
pub fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Diretório onde os downloads são guardados (`data/raw` por padrão).
#[derive(Debug, Clone)]
pub struct RawStore {
    dir: PathBuf,
}

impl RawStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// Grava `bytes` em `<dir>/<provider>/<name>` e devolve o caminho.
    pub async fn save(
        &self,
        provider: &str,
        name: &str,
        bytes: &[u8],
    ) -> Result<PathBuf, CrawlError> {
        let dir = self.dir.join(provider);
        tokio::fs::create_dir_all(&dir).await?;
        let path = dir.join(name);
        tokio::fs::write(&path, bytes).await?;
        tracing::info!(path = %path.display(), bytes = bytes.len(), "arquivo bruto gravado");
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_stable_and_content_sensitive() {
        let a = dataset_version("x.csv", b"abc");
        assert_eq!(a, dataset_version("x.csv", b"abc"));
        assert_ne!(a, dataset_version("x.csv", b"abd"));
        assert!(a.starts_with("x.csv@sha256:ba7816bf8f01"));
    }
}

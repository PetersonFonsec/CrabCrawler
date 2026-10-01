use std::path::PathBuf;

use async_trait::async_trait;
use crab_domain::RawListing;

use crate::{CrawlError, ListingSource};

/// Lê anúncios de um arquivo JSON local.
///
/// Serve para desenvolver normalização, enriquecimento e API sem depender de
/// um portal real. A fonte real de anúncios entra como outro `ListingSource`.
pub struct FixtureListingSource {
    path: PathBuf,
}

impl FixtureListingSource {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

#[async_trait]
impl ListingSource for FixtureListingSource {
    fn name(&self) -> &'static str {
        crab_domain::DataSource::ListingFixture.as_str()
    }

    async fn fetch(&self) -> Result<Vec<RawListing>, CrawlError> {
        let content = tokio::fs::read_to_string(&self.path).await?;
        Ok(serde_json::from_str(&content)?)
    }
}

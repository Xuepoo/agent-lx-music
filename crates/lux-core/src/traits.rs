use crate::error::SourceError;
use crate::types::{Quality, SearchResult, Source};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LyricInfo {
    pub lyric: String,
    pub tlyric: Option<String>,
    pub rlyric: Option<String>,
    pub lxlyric: Option<String>,
}

#[async_trait]
pub trait MusicSource: Send + Sync {
    fn platform(&self) -> Source;
    fn supported_qualities(&self) -> &[Quality];
    async fn search(
        &self,
        keyword: &str,
        page: usize,
        limit: usize,
    ) -> Result<SearchResult, SourceError>;
    async fn get_url(&self, song_id: &str, quality: Quality) -> Result<String, SourceError>;
    async fn get_lyric(&self, song_id: &str) -> Result<Option<LyricInfo>, SourceError>;
    async fn get_pic(&self, song_id: &str) -> Result<Option<String>, SourceError>;
}

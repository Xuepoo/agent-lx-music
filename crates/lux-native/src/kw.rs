use async_trait::async_trait;
use lux_core::error::SourceError;
use lux_core::traits::{LyricInfo, MusicSource};
use lux_core::types::{Quality, SearchResult, Source};

#[derive(Default)]
pub struct KuwoSource;

#[async_trait]
impl MusicSource for KuwoSource {
    fn platform(&self) -> Source {
        Source::Kuwo
    }

    fn supported_qualities(&self) -> &[Quality] {
        &[Quality::Q128k, Quality::Q320k, Quality::Flac]
    }

    async fn search(
        &self,
        _keyword: &str,
        _page: usize,
        _limit: usize,
    ) -> Result<SearchResult, SourceError> {
        Err(SourceError::PlatformError(
            "Kuwo native search not implemented yet".to_string(),
        ))
    }

    async fn get_url(&self, _song_id: &str, _quality: Quality) -> Result<String, SourceError> {
        Err(SourceError::PlatformError(
            "Kuwo native get_url not implemented yet".to_string(),
        ))
    }

    async fn get_lyric(&self, _song_id: &str) -> Result<Option<LyricInfo>, SourceError> {
        Err(SourceError::PlatformError(
            "Kuwo native get_lyric not implemented yet".to_string(),
        ))
    }

    async fn get_pic(&self, _song_id: &str) -> Result<Option<String>, SourceError> {
        Err(SourceError::PlatformError(
            "Kuwo native get_pic not implemented yet".to_string(),
        ))
    }
}

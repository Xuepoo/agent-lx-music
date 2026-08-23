use async_trait::async_trait;
use lux_core::error::SourceError;
use lux_core::traits::{LyricInfo, MusicSource};
use lux_core::types::{MusicInfo, Quality, SearchResult, Source};

#[derive(Default)]
pub struct NetEaseSource;

#[async_trait]
impl MusicSource for NetEaseSource {
    fn platform(&self) -> Source {
        Source::NetEase
    }

    fn supported_qualities(&self) -> &[Quality] {
        &[Quality::Q128k, Quality::Q320k, Quality::Flac]
    }

    async fn search(
        &self,
        keyword: &str,
        page: usize,
        limit: usize,
    ) -> Result<SearchResult, SourceError> {
        let offset = (page - 1) * limit;
        let url = "http://music.163.com/api/search/get/web";

        let client = crate::http::client();
        let resp = client
            .get(url)
            .query(&[
                ("s", keyword),
                ("type", "1"),
                ("offset", &offset.to_string()),
                ("limit", &limit.to_string()),
            ])
            .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64)")
            .send()
            .await
            .map_err(|e| SourceError::HttpError(e.to_string()))?;

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| SourceError::HttpError(e.to_string()))?;

        let mut list = Vec::new();
        let songs = body["result"]["songs"].as_array();
        let total = body["result"]["songCount"].as_u64().unwrap_or(0) as usize;

        if let Some(songs) = songs {
            for song in songs {
                let songmid = song["id"].as_i64().unwrap_or(0).to_string();
                let name = song["name"].as_str().unwrap_or("Unknown").to_string();
                let singer = if let Some(artists) = song["artists"].as_array() {
                    artists
                        .iter()
                        .map(|a| a["name"].as_str().unwrap_or(""))
                        .collect::<Vec<_>>()
                        .join("、")
                } else {
                    "Unknown".to_string()
                };
                let album_name = song["album"]["name"].as_str().map(|s| s.to_string());
                let album_id = song["album"]["id"].as_i64().map(|id| id.to_string());
                let duration_ms = song["duration"].as_i64().unwrap_or(0);
                let duration_secs = duration_ms / 1000;
                let interval = format!("{:02}:{:02}", duration_secs / 60, duration_secs % 60);

                list.push(MusicInfo {
                    songmid,
                    name,
                    singer,
                    source: Source::NetEase,
                    album_name,
                    album_id,
                    interval: Some(interval),
                    pic_url: song["album"]["picUrl"]
                        .as_str()
                        .or_else(|| song["album"]["pic"].as_str())
                        .or_else(|| song["album"]["artist"]["img1v1Url"].as_str())
                        .map(|s| s.to_string()),
                    hash: None,
                    extra: None,
                });
            }
        }

        Ok(SearchResult {
            page,
            limit,
            total,
            list,
        })
    }

    async fn get_url(&self, _song_id: &str, _quality: Quality) -> Result<String, SourceError> {
        Err(SourceError::PlatformError(
            "NetEase native URL resolution requires JS source bridge".to_string(),
        ))
    }

    async fn get_lyric(&self, song_id: &str) -> Result<Option<LyricInfo>, SourceError> {
        let url = format!("http://music.163.com/api/song/media?id={}", song_id);
        let client = crate::http::client();
        let resp = client
            .get(&url)
            .send()
            .await
            .map_err(|e| SourceError::HttpError(e.to_string()))?;

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| SourceError::HttpError(e.to_string()))?;

        if let Some(lyric) = body["lyric"].as_str() {
            Ok(Some(LyricInfo {
                lyric: lyric.to_string(),
                tlyric: None,
                rlyric: None,
                lxlyric: None,
            }))
        } else {
            Ok(None)
        }
    }

    async fn get_pic(&self, song_id: &str) -> Result<Option<String>, SourceError> {
        let url = format!(
            "http://music.163.com/api/song/detail/?id={}&ids=[{}]",
            song_id, song_id
        );
        let client = crate::http::client();
        let resp = client
            .get(&url)
            .send()
            .await
            .map_err(|e| SourceError::HttpError(e.to_string()))?;

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| SourceError::HttpError(e.to_string()))?;

        if let Some(pic_url) = body["songs"]
            .as_array()
            .and_then(|songs| songs.first())
            .and_then(|song| song["album"]["picUrl"].as_str())
        {
            return Ok(Some(pic_url.to_string()));
        }
        Ok(None)
    }
}

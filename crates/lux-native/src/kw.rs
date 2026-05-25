use async_trait::async_trait;
use lux_core::error::SourceError;
use lux_core::traits::{LyricInfo, MusicSource};
use lux_core::types::{MusicInfo, Quality, SearchResult, Source};

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
        keyword: &str,
        page: usize,
        limit: usize,
    ) -> Result<SearchResult, SourceError> {
        let url = "http://search.kuwo.cn/r.s";
        let client = reqwest::Client::new();
        let resp = client
            .get(url)
            .query(&[
                ("client", "kt"),
                ("all", keyword),
                ("pn", &(page - 1).to_string()),
                ("rn", &limit.to_string()),
                ("rformat", "json"),
                ("encoding", "utf8"),
                ("ver", "kwplayer_ar_9.2.2.1"),
                ("vipver", "1"),
                ("show_copyright_off", "1"),
                ("newver", "1"),
                ("ft", "music"),
            ])
            .send()
            .await
            .map_err(|e| SourceError::HttpError(e.to_string()))?;

        let text = resp
            .text()
            .await
            .map_err(|e| SourceError::HttpError(e.to_string()))?;

        // Kuwo response might contain single quotes instead of double quotes, or leading/trailing whitespace.
        // Let's replace single quotes with double quotes for valid JSON parsing if needed, but usually it works.
        let parsed_json: serde_json::Value = serde_json::from_str(&text)
            .or_else(|_| {
                let fixed = text.replace('\'', "\"");
                serde_json::from_str(&fixed)
            })
            .map_err(|e| SourceError::HttpError(format!("JSON parse error: {}", e)))?;

        let mut list = Vec::new();
        let abslist = parsed_json["abslist"].as_array();
        let total = parsed_json["TOTAL"]
            .as_str()
            .unwrap_or("0")
            .parse::<usize>()
            .unwrap_or(0);

        if let Some(abslist) = abslist {
            for item in abslist {
                let songmid = item["MUSICRID"]
                    .as_str()
                    .unwrap_or("")
                    .replace("MUSIC_", "");
                if songmid.is_empty() {
                    continue;
                }

                let name = item["SONGNAME"].as_str().unwrap_or("Unknown").to_string();
                let singer = item["ARTIST"].as_str().unwrap_or("Unknown").to_string();
                let album_name = item["ALBUM"].as_str().map(|s| s.to_string());
                let album_id = item["ALBUMID"].as_str().map(|s| s.to_string());

                let duration_secs = item["DURATION"]
                    .as_str()
                    .unwrap_or("0")
                    .parse::<i64>()
                    .unwrap_or(0);
                let interval = format!("{:02}:{:02}", duration_secs / 60, duration_secs % 60);

                list.push(MusicInfo {
                    songmid,
                    name,
                    singer,
                    source: Source::Kuwo,
                    album_name,
                    album_id,
                    interval: Some(interval),
                    pic_url: None,
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
            "Kuwo native URL resolution requires JS source bridge".to_string(),
        ))
    }

    async fn get_lyric(&self, song_id: &str) -> Result<Option<LyricInfo>, SourceError> {
        let url = format!(
            "http://mobile.kuwo.cn/mpage/html5/songinfoandlrc?mid={}&flag=0",
            song_id
        );
        let client = reqwest::Client::new();
        let resp = client
            .get(&url)
            .send()
            .await
            .map_err(|e| SourceError::HttpError(e.to_string()))?;

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| SourceError::HttpError(e.to_string()))?;

        let mut lrc_text = String::new();
        if let Some(lrc_list) = body["lrclist"].as_array() {
            for item in lrc_list {
                let time = item["time"].as_str().unwrap_or("00:00");
                let line_text = item["lineinfo"].as_str().unwrap_or("");
                lrc_text.push_str(&format!("[{}]{}\n", time, line_text));
            }
        }

        if !lrc_text.is_empty() {
            Ok(Some(LyricInfo {
                lyric: lrc_text,
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
            "http://artistpic.kuwo.cn/pic.web?type=ect_music&rid={}&size=500",
            song_id
        );
        let client = reqwest::Client::new();
        let resp = client
            .get(&url)
            .send()
            .await
            .map_err(|e| SourceError::HttpError(e.to_string()))?;

        let text = resp
            .text()
            .await
            .map_err(|e| SourceError::HttpError(e.to_string()))?;

        if text.starts_with("http") {
            Ok(Some(text.trim().to_string()))
        } else {
            Ok(None)
        }
    }
}

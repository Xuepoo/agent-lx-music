use async_trait::async_trait;
use lux_core::error::SourceError;
use lux_core::traits::{LyricInfo, MusicSource};
use lux_core::types::{MusicInfo, Quality, SearchResult, Source};
use serde_json::Value;

#[derive(Default)]
pub struct KugouSource;

/// Remove search highlight HTML tags (e.g. `<em>`, `</em>`) and common HTML entities.
fn strip_tags(input: &str) -> String {
    input
        .replace("<em>", "")
        .replace("</em>", "")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
}

/// Parse Kugou song search JSON response into a list of `MusicInfo` and the total count.
fn parse_kugou_search_response(body: &Value) -> (Vec<MusicInfo>, usize) {
    let mut list = Vec::new();
    let total = body["data"]["total"].as_u64().unwrap_or(0) as usize;

    if let Some(items) = body["data"]["lists"].as_array() {
        for item in items {
            let file_hash = item["FileHash"].as_str().unwrap_or("").to_string();
            // Prefer FileHash; fallback to Audioid or ID if FileHash is empty
            let songmid = if !file_hash.is_empty() {
                file_hash.clone()
            } else {
                item["Audioid"]
                    .as_i64()
                    .map(|id| id.to_string())
                    .or_else(|| item["ID"].as_str().map(|id| id.to_string()))
                    .unwrap_or_default()
            };

            if songmid.is_empty() {
                continue;
            }

            let name = item["SongName"]
                .as_str()
                .map(strip_tags)
                .unwrap_or_else(|| "Unknown".to_string());

            let singer = item["SingerName"]
                .as_str()
                .map(strip_tags)
                .unwrap_or_else(|| "Unknown".to_string());

            let album_name = item["AlbumName"]
                .as_str()
                .map(strip_tags)
                .filter(|s| !s.is_empty());

            let album_id = item["AlbumID"]
                .as_str()
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty() && s != "0");

            let duration_secs = item["Duration"].as_i64().unwrap_or(0);
            let interval = format!("{:02}:{:02}", duration_secs / 60, duration_secs % 60);

            let pic_url = item["Image"]
                .as_str()
                .or_else(|| item["trans_param"]["union_cover"].as_str())
                .map(|img| img.replace("{size}", "400"))
                .filter(|s| !s.is_empty());

            let hash = if !file_hash.is_empty() {
                Some(file_hash)
            } else {
                None
            };

            list.push(MusicInfo {
                songmid,
                name,
                singer,
                source: Source::Kugou,
                album_name,
                album_id,
                interval: Some(interval),
                pic_url,
                hash,
                extra: None,
            });
        }
    }

    (list, total)
}

#[async_trait]
impl MusicSource for KugouSource {
    fn platform(&self) -> Source {
        Source::Kugou
    }

    fn supported_qualities(&self) -> &[Quality] {
        &[
            Quality::Q128k,
            Quality::Q320k,
            Quality::Flac,
            Quality::Flac24bit,
        ]
    }

    async fn search(
        &self,
        keyword: &str,
        page: usize,
        limit: usize,
    ) -> Result<SearchResult, SourceError> {
        let page_num = page.max(1);
        let url = "http://songsearch.kugou.com/song_search_v2";

        let client = crate::http::client();
        let resp = client
            .get(url)
            .query(&[
                ("keyword", keyword),
                ("page", &page_num.to_string()),
                ("pagesize", &limit.to_string()),
                ("platform", "WebFilter"),
                ("filter", "2"),
                ("iscorrection", "1"),
            ])
            .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64)")
            .send()
            .await
            .map_err(|e| SourceError::HttpError(e.to_string()))?;

        let body: Value = resp
            .json()
            .await
            .map_err(|e| SourceError::HttpError(e.to_string()))?;

        let (list, total) = parse_kugou_search_response(&body);

        Ok(SearchResult {
            list,
            total,
            page: page_num,
            limit,
        })
    }

    async fn get_url(&self, _song_id: &str, _quality: Quality) -> Result<String, SourceError> {
        Err(SourceError::PlatformError(
            "Kugou native get_url not implemented yet".to_string(),
        ))
    }

    async fn get_lyric(&self, _song_id: &str) -> Result<Option<LyricInfo>, SourceError> {
        Err(SourceError::PlatformError(
            "Kugou native get_lyric not implemented yet".to_string(),
        ))
    }

    async fn get_pic(&self, _song_id: &str) -> Result<Option<String>, SourceError> {
        Err(SourceError::PlatformError(
            "Kugou native get_pic not implemented yet".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_tags() {
        assert_eq!(
            strip_tags("<em>Talking</em> to the <em>Moon</em>"),
            "Talking to the Moon"
        );
        assert_eq!(strip_tags("Bruno&nbsp;Mars"), "Bruno Mars");
        assert_eq!(strip_tags("Tom&amp;Jerry"), "Tom&Jerry");
        assert_eq!(strip_tags("&lt;Artist&gt;"), "<Artist>");
    }

    #[test]
    fn test_parse_kugou_search_response() {
        let json_data = serde_json::json!({
            "status": 1,
            "error_code": 0,
            "data": {
                "total": 42,
                "lists": [
                    {
                        "FileHash": "B732EE6AA70BA985627E0607FC08773A",
                        "SongName": "<em>Talking</em> to the Moon",
                        "SingerName": "<em>Bruno</em> Mars",
                        "AlbumName": "Doo-Wops &amp; Hooligans",
                        "AlbumID": "8175984",
                        "Duration": 217
                    },
                    {
                        "FileHash": "",
                        "Audioid": 123456,
                        "SongName": "Test Song",
                        "SingerName": "Test Singer",
                        "AlbumName": "",
                        "AlbumID": "0",
                        "Duration": 65
                    }
                ]
            }
        });

        let (list, total) = parse_kugou_search_response(&json_data);
        assert_eq!(total, 42);
        assert_eq!(list.len(), 2);

        let s1 = &list[0];
        assert_eq!(s1.songmid, "B732EE6AA70BA985627E0607FC08773A");
        assert_eq!(s1.name, "Talking to the Moon");
        assert_eq!(s1.singer, "Bruno Mars");
        assert_eq!(s1.album_name.as_deref(), Some("Doo-Wops & Hooligans"));
        assert_eq!(s1.album_id.as_deref(), Some("8175984"));
        assert_eq!(s1.interval.as_deref(), Some("03:37"));
        assert_eq!(s1.source, Source::Kugou);

        let s2 = &list[1];
        assert_eq!(s2.songmid, "123456");
        assert_eq!(s2.name, "Test Song");
        assert_eq!(s2.singer, "Test Singer");
        assert_eq!(s2.album_name, None);
        assert_eq!(s2.album_id, None);
        assert_eq!(s2.interval.as_deref(), Some("01:05"));
    }

    #[test]
    fn test_platform_and_qualities() {
        let source = KugouSource;
        assert_eq!(source.platform(), Source::Kugou);
        assert!(source.supported_qualities().contains(&Quality::Flac));
        assert!(source.supported_qualities().contains(&Quality::Flac24bit));
    }
}

#![allow(clippy::collapsible_if, clippy::collapsible_else_if)]
use crate::library::db::{SearchCacheEntry, insert_search_cache};
use anyhow::Result;
use lux_core::types::{MusicInfo, Quality, Source};
use md5::Digest;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportedTrack {
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub song_id: Option<String>,
    pub source: Option<String>,
}

// ==========================================
// 1. Pluggable Format Parsers
// ==========================================

pub fn parse_m3u(content: &str) -> Vec<ImportedTrack> {
    let mut tracks = Vec::new();
    let lines: Vec<&str> = content.lines().map(|l| l.trim()).collect();

    for line in lines {
        if line.starts_with("#EXTINF:") {
            // e.g., #EXTINF:269,周杰伦 - 晴天
            // or #EXTINF:-1,Artist - Title
            if let Some(comma_idx) = line.find(',') {
                let metadata = &line[comma_idx + 1..];
                if let Some(track) = parse_single_line_txt(metadata) {
                    tracks.push(track);
                }
            }
        }
    }
    tracks
}

pub fn parse_csv(content: &str) -> Vec<ImportedTrack> {
    let mut tracks = Vec::new();
    let lines: Vec<&str> = content.lines().map(|l| l.trim()).collect();
    if lines.is_empty() {
        return tracks;
    }

    // Parse header to find relevant columns
    let headers: Vec<String> = lines[0]
        .split(',')
        .map(|s| s.trim().to_lowercase())
        .collect();

    let mut title_idx = None;
    let mut artist_idx = None;
    let mut album_idx = None;

    for (idx, header) in headers.iter().enumerate() {
        if title_idx.is_none()
            && (header.contains("title")
                || header.contains("name")
                || header.contains("track")
                || header.contains("歌名"))
        {
            title_idx = Some(idx);
        }
        if artist_idx.is_none()
            && (header.contains("artist")
                || header.contains("singer")
                || header.contains("author")
                || header.contains("歌手"))
        {
            artist_idx = Some(idx);
        }
        if album_idx.is_none() && (header.contains("album") || header.contains("专辑")) {
            album_idx = Some(idx);
        }
    }

    let t_idx = match title_idx {
        Some(i) => i,
        None => return tracks, // Title column is mandatory
    };
    let a_idx = artist_idx.unwrap_or(t_idx); // Fallback to title column if no artist column is found

    for line in lines.iter().skip(1) {
        if line.is_empty() {
            continue;
        }

        // Simple CSV splitter handling quoted values minimally
        let mut columns = Vec::new();
        let mut in_quotes = false;
        let mut current_col = String::new();

        for ch in line.chars() {
            match ch {
                '"' => in_quotes = !in_quotes,
                ',' if !in_quotes => {
                    columns.push(current_col.trim().to_string());
                    current_col.clear();
                }
                _ => current_col.push(ch),
            }
        }
        columns.push(current_col.trim().to_string());

        if columns.len() > t_idx && columns.len() > a_idx {
            let title = columns[t_idx].clone();
            let artist = columns[a_idx].clone();
            if title.is_empty() {
                continue;
            }
            let album = album_idx
                .and_then(|idx| columns.get(idx).cloned())
                .filter(|s| !s.is_empty());

            tracks.push(ImportedTrack {
                title,
                artist,
                album,
                song_id: None,
                source: None,
            });
        }
    }
    tracks
}

pub fn parse_txt(content: &str) -> Vec<ImportedTrack> {
    let mut tracks = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(track) = parse_single_line_txt(trimmed) {
            tracks.push(track);
        }
    }
    tracks
}

fn parse_single_line_txt(line: &str) -> Option<ImportedTrack> {
    // Delimiters: " - ", " — ", " -", "- "
    let delimiters = vec![" — ", " - ", " -", "- "];
    for delim in delimiters {
        if let Some(idx) = line.find(delim) {
            let left = line[..idx].trim().to_string();
            let right = line[idx + delim.len()..].trim().to_string();
            if !left.is_empty() && !right.is_empty() {
                return Some(ImportedTrack {
                    title: right,
                    artist: left,
                    album: None,
                    song_id: None,
                    source: None,
                });
            }
        }
    }

    // Fallback: entire line as title, artist unknown
    if !line.is_empty() {
        Some(ImportedTrack {
            title: line.to_string(),
            artist: "Unknown".to_string(),
            album: None,
            song_id: None,
            source: None,
        })
    } else {
        None
    }
}

pub fn parse_universal_playlist(content: &str) -> Vec<ImportedTrack> {
    let trimmed = content.trim();
    if trimmed.starts_with('[') || trimmed.starts_with('{') {
        // Try parsing JSON list
        if let Ok(tracks) = serde_json::from_str::<Vec<ImportedTrack>>(trimmed) {
            return tracks;
        }
        if let Ok(entries) = serde_json::from_str::<Vec<SearchCacheEntry>>(trimmed) {
            return entries
                .into_iter()
                .map(|e| ImportedTrack {
                    title: e.name,
                    artist: e.singer,
                    album: e.album_name,
                    song_id: Some(e.song_id),
                    source: Some(e.source),
                })
                .collect();
        }
    }
    if trimmed.contains("#EXTM3U") {
        return parse_m3u(trimmed);
    }
    if trimmed.contains(',')
        && trimmed
            .lines()
            .next()
            .map(|l| l.contains("name") || l.contains("title") || l.contains("歌名"))
            .unwrap_or(false)
    {
        return parse_csv(trimmed);
    }
    parse_txt(trimmed)
}

// ==========================================
// 2. Agent Fuzzy Match Selector
// ==========================================

pub async fn resolve_imported_track(
    track: &ImportedTrack,
    preferred_quality: Option<Quality>,
) -> Result<Option<SearchCacheEntry>> {
    // If the track already possesses direct identifiers, bypass matching
    if let (Some(sid), Some(src)) = (&track.song_id, &track.source) {
        let hash_input = format!("{}-{}", src, sid);
        let digest = md5::Md5::digest(hash_input.as_bytes());
        let cli_id = format!("{:x}", digest)[..8].to_string();

        let entry = SearchCacheEntry {
            cli_id,
            song_id: sid.clone(),
            name: track.title.clone(),
            singer: track.artist.clone(),
            source: src.clone(),
            interval: None,
            album_name: track.album.clone(),
            album_id: None,
            pic_url: None,
            songmid: Some(sid.clone()),
            hash: None,
            extra: None,
        };
        let _ = insert_search_cache(&entry);
        return Ok(Some(entry));
    }

    // Trigger cross-platform search pipeline
    let query = format!("{} {}", track.artist, track.title);
    let config = lux_core::config::Config::load().unwrap_or_default();

    // Gather all configured sources
    let sources_to_query = vec![
        Source::NetEase,
        Source::Kuwo,
        Source::QQ,
        Source::Migu,
        Source::Kugou,
    ];

    let mut tasks = Vec::new();
    for src in sources_to_query {
        let query_clone = query.clone();
        let task = tokio::spawn(async move {
            let _ = &src;
            #[cfg(feature = "lux-native")]
            {
                if let Some(native_src) = lux_native::get_native_source(&src) {
                    return native_src.search(&query_clone, 1, 10).await.ok();
                }
            }
            let _ = query_clone;
            Option::<lux_core::types::SearchResult>::None
        });
        tasks.push(task);
    }

    let mut consolidated_candidates: Vec<MusicInfo> = Vec::new();
    let mut seen = HashSet::new();

    for task in tasks {
        if let Ok(Some(res)) = task.await {
            for song in res.list {
                let unique_key = format!("{}-{}", song.source, song.songmid);
                if !seen.contains(&unique_key) {
                    seen.insert(unique_key.clone());
                    consolidated_candidates.push(song);
                }
            }
        }
    }

    if consolidated_candidates.is_empty() {
        return Ok(None);
    }

    // Run the multi-dimensional scoring evaluator
    let mut scored_candidates: Vec<(f64, MusicInfo)> = consolidated_candidates
        .into_iter()
        .map(|candidate| {
            let score = calculate_agent_score(
                &candidate,
                track,
                &config.source.platform_priority,
                preferred_quality,
            );
            (score, candidate)
        })
        .collect();

    // Sort by descending scores
    scored_candidates.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    // Platform fallback & validation sweep loop
    let mgr = crate::source::SourceManager::new();
    for (score, candidate) in scored_candidates {
        if score < 30.0 {
            // Drop candidates with extremely low affinity correlation coefficients
            continue;
        }

        // Test URL resolution to guarantee streaming eligibility (Source Switching Fallback)
        let resolved_quality = preferred_quality.unwrap_or(config.source.default_quality);
        if let Ok(url) = mgr.resolve_url(
            candidate.source.as_str(),
            &candidate.songmid,
            resolved_quality,
        ) {
            if !url.is_empty() {
                // Resolved successfully! Construct cache entry and commit
                let hash_input = format!("{}-{}", candidate.source.as_str(), candidate.songmid);
                let digest = md5::Md5::digest(hash_input.as_bytes());
                let cli_id = format!("{:x}", digest)[..8].to_string();

                let entry = SearchCacheEntry {
                    cli_id,
                    song_id: candidate.songmid.clone(),
                    name: candidate.name.clone(),
                    singer: candidate.singer.clone(),
                    source: candidate.source.as_str().to_string(),
                    interval: candidate.interval.clone(),
                    album_name: candidate.album_name.clone(),
                    album_id: candidate.album_id.clone(),
                    pic_url: candidate.pic_url.clone(),
                    songmid: Some(candidate.songmid.clone()),
                    hash: candidate.hash.clone(),
                    extra: Some(url),
                };
                let _ = insert_search_cache(&entry);
                return Ok(Some(entry));
            }
        }
    }

    Ok(None)
}

fn calculate_agent_score(
    candidate: &MusicInfo,
    target: &ImportedTrack,
    platform_priority: &[String],
    preferred_quality: Option<Quality>,
) -> f64 {
    let _ = preferred_quality;
    let mut score = 0.0;

    let cand_singer_lower = candidate.singer.to_lowercase();
    let target_artist_lower = target.artist.to_lowercase();
    let cand_title_lower = candidate.name.to_lowercase();
    let target_title_lower = target.title.to_lowercase();

    // 1. Artist Match (Weight: 45.0)
    if cand_singer_lower == target_artist_lower
        || (cand_title_lower.contains(&target_artist_lower)
            && cand_singer_lower == target_title_lower)
    {
        score += 45.0;
    } else if cand_singer_lower.contains(&target_artist_lower)
        || target_artist_lower.contains(&cand_singer_lower)
    {
        score += 25.0;
    }

    // 2. Title Match (Weight: 30.0)
    if cand_title_lower == target_title_lower
        || (cand_singer_lower == target_title_lower && cand_title_lower == target_artist_lower)
    {
        score += 30.0;
    } else if cand_title_lower.contains(&target_title_lower)
        || target_title_lower.contains(&cand_title_lower)
    {
        score += 15.0;
    }

    // 3. Remix / Acoustic / Live Flags Guard (Weight: 30.0 penalty for mismatch)
    let version_flags = vec!["remix", "live", "acoustic", "instrumental", "伴奏", "现场"];
    for flag in version_flags {
        let target_has = target_title_lower.contains(flag);
        let cand_has = cand_title_lower.contains(flag);
        if target_has != cand_has {
            score -= 30.0;
        }
    }

    // 4. Album Affinity (Weight: 10.0)
    if let (Some(t_album), Some(c_album)) = (&target.album, &candidate.album_name) {
        let t_alb_lower = t_album.to_lowercase();
        let c_alb_lower = c_album.to_lowercase();
        if c_alb_lower == t_alb_lower {
            score += 10.0;
        } else if c_alb_lower.contains(&t_alb_lower) || t_alb_lower.contains(&c_alb_lower) {
            score += 5.0;
        }
    }

    // 5. Quality Premium Weight (Weight: 15.0)
    #[cfg(feature = "lux-native")]
    {
        if let Some(pq) = preferred_quality {
            if let Some(native_src) = lux_native::get_native_source(&candidate.source) {
                let supported = native_src.supported_qualities();
                if supported.contains(&pq) {
                    score += 15.0;
                } else if pq == Quality::Flac24bit && supported.contains(&Quality::Flac) {
                    score += 8.0;
                }
            }
        }
    }

    // 6. Configurable Platform Priority Weighting (Weight: 10.0)
    let plat_str = candidate.source.as_str();
    if let Some(pos) = platform_priority.iter().position(|p| p == plat_str) {
        let n = platform_priority.len();
        score += (n - pos) as f64 * 2.0;
    }

    score
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_playlist_parsers() {
        // 1. M3U Parser
        let m3u_data = "#EXTM3U\n#EXTINF:260,ArtistA - SongA\n/local/path.mp3";
        let tracks = parse_m3u(m3u_data);
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].title, "SongA");
        assert_eq!(tracks[0].artist, "ArtistA");

        // 2. CSV Parser
        let csv_data = "Track Name,Artist Name(s),Album\nSongB,ArtistB,AlbumB";
        let tracks = parse_csv(csv_data);
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].title, "SongB");
        assert_eq!(tracks[0].artist, "ArtistB");

        // 3. Plain Text Parser
        let txt_data = "ArtistC - SongC\n# Comment line\nSongD - ArtistD";
        let tracks = parse_txt(txt_data);
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].title, "SongC");
        assert_eq!(tracks[0].artist, "ArtistC");
    }

    #[test]
    fn test_scoring_weights() {
        let candidate = MusicInfo {
            songmid: "123".to_string(),
            name: "晴天".to_string(),
            singer: "周杰伦".to_string(),
            source: Source::NetEase,
            album_name: Some("叶惠美".to_string()),
            album_id: None,
            interval: None,
            pic_url: None,
            hash: None,
            extra: None,
        };

        let target = ImportedTrack {
            title: "晴天".to_string(),
            artist: "周杰伦".to_string(),
            album: Some("叶惠美".to_string()),
            song_id: None,
            source: None,
        };

        let score = calculate_agent_score(
            &candidate,
            &target,
            &["wy".to_string(), "kw".to_string()],
            None,
        );
        assert!(score > 80.0);
    }
}

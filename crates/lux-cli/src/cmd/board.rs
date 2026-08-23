use crate::library::db::SearchCacheEntry;
use crate::player::MpvClient;
use crate::source::SourceManager;
use anyhow::{Result, anyhow};
use colored::Colorize;
use serde::Deserialize;

#[derive(Deserialize, Debug, Clone)]
pub struct PlaylistDetailResponse {
    pub playlist: PlaylistDetail,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug, Clone)]
pub struct PlaylistDetail {
    pub id: i64,
    pub name: String,
    pub tracks: Vec<TrackDetail>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct TrackDetail {
    pub id: i64,
    pub name: String,
    pub ar: Vec<ArtistDetail>,
    pub al: AlbumDetail,
}

#[derive(Deserialize, Debug, Clone)]
pub struct ArtistDetail {
    pub name: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct AlbumDetail {
    pub name: String,
    #[serde(rename = "picUrl")]
    pub pic_url: Option<String>,
}

pub async fn fetch_playlist_tracks(playlist_id: &str) -> Result<Vec<TrackDetail>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?;
    let url = format!(
        "https://music.163.com/api/v6/playlist/detail?id={}",
        playlist_id
    );
    let res = client
        .get(&url)
        .header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
        )
        .send()
        .await?
        .json::<PlaylistDetailResponse>()
        .await?;
    Ok(res.playlist.tracks)
}

pub async fn run_board(
    _source: Option<String>,
    id: Option<String>,
    play: bool,
    json: bool,
) -> Result<()> {
    if id.is_none() {
        if json {
            println!(
                "{}",
                serde_json::json!({
                    "status": "success",
                    "data": [
                        { "id": "wy-hot", "name": "热歌榜", "description": "网易云音乐热歌榜" },
                        { "id": "wy-new", "name": "新歌榜", "description": "网易云音乐新歌榜" },
                        { "id": "wy-up", "name": "飙升榜", "description": "网易云音乐飙升榜" },
                        { "id": "wy-origin", "name": "原创榜", "description": "网易云音乐原创榜" }
                    ]
                })
            );
        } else {
            println!("Available Charts (Netease Music):");
            println!("{:<12} {:<10} {:<30}", "ID", "Name", "Description");
            println!(
                "{:<12} {:<10} {:<30}",
                "wy-hot", "热歌榜", "网易云音乐热歌榜"
            );
            println!(
                "{:<12} {:<10} {:<30}",
                "wy-new", "新歌榜", "网易云音乐新歌榜"
            );
            println!(
                "{:<12} {:<10} {:<30}",
                "wy-up", "飙升榜", "网易云音乐飙升榜"
            );
            println!(
                "{:<12} {:<10} {:<30}",
                "wy-origin", "原创榜", "网易云音乐原创榜"
            );
        }
        return Ok(());
    }

    let raw_id = id.unwrap();
    let target_id = match raw_id.as_str() {
        "wy-hot" => "3778678",
        "wy-new" => "3779629",
        "wy-up" => "19723756",
        "wy-origin" => "2884035",
        other => other,
    };

    let chart_name = match raw_id.as_str() {
        "wy-hot" => "热歌榜",
        "wy-new" => "新歌榜",
        "wy-up" => "飙升榜",
        "wy-origin" => "原创榜",
        _ => "榜单",
    };

    let tracks = fetch_playlist_tracks(target_id).await?;
    if tracks.is_empty() {
        return Err(anyhow!("No tracks found in chart: {}", raw_id));
    }

    let tracks_limit: Vec<TrackDetail> = tracks.into_iter().take(30).collect();

    // DEF-011/#147: ensure the schema exists so cached rows are actually
    // persisted instead of failing silently on a fresh database.
    crate::library::db::init_db()?;

    let mut cache_entries = Vec::new();
    for track in &tracks_limit {
        let cli_id = format!("wy:{}", track.id);
        let singer = track
            .ar
            .iter()
            .map(|a| a.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let entry = SearchCacheEntry {
            cli_id: cli_id.clone(),
            song_id: track.id.to_string(),
            name: track.name.clone(),
            singer: singer.clone(),
            source: "wy".to_string(),
            interval: None,
            album_name: Some(track.al.name.clone()),
            album_id: None,
            pic_url: track.al.pic_url.clone(),
            songmid: None,
            hash: None,
            extra: None,
        };
        let _ = crate::library::db::insert_search_cache(&entry);
        cache_entries.push(entry);
    }

    if play {
        play_tracks(&cache_entries, chart_name, json).await?;
    } else {
        if json {
            println!("{}", serde_json::to_string(&cache_entries)?);
        } else {
            println!("Chart: {} ({} tracks)", chart_name, cache_entries.len());
            println!(
                "{:<10} {:<30} {:<20} {:<20}",
                "CLI ID", "Song Name", "Artist", "Album"
            );
            for e in &cache_entries {
                let name_trimmed = if e.name.chars().count() > 28 {
                    e.name.chars().take(25).collect::<String>() + "..."
                } else {
                    e.name.clone()
                };
                let singer_trimmed = if e.singer.chars().count() > 18 {
                    e.singer.chars().take(15).collect::<String>() + "..."
                } else {
                    e.singer.clone()
                };
                let album_trimmed = if e.album_name.as_deref().unwrap_or("").chars().count() > 18 {
                    e.album_name
                        .as_deref()
                        .unwrap_or("")
                        .chars()
                        .take(15)
                        .collect::<String>()
                        + "..."
                } else {
                    e.album_name.clone().unwrap_or_default()
                };
                println!(
                    "{:<10} {:<30} {:<20} {:<20}",
                    e.cli_id, name_trimmed, singer_trimmed, album_trimmed
                );
            }
        }
    }
    Ok(())
}

pub async fn play_tracks(songs: &[SearchCacheEntry], name: &str, json: bool) -> Result<()> {
    if songs.is_empty() {
        return Err(anyhow!("No songs to play"));
    }
    let config = lux_core::config::Config::load().unwrap_or_default();
    let client = MpvClient::new();
    let mgr = SourceManager::new();
    let quality = config.source.default_quality;

    if !json {
        println!(
            "{} Loading tracks from '{}' ({} songs)...",
            "⚡".yellow().bold(),
            name.cyan(),
            songs.len()
        );
    }

    let _ = crate::player::ipc::send_mpv_command(
        &client.socket_path,
        vec![serde_json::json!("playlist-clear")],
    );

    let first_song = &songs[0];
    if !json {
        println!(
            "{} Resolving playable URL for '{}'...",
            "⚡".yellow().bold(),
            first_song.name.cyan()
        );
    }
    let first_url = mgr.resolve_url(&first_song.source, &first_song.song_id, quality)?;
    client.play_file_or_url(&first_url)?;

    let paths = lux_core::config::resolve_paths();
    let current_json_path = paths.cache_dir.join("current.json");
    let _ = std::fs::create_dir_all(&paths.cache_dir);
    let state_json = serde_json::json!({
        "song": first_song,
        "last_position": 0.0,
        "volume": config.player.default_volume
    });
    let _ = std::fs::write(&current_json_path, state_json.to_string());

    let _ = crate::library::db::add_to_history(first_song, None);

    let mut added_songs = vec![first_song.clone()];

    for song in songs.iter().skip(1) {
        if let Ok(url) = mgr.resolve_url(&song.source, &song.song_id, quality) {
            let _ = client.append_file_or_url(&url);
            added_songs.push(song.clone());
        }
    }

    let updated_queue = crate::cmd::queue::PlayQueue {
        songs: added_songs,
        current_index: Some(0),
    };
    crate::cmd::queue::save_queue(&updated_queue)?;

    if json {
        println!(
            "{}",
            serde_json::json!({
                "status": "playing",
                "source": name,
                "count": updated_queue.songs.len(),
                "current": first_song.name
            })
        );
    } else {
        println!(
            "{} Started playing: {} (loaded {} songs)",
            "▶".green().bold(),
            name.cyan(),
            updated_queue.songs.len()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_playlist_detail() {
        let sample = r#"{
            "playlist": {
                "id": 123456,
                "name": "Hot Chart",
                "tracks": [
                    {
                        "id": 98765,
                        "name": "Song Name A",
                        "ar": [
                            { "name": "Artist A" }
                        ],
                        "al": {
                            "name": "Album A",
                            "picUrl": "https://example.com/pic.jpg"
                        }
                    }
                ]
            }
        }"#;
        let parsed: PlaylistDetailResponse = serde_json::from_str(sample).unwrap();
        assert_eq!(parsed.playlist.id, 123456);
        assert_eq!(parsed.playlist.name, "Hot Chart");
        assert_eq!(parsed.playlist.tracks[0].name, "Song Name A");
        assert_eq!(parsed.playlist.tracks[0].ar[0].name, "Artist A");
        assert_eq!(parsed.playlist.tracks[0].al.name, "Album A");
        assert_eq!(
            parsed.playlist.tracks[0].al.pic_url.as_deref(),
            Some("https://example.com/pic.jpg")
        );
    }
}

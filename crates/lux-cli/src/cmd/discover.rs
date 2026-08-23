use crate::cli::DiscoverAction;
use crate::cmd::board::{fetch_playlist_tracks, play_tracks};
use crate::library::db::SearchCacheEntry;
use anyhow::{Result, anyhow};
use serde::Deserialize;

#[derive(Deserialize, Debug, Clone)]
pub struct PersonalizedResponse {
    pub result: Vec<PersonalizedPlaylist>,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug, Clone)]
pub struct PersonalizedPlaylist {
    pub id: i64,
    pub name: String,
    #[serde(rename = "playCount")]
    pub play_count: f64,
    pub copywriter: Option<String>,
}

pub async fn fetch_personalized_playlists() -> Result<Vec<PersonalizedPlaylist>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?;
    let url = "https://music.163.com/api/personalized/playlist?limit=10";
    let res = client
        .get(url)
        .header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
        )
        .send()
        .await?
        .json::<PersonalizedResponse>()
        .await?;
    Ok(res.result)
}

pub async fn run_discover(
    _source: Option<String>,
    _tag: Option<String>,
    action: Option<DiscoverAction>,
    json: bool,
) -> Result<()> {
    if let Some(act) = action {
        // DEF-011/#147: ensure the schema exists so cached rows are actually
        // persisted instead of failing silently on a fresh database.
        crate::library::db::init_db()?;
        match act {
            DiscoverAction::Show { playlist_id } => {
                let tracks = fetch_playlist_tracks(&playlist_id).await?;
                if tracks.is_empty() {
                    return Err(anyhow!("No tracks found in playlist: {}", playlist_id));
                }

                let mut cache_entries = Vec::new();
                for track in tracks.iter().take(30) {
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

                if json {
                    println!("{}", serde_json::to_string(&cache_entries)?);
                } else {
                    println!("Playlist: {} ({} tracks)", playlist_id, cache_entries.len());
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
                        let album_trimmed =
                            if e.album_name.as_deref().unwrap_or("").chars().count() > 18 {
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
            DiscoverAction::Play { playlist_id } => {
                let tracks = fetch_playlist_tracks(&playlist_id).await?;
                if tracks.is_empty() {
                    return Err(anyhow!("No tracks found in playlist: {}", playlist_id));
                }

                let mut cache_entries = Vec::new();
                for track in tracks.iter().take(30) {
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

                play_tracks(&cache_entries, &format!("Playlist: {}", playlist_id), json).await?;
            }
        }
        return Ok(());
    }

    let playlists = fetch_personalized_playlists().await?;
    if playlists.is_empty() {
        return Err(anyhow!("No personalized recommendations found"));
    }

    if json {
        let data: Vec<serde_json::Value> = playlists
            .iter()
            .map(|p| {
                serde_json::json!({
                    "id": p.id.to_string(),
                    "name": p.name,
                    "play_count": format_play_count(p.play_count)
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::json!({
                "status": "success",
                "data": data
            })
        );
    } else {
        println!("Recommended Playlists:");
        println!(
            "{:<16} {:<45} {:<10}",
            "Playlist ID", "Playlist Name", "Play Count"
        );
        for p in &playlists {
            let count_str = format_play_count(p.play_count);
            let name_trimmed = if p.name.chars().count() > 42 {
                p.name.chars().take(39).collect::<String>() + "..."
            } else {
                p.name.clone()
            };
            println!("{:<16} {:<45} {:<10}", p.id, name_trimmed, count_str);
        }
    }

    Ok(())
}

fn format_play_count(count: f64) -> String {
    if count >= 100_000.0 {
        format!("{:.1}W", count / 10000.0)
    } else {
        count.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_personalized() {
        let sample = r#"{
            "code": 200,
            "result": [
                {
                    "id": 123456,
                    "name": "Relaxing Afternoon",
                    "playCount": 120500.0,
                    "copywriter": "Perfect afternoon vibes"
                }
            ]
        }"#;
        let parsed: PersonalizedResponse = serde_json::from_str(sample).unwrap();
        assert_eq!(parsed.result[0].id, 123456);
        assert_eq!(parsed.result[0].name, "Relaxing Afternoon");
        assert_eq!(parsed.result[0].play_count, 120500.0);
        assert_eq!(
            parsed.result[0].copywriter.as_deref(),
            Some("Perfect afternoon vibes")
        );
    }
}

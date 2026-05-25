#![allow(clippy::collapsible_if, clippy::collapsible_else_if)]
use crate::library::db::{SearchCacheEntry, add_to_history, get_song_by_cli_id};
use crate::player::MpvClient;
use crate::source::SourceManager;
use anyhow::{Result, anyhow};
use colored::Colorize;
use lux_core::types::Quality;
use std::fs;
use std::path::Path;
use std::time::Duration;

pub async fn run(
    id_or_urls: Vec<String>,
    quality_str: Option<String>,
    _from_playlist: Option<String>,
    _shuffle: bool,
    json: bool,
) -> Result<()> {
    if id_or_urls.is_empty() {
        let config = lux_core::config::Config::load().unwrap_or_default();
        if config.player.auto_resume {
            let paths = lux_core::config::resolve_paths();
            let current_json_path = paths.cache_dir.join("current.json");
            if current_json_path.exists() {
                if let Ok(content) = fs::read_to_string(&current_json_path) {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                        if let Some(song_val) = val.get("song") {
                            if let Ok(song) =
                                serde_json::from_value::<SearchCacheEntry>(song_val.clone())
                            {
                                let last_pos = val
                                    .get("last_position")
                                    .and_then(|v| v.as_f64())
                                    .unwrap_or(0.0);
                                let volume = val
                                    .get("volume")
                                    .and_then(|v| v.as_u64())
                                    .map(|v| v as u8)
                                    .unwrap_or(config.player.default_volume);

                                let client = MpvClient::new();
                                let resolved_url = if song.source == "local" {
                                    song.extra.clone().unwrap_or_default()
                                } else {
                                    let mgr = SourceManager::new();
                                    mgr.resolve_url(
                                        &song.source,
                                        &song.song_id,
                                        config.source.default_quality,
                                    )?
                                };

                                if !resolved_url.is_empty() {
                                    if !json {
                                        println!(
                                            "{} Resuming playback: {} — {} (at {:.1}s)...",
                                            "▶".green().bold(),
                                            song.name.bold(),
                                            song.singer.cyan(),
                                            last_pos
                                        );
                                    }
                                    client.play_file_or_url(&resolved_url)?;
                                    client.set_volume(volume)?;
                                    if last_pos > 0.0 {
                                        std::thread::sleep(Duration::from_millis(300));
                                        let _ = client.seek(&last_pos.to_string());
                                    }
                                    return Ok(());
                                }
                            }
                        }
                    }
                }
            }
        }
        return Err(anyhow!(
            "No active song found to resume, or auto_resume is disabled. Please provide a song ID."
        ));
    }

    let config = lux_core::config::Config::load().unwrap_or_default();
    let client = MpvClient::new();
    let first = &id_or_urls[0];

    // Check if it is a direct URL or local file path
    if first.starts_with("http://") || first.starts_with("https://") || Path::new(first).exists() {
        client.play_file_or_url(first)?;

        let filename = Path::new(first)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(first);

        let entry = SearchCacheEntry {
            cli_id: "direct".to_string(),
            song_id: "direct".to_string(),
            name: filename.to_string(),
            singer: "Direct Link / Local File".to_string(),
            source: "local".to_string(),
            interval: None,
            album_name: None,
            album_id: None,
            pic_url: None,
            songmid: None,
            hash: None,
            extra: None,
        };

        save_currently_playing(&entry)?;

        if json {
            println!(
                "{}",
                serde_json::json!({
                    "status": "playing",
                    "file": first
                })
            );
        } else {
            println!(
                "{} Playing direct link/file: {}",
                "▶".green().bold(),
                first.cyan()
            );
        }
        return Ok(());
    }

    // Otherwise, treat as CLI ID from the database cache
    crate::library::db::init_db()?;
    let song = get_song_by_cli_id(first)?.ok_or_else(|| {
        anyhow!(
            "Song ID '{}' not found in search cache. Run 'rlx search' first.",
            first
        )
    })?;

    // Determine quality
    let quality = if let Some(ref q) = quality_str {
        q.parse::<Quality>()
            .map_err(|e| anyhow!("Invalid quality override: {}", e))?
    } else {
        config.source.default_quality
    };

    if !json {
        println!(
            "{} Resolving playable URL for '{}'...",
            "⚡".yellow().bold(),
            song.name.cyan()
        );
    }

    // Resolve URL using SourceManager
    let mgr = SourceManager::new();
    let resolved_url = mgr.resolve_url(&song.source, &song.song_id, quality)?;

    // Play in mpv
    client.play_file_or_url(&resolved_url)?;

    // Save currently playing info
    save_currently_playing(&song)?;

    // Add to history
    let _ = add_to_history(&song, None);

    if json {
        println!(
            "{}",
            serde_json::json!({
                "status": "playing",
                "id": song.cli_id,
                "name": song.name,
                "singer": song.singer,
                "source": song.source,
                "url": resolved_url
            })
        );
    } else {
        println!(
            "\n{} Started playing: {} — {}",
            "▶".green().bold(),
            song.name.bold(),
            song.singer.cyan()
        );
        if let Some(ref album) = song.album_name {
            println!("  Album:  {}", album);
        }
        println!(
            "  Source: {} | Quality: {}",
            song.source.green(),
            quality.to_string().yellow()
        );
        println!();
    }

    Ok(())
}

fn save_currently_playing(entry: &SearchCacheEntry) -> Result<()> {
    let paths = lux_core::config::resolve_paths();
    let current_json_path = paths.cache_dir.join("current.json");
    if let Some(parent) = current_json_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let serialized = serde_json::to_string(&entry)?;
    fs::write(current_json_path, serialized)?;
    Ok(())
}

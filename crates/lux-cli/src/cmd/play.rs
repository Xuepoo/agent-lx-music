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

use rand::seq::SliceRandom;

pub async fn run(
    id_or_urls: Vec<String>,
    quality_str: Option<String>,
    from_playlist: Option<String>,
    shuffle: bool,
    json: bool,
) -> Result<()> {
    let config = lux_core::config::Config::load().unwrap_or_default();
    let client = MpvClient::new();
    let mgr = SourceManager::new();

    // Determine quality
    let quality = if let Some(ref q) = quality_str {
        q.parse::<Quality>()
            .map_err(|e| anyhow!("Invalid quality override: {}", e))?
    } else {
        config.source.default_quality
    };

    // Case A: Play from playlist
    if let Some(ref playlist_name) = from_playlist {
        crate::library::db::init_db()?;
        let mut songs = crate::library::db::get_playlist_songs(playlist_name)?;
        if songs.is_empty() {
            return Err(anyhow!(
                "Playlist '{}' is empty or does not exist",
                playlist_name
            ));
        }

        if shuffle {
            let mut rng = rand::thread_rng();
            songs.shuffle(&mut rng);
        }

        if !json {
            println!(
                "{} Loading playlist '{}' ({} songs)...",
                "⚡".yellow().bold(),
                playlist_name.cyan(),
                songs.len()
            );
        }

        // Clear playlist
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
        save_currently_playing(first_song)?;
        let _ = add_to_history(first_song, None);

        let mut added_songs = vec![first_song.clone()];

        for song in songs.iter().skip(1) {
            if let Ok(url) = mgr.resolve_url(&song.source, &song.song_id, quality) {
                client.append_file_or_url(&url)?;
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
                    "status": "playing_playlist",
                    "playlist": playlist_name,
                    "count": updated_queue.songs.len(),
                    "current": first_song.name
                })
            );
        } else {
            println!(
                "{} Started playing playlist: {} (loaded {} songs)",
                "▶".green().bold(),
                playlist_name.cyan(),
                updated_queue.songs.len()
            );
        }
        return Ok(());
    }

    // Case B: No songs provided, try to resume
    if id_or_urls.is_empty() {
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

                                let resolved_url = if song.source == "local" {
                                    song.extra.clone().unwrap_or_default()
                                } else {
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

    // Case C: Direct link or local file (First element)
    let first = &id_or_urls[0];
    let is_url = first.starts_with("http://") || first.starts_with("https://");
    let expanded_path = lux_core::config::expand_path(first);
    let is_path_like = first.starts_with('/')
        || first.starts_with('~')
        || first.contains('/')
        || first.contains('\\')
        || first.ends_with(".mp3")
        || first.ends_with(".flac")
        || first.ends_with(".m4a")
        || first.ends_with(".ogg")
        || first.ends_with(".wav");

    if is_url || expanded_path.exists() {
        let play_target = if is_url {
            first.clone()
        } else {
            expanded_path.to_string_lossy().to_string()
        };
        client.play_file_or_url(&play_target)?;

        let filename = Path::new(&play_target)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&play_target);

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
            extra: Some(play_target.clone()),
        };

        save_currently_playing(&entry)?;

        if json {
            println!(
                "{}",
                serde_json::json!({
                    "status": "playing",
                    "file": play_target
                })
            );
        } else {
            println!(
                "{} Playing direct link/file: {}",
                "▶".green().bold(),
                play_target.cyan()
            );
        }
        return Ok(());
    } else if is_path_like {
        return Err(anyhow!("Local file not found: {}", first));
    }

    // Case D: Multiple or Single CLI IDs from Database
    crate::library::db::init_db()?;
    let mut songs = Vec::new();
    for id in &id_or_urls {
        if let Some(song) = get_song_by_cli_id(id)? {
            songs.push(song);
        } else {
            return Err(anyhow!(
                "Song ID '{}' not found in search cache. Run 'alx search' first.",
                id
            ));
        }
    }

    if shuffle {
        let mut rng = rand::thread_rng();
        songs.shuffle(&mut rng);
    }

    // Clear queue
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
    save_currently_playing(first_song)?;
    let _ = add_to_history(first_song, None);

    let mut added_songs = vec![first_song.clone()];

    for song in songs.iter().skip(1) {
        if let Ok(url) = mgr.resolve_url(&song.source, &song.song_id, quality) {
            client.append_file_or_url(&url)?;
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
                "id": first_song.cli_id,
                "name": first_song.name,
                "singer": first_song.singer,
                "source": first_song.source,
                "url": first_url,
                "queue_count": updated_queue.songs.len()
            })
        );
    } else {
        println!(
            "\n{} Started playing: {} — {}",
            "▶".green().bold(),
            first_song.name.bold(),
            first_song.singer.cyan()
        );
        if let Some(ref album) = first_song.album_name {
            println!("  Album:  {}", album);
        }
        println!(
            "  Source: {} | Quality: {} | Queue: {} songs",
            first_song.source.green(),
            quality.to_string().yellow(),
            updated_queue.songs.len()
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

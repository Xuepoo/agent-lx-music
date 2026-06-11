#![allow(
    clippy::collapsible_if,
    clippy::collapsible_else_if,
    clippy::let_unit_value,
    clippy::explicit_counter_loop
)]
use crate::cli::QueueAction;
use crate::library::db::{self, SearchCacheEntry};
use crate::player::MpvClient;
use crate::source::SourceManager;
use anyhow::{Result, anyhow};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use tabled::Tabled;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PlayQueue {
    pub songs: Vec<SearchCacheEntry>,
    pub current_index: Option<usize>,
}

#[derive(Tabled)]
struct QueueTableEntry {
    #[tabled(rename = "Pos")]
    position: usize,
    #[tabled(rename = "Title")]
    title: String,
    #[tabled(rename = "Artist")]
    singer: String,
    #[tabled(rename = "Platform")]
    source: String,
    #[tabled(rename = "Status")]
    status: String,
}

pub fn load_or_init_queue() -> Result<PlayQueue> {
    let paths = lux_core::config::resolve_paths();
    let queue_json_path = paths.cache_dir.join("queue.json");
    if !queue_json_path.exists() {
        return Ok(PlayQueue {
            songs: Vec::new(),
            current_index: None,
        });
    }
    let content = fs::read_to_string(queue_json_path)?;
    let queue: PlayQueue = serde_json::from_str(&content)?;
    Ok(queue)
}

pub fn save_queue(queue: &PlayQueue) -> Result<()> {
    let paths = lux_core::config::resolve_paths();
    let queue_json_path = paths.cache_dir.join("queue.json");
    if let Some(parent) = queue_json_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let content = serde_json::to_string(queue)?;
    fs::write(queue_json_path, content)?;
    Ok(())
}

pub async fn run(action: QueueAction, json_out: bool) -> Result<()> {
    let mut queue = load_or_init_queue().unwrap_or(PlayQueue {
        songs: Vec::new(),
        current_index: None,
    });
    let client = MpvClient::new();
    let mgr = SourceManager::new();
    let config = lux_core::config::Config::load().unwrap_or_default();

    match action {
        QueueAction::Show => {
            // Update index against mpv in case of changes
            if let Ok(Some(new_idx)) = client.get_playing_index() {
                if queue.current_index != Some(new_idx) {
                    queue.current_index = Some(new_idx);
                    let _ = save_queue(&queue);
                }
            }

            if json_out {
                println!("{}", serde_json::to_string_pretty(&queue)?);
                return Ok(());
            }

            if queue.songs.is_empty() {
                println!("Queue is empty. Add songs using 'rlx queue add <ids>'");
                return Ok(());
            }

            let mut table_data = Vec::new();
            for (i, song) in queue.songs.iter().enumerate() {
                let is_playing = queue.current_index == Some(i);
                let status = if is_playing {
                    "← playing".green().bold().to_string()
                } else {
                    "".to_string()
                };

                table_data.push(QueueTableEntry {
                    position: i + 1,
                    title: if is_playing {
                        song.name.green().bold().to_string()
                    } else {
                        song.name.clone()
                    },
                    singer: song.singer.clone(),
                    source: song.source.clone(),
                    status,
                });
            }

            let table = tabled::Table::new(table_data)
                .with(tabled::settings::Style::rounded())
                .to_string();

            println!("\nQueue ({} songs):", queue.songs.len());
            println!("{}", table);
        }
        QueueAction::Add { ids } => {
            let mut added_songs = Vec::new();
            for id in ids {
                let song = db::get_song_by_cli_id(&id)?.ok_or_else(|| {
                    anyhow!("Song ID '{}' not found in search cache. Search first.", id)
                })?;

                if !json_out {
                    println!(
                        "{} Resolving playable URL for '{}'...",
                        "⚡".yellow().bold(),
                        song.name.cyan()
                    );
                }

                let url =
                    mgr.resolve_url(&song.source, &song.song_id, config.source.default_quality)?;

                // Append song to mpv
                let _ = client.append_file_or_url(&url)?;

                added_songs.push(song);
            }

            // Sync with local queue structure
            let mut songs = queue.songs;
            songs.extend(added_songs.clone());

            let mut current_idx = queue.current_index;
            if current_idx.is_none() && !songs.is_empty() {
                current_idx = Some(0);
                // Trigger play if queue was empty
                if let Some(first_song) = songs.first() {
                    let url = mgr.resolve_url(
                        &first_song.source,
                        &first_song.song_id,
                        config.source.default_quality,
                    )?;
                    let _ = client.play_file_or_url(&url)?;

                    // Save to current.json
                    let current_json_path = paths_to_current_json();
                    let new_state = serde_json::json!({
                        "song": first_song,
                        "last_position": 0.0,
                        "volume": config.player.default_volume,
                        "updated_at": chrono::Local::now().to_rfc3339()
                    });
                    let _ = fs::write(current_json_path, serde_json::to_string(&new_state)?);

                    // Add to history
                    let _ = db::add_to_history(first_song, None);
                }
            }

            let updated_queue = PlayQueue {
                songs,
                current_index: current_idx,
            };
            save_queue(&updated_queue)?;

            if json_out {
                println!(
                    "{}",
                    serde_json::json!({ "status": "added", "count": added_songs.len() })
                );
            } else {
                for s in added_songs {
                    println!("✓ Added \"{} - {}\" to queue.", s.singer, s.name);
                }
            }
        }
        QueueAction::Insert { ids } => {
            let current_idx = client.get_playing_index().unwrap_or(Some(0)).unwrap_or(0);
            let mut inserted_songs = Vec::new();
            let mut target_pos = current_idx + 1;

            for id in ids {
                let song = db::get_song_by_cli_id(&id)?
                    .ok_or_else(|| anyhow!("Song ID '{}' not found in search cache.", id))?;

                let url =
                    mgr.resolve_url(&song.source, &song.song_id, config.source.default_quality)?;

                // Append song temporarily to mpv
                let _ = client.append_file_or_url(&url)?;

                // Fetch new total count in mpv
                let conn_socket = client.socket_path.clone();
                let list_len: usize = if let Ok(val) = crate::player::ipc::send_mpv_command(
                    &conn_socket,
                    vec![json!("get_property"), json!("playlist")],
                ) {
                    val.as_array().map(|a| a.len()).unwrap_or(1)
                } else {
                    1
                };

                // Move from the end to target position
                if list_len > 1 {
                    let _ = crate::player::ipc::send_mpv_command(
                        &conn_socket,
                        vec![
                            json!("playlist-move"),
                            json!(list_len - 1),
                            json!(target_pos),
                        ],
                    );
                }

                inserted_songs.push((target_pos, song));
                target_pos += 1;
            }

            // Sync with local queue
            let mut songs = queue.songs;
            for (pos, s) in inserted_songs.iter() {
                if *pos <= songs.len() {
                    songs.insert(*pos, s.clone());
                } else {
                    songs.push(s.clone());
                }
            }

            let updated_queue = PlayQueue {
                songs,
                current_index: Some(current_idx),
            };
            save_queue(&updated_queue)?;

            if json_out {
                println!(
                    "{}",
                    serde_json::json!({ "status": "inserted", "count": inserted_songs.len() })
                );
            } else {
                for (_, s) in inserted_songs {
                    println!(
                        "✓ Inserted \"{} - {}\" after current song.",
                        s.singer, s.name
                    );
                }
            }
        }
        QueueAction::Remove { position } => {
            if position == 0 || position > queue.songs.len() {
                return Err(anyhow!("Invalid position: {}", position));
            }
            let idx = position - 1;

            // Remove from mpv
            let conn_socket = client.socket_path.clone();
            let _ = crate::player::ipc::send_mpv_command(
                &conn_socket,
                vec![json!("playlist-remove"), json!(idx)],
            );

            let mut songs = queue.songs;
            let removed = songs.remove(idx);

            let mut new_current = queue.current_index;
            if let Some(curr) = queue.current_index {
                if curr >= songs.len() {
                    new_current = if songs.is_empty() {
                        None
                    } else {
                        Some(songs.len() - 1)
                    };
                } else if curr > idx {
                    new_current = Some(curr - 1);
                }
            }

            let updated_queue = PlayQueue {
                songs,
                current_index: new_current,
            };
            save_queue(&updated_queue)?;

            if json_out {
                println!(
                    "{}",
                    serde_json::json!({ "status": "removed", "song": removed.name })
                );
            } else {
                println!(
                    "✓ Removed \"{} - {}\" from queue.",
                    removed.singer, removed.name
                );
            }
        }
        QueueAction::Clear => {
            let conn_socket = client.socket_path.clone();
            let _ =
                crate::player::ipc::send_mpv_command(&conn_socket, vec![json!("playlist-clear")]);
            let _ = client.stop();

            let updated_queue = PlayQueue {
                songs: Vec::new(),
                current_index: None,
            };
            save_queue(&updated_queue)?;

            if json_out {
                println!("{}", serde_json::json!({ "status": "cleared" }));
            } else {
                println!("✓ Queue cleared.");
            }
        }
        QueueAction::Move { from, to } => {
            if from == 0 || from > queue.songs.len() || to == 0 || to > queue.songs.len() {
                return Err(anyhow!("Positions must be within range"));
            }
            let f = from - 1;
            let t = to - 1;

            let conn_socket = client.socket_path.clone();
            let _ = crate::player::ipc::send_mpv_command(
                &conn_socket,
                vec![json!("playlist-move"), json!(f), json!(t)],
            );

            let mut songs = queue.songs;
            let item = songs.remove(f);
            songs.insert(t, item);

            let mut new_current = queue.current_index;
            if let Some(curr) = queue.current_index {
                if curr == f {
                    new_current = Some(t);
                } else if f < curr && t >= curr {
                    new_current = Some(curr - 1);
                } else if f > curr && t <= curr {
                    new_current = Some(curr + 1);
                }
            }

            let updated_queue = PlayQueue {
                songs,
                current_index: new_current,
            };
            save_queue(&updated_queue)?;

            if json_out {
                println!(
                    "{}",
                    serde_json::json!({ "status": "moved", "from": from, "to": to })
                );
            } else {
                println!("✓ Moved song from position {} to {}.", from, to);
            }
        }
    }
    Ok(())
}

fn paths_to_current_json() -> std::path::PathBuf {
    let paths = lux_core::config::resolve_paths();
    paths.cache_dir.join("current.json")
}

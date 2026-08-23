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
    load_queue_from(&queue_json_path)
}

/// Load the queue from an explicit path. A missing file initializes an
/// empty queue; a corrupt one is a hard, read-only error — callers must
/// never persist a substituted empty queue over data we failed to parse.
pub(crate) fn load_queue_from(path: &std::path::Path) -> Result<PlayQueue> {
    if !path.exists() {
        return Ok(PlayQueue {
            songs: Vec::new(),
            current_index: None,
        });
    }
    let content = fs::read_to_string(path)
        .map_err(|e| anyhow!("Failed to read queue file {}: {e}", path.display()))?;
    serde_json::from_str(&content).map_err(|e| {
        anyhow!(
            "Queue file {} is corrupted and was left untouched (no empty queue was written); \
             fix or remove it manually: {e}",
            path.display()
        )
    })
}

pub fn save_queue(queue: &PlayQueue) -> Result<()> {
    let paths = lux_core::config::resolve_paths();
    let queue_json_path = paths.cache_dir.join("queue.json");
    save_queue_to(&queue_json_path, queue)
}

pub(crate) fn save_queue_to(path: &std::path::Path, queue: &PlayQueue) -> Result<()> {
    atomic_write(path, &serde_json::to_string(queue)?)
}

/// Write via temp-file + rename so readers never observe torn content.
pub(crate) fn atomic_write(path: &std::path::Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    fs::write(&tmp, contents)?;
    match fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(anyhow!(
                "Failed to atomically replace {}: {e}",
                path.display()
            ))
        }
    }
}

pub async fn run(action: QueueAction, json_out: bool) -> Result<()> {
    // A corrupt queue file is fatal and read-only; never fall back to an
    // empty queue that would then be persisted over the damaged original.
    let mut queue = load_or_init_queue()?;
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
            // Inserting happens after the currently playing song; without a
            // known playing position there is no anchor, so fail explicitly
            // instead of silently assuming index 0.
            let current_idx = match client.get_playing_index() {
                Ok(Some(idx)) => idx,
                Ok(None) => {
                    return Err(anyhow!(
                        "Nothing is playing; start playback or use 'alx queue add' to append instead."
                    ));
                }
                Err(e) => return Err(e),
            };
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
                vec![
                    json!("playlist-move"),
                    json!(f),
                    json!(mpv_playlist_move_target(f, t)),
                ],
            );

            let mut songs = queue.songs;
            let item = songs.remove(f);
            songs.insert(t, item);

            let new_current = adjust_current_index_after_move(queue.current_index, f, t);

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

/// mpv `playlist-move` target argument that lands an entry at final
/// position `to`. mpv interprets the second argument as "the entry whose
/// place is taken", so moving forward needs `to + 1`; moving backward uses
/// `to` directly.
pub(crate) fn mpv_playlist_move_target(from: usize, to: usize) -> usize {
    if from < to { to + 1 } else { to }
}

/// Current-index bookkeeping equivalent to local `remove(from)` + `insert(to)`.
fn adjust_current_index_after_move(curr: Option<usize>, from: usize, to: usize) -> Option<usize> {
    match curr {
        Some(c) if c == from => Some(to),
        Some(c) if from < c && c <= to => Some(c - 1),
        Some(c) if to <= c && c < from => Some(c + 1),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Simulate mpv's own interpretation of `playlist-move from arg`:
    /// remove at `from`, then insert so the entry takes the place of the
    /// element now at index `arg` (i.e. before it).
    fn simulate_mpv_move(order: &mut Vec<usize>, from: usize, to: usize) {
        let arg = mpv_playlist_move_target(from, to);
        let item = order.remove(from);
        let ins = if from < arg { arg - 1 } else { arg };
        order.insert(ins.min(order.len()), item);
    }

    fn assert_move_equivalent(n: usize, from: usize, to: usize) {
        let mut local: Vec<usize> = (0..n).collect();
        let item = local.remove(from);
        local.insert(to, item);

        let mut mpv: Vec<usize> = (0..n).collect();
        simulate_mpv_move(&mut mpv, from, to);

        assert_eq!(
            local, mpv,
            "local remove+insert must match mpv playlist-move ({from}->{to})"
        );
    }

    #[test]
    fn move_forward_matches_mpv_semantics() {
        assert_move_equivalent(4, 0, 2);
        assert_move_equivalent(4, 1, 3);
        assert_move_equivalent(5, 0, 4);
        assert_eq!(mpv_playlist_move_target(0, 2), 3);
        assert_eq!(mpv_playlist_move_target(1, 3), 4);
    }

    #[test]
    fn move_backward_matches_mpv_semantics() {
        assert_move_equivalent(4, 2, 0);
        assert_move_equivalent(4, 3, 1);
        assert_move_equivalent(5, 4, 0);
        assert_eq!(mpv_playlist_move_target(2, 0), 0);
        assert_eq!(mpv_playlist_move_target(3, 1), 1);
    }

    #[test]
    fn move_to_same_position_is_noop() {
        assert_move_equivalent(4, 1, 1);
        assert_eq!(mpv_playlist_move_target(1, 1), 1);
    }

    #[test]
    fn move_adjacent_positions() {
        assert_move_equivalent(4, 0, 1);
        assert_move_equivalent(4, 1, 0);
        assert_move_equivalent(4, 2, 3);
        assert_move_equivalent(4, 3, 2);
    }

    #[test]
    fn current_index_follows_moved_entry() {
        // The playing song itself is moved.
        assert_eq!(adjust_current_index_after_move(Some(1), 1, 3), Some(3));
        assert_eq!(adjust_current_index_after_move(Some(3), 3, 0), Some(0));
    }

    #[test]
    fn current_index_shifts_when_crossed_by_move() {
        // Move crosses the current song from below.
        assert_eq!(adjust_current_index_after_move(Some(2), 0, 3), Some(1));
        // Move crosses the current song from above.
        assert_eq!(adjust_current_index_after_move(Some(1), 3, 0), Some(2));
        // Boundary: target equals current slot.
        assert_eq!(adjust_current_index_after_move(Some(2), 0, 2), Some(1));
        assert_eq!(adjust_current_index_after_move(Some(2), 3, 2), Some(3));
    }

    #[test]
    fn current_index_untouched_by_disjoint_move() {
        assert_eq!(adjust_current_index_after_move(Some(1), 2, 3), Some(1));
        assert_eq!(adjust_current_index_after_move(None, 2, 3), None);
    }

    fn tmp_dir(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("alx-queue-test-{}-{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn entry(id: &str) -> SearchCacheEntry {
        SearchCacheEntry {
            cli_id: id.to_string(),
            song_id: id.to_string(),
            name: format!("Song {id}"),
            singer: "Artist".to_string(),
            source: "wy".to_string(),
            interval: None,
            album_name: None,
            album_id: None,
            pic_url: None,
            songmid: None,
            hash: None,
            extra: None,
        }
    }

    #[test]
    fn atomic_write_creates_and_replaces_without_residue() {
        let dir = tmp_dir("atomic");
        let path = dir.join("nested").join("queue.json");

        atomic_write(&path, "{\"v\":1}").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "{\"v\":1}");
        assert!(path.is_file());

        atomic_write(&path, "{\"v\":2}").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "{\"v\":2}");

        // No temp residue left behind.
        let leftovers: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.contains(".tmp."))
            .collect();
        assert!(leftovers.is_empty(), "residue: {leftovers:?}");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn queue_roundtrip_through_explicit_path() {
        let dir = tmp_dir("roundtrip");
        let path = dir.join("queue.json");

        save_queue_to(
            &path,
            &PlayQueue {
                songs: vec![entry("a"), entry("b")],
                current_index: Some(1),
            },
        )
        .unwrap();

        let loaded = load_queue_from(&path).unwrap();
        assert_eq!(loaded.songs.len(), 2);
        assert_eq!(loaded.current_index, Some(1));
        assert_eq!(loaded.songs[1].cli_id, "b");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_initializes_empty() {
        let dir = tmp_dir("missing");
        let loaded = load_queue_from(&dir.join("absent.json")).unwrap();
        assert!(loaded.songs.is_empty());
        assert_eq!(loaded.current_index, None);
        // Loading must not create the file either.
        assert!(!dir.join("absent.json").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_file_errors_read_only_and_is_never_clobbered() {
        let dir = tmp_dir("corrupt");
        let path = dir.join("queue.json");
        let torn = "{\"songs\": [{\"cli_id\": \"a\""; // truncated JSON
        fs::write(&path, torn).unwrap();

        let err = load_queue_from(&path).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("corrupted") && msg.contains("left untouched"),
            "error must be explicit and read-only: {msg}"
        );

        // The original bytes are intact — no empty-queue substitution.
        assert_eq!(fs::read_to_string(&path).unwrap(), torn);

        let _ = fs::remove_dir_all(&dir);
    }
}

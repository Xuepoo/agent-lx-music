#![allow(clippy::collapsible_if, clippy::collapsible_else_if)]
use crate::library::db::{SearchCacheEntry, add_to_history, get_song_by_cli_id};
use crate::player::MpvClient;
use crate::source::SourceManager;
use anyhow::{Result, anyhow};
use colored::Colorize;
use lux_core::types::Quality;
use serde_json::json;
use std::fs;
use std::path::Path;
use std::thread;
use std::time::Duration;

use rand::seq::SliceRandom;

/// Summary of a successful playback start.
pub struct PlayOutcome {
    pub first: SearchCacheEntry,
    pub url: String,
    pub queue_count: usize,
}

/// Synthetic queue entry for direct stream URLs / local files.
///
/// Shared by `alx play` (URL/path arguments) and the MCP `play` tool.
pub fn direct_entry(play_target: String) -> SearchCacheEntry {
    let filename = Path::new(&play_target)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(&play_target);

    SearchCacheEntry {
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
        extra: Some(play_target),
    }
}

/// Resolve entries into playable URLs, drive mpv playback, and persist the
/// queue/current/history state.
///
/// The first entry starts playing immediately; remaining entries are
/// appended (best-effort, mirroring `alx play`). `replace_queue = true`
/// clears mpv's playlist first — the CLI behavior. Local-source entries
/// (`source == "local"`) play their stored path from `extra`.
///
/// Shared by `alx play` (URL/path and CLI-ID cases) and the MCP `play`
/// tool; all output stays with the callers.
/// Time/space complexity: O(k) URL resolutions for k entries / O(n).
pub fn start_playback(
    entries: Vec<SearchCacheEntry>,
    quality: Quality,
    replace_queue: bool,
) -> Result<PlayOutcome> {
    let first = entries.first().ok_or_else(|| anyhow!("No songs to play"))?;
    let client = MpvClient::new();
    let mgr = SourceManager::new();

    if replace_queue {
        let _ = crate::player::ipc::send_mpv_command(
            &client.socket_path,
            vec![json!("playlist-clear")],
        );
    }

    let resolve_target = |entry: &SearchCacheEntry| -> Result<String> {
        if entry.source == "local" {
            Ok(entry.extra.clone().unwrap_or_default())
        } else {
            mgr.resolve_url(&entry.source, &entry.song_id, quality)
        }
    };

    let first_url = resolve_target(first)?;
    client.play_file_or_url(&first_url)?;

    let mut added_songs = vec![first.clone()];
    for song in entries.iter().skip(1) {
        if let Ok(url) = resolve_target(song)
            && !url.is_empty()
        {
            client.append_file_or_url(&url)?;
            added_songs.push(song.clone());
        }
    }

    let updated_queue = crate::cmd::queue::PlayQueue {
        songs: added_songs.clone(),
        current_index: Some(0),
    };
    crate::cmd::queue::save_queue(&updated_queue)?;

    // State will be synced by mpv Lua script after load
    save_currently_playing(first)?;
    let _ = add_to_history(first, None);

    Ok(PlayOutcome {
        first: first.clone(),
        url: first_url,
        queue_count: added_songs.len(),
    })
}

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

        // State will be synced by mpv Lua script after load
        save_currently_playing(first_song)?;
        let _ = add_to_history(first_song, None);

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
                        // Support both PlaybackState (new) and SearchCacheEntry (old)
                        let (song, last_pos, volume) = if val.get("song").is_some() {
                            let state: crate::library::db::PlaybackState =
                                serde_json::from_value(val.clone())?;
                            (state.song, state.last_position, state.volume)
                        } else {
                            let entry: SearchCacheEntry = serde_json::from_value(val.clone())?;
                            (entry, 0.0, config.player.default_volume)
                        };

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
                                resume_seek(&client, last_pos, json);
                            }
                            return Ok(());
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
        // Collect every valid direct target, then start playback through the
        // shared core (first target plays, the rest append).
        let mut targets = Vec::new();
        for target in id_or_urls.iter() {
            let is_u = target.starts_with("http://") || target.starts_with("https://");
            let exp_p = lux_core::config::expand_path(target);

            if is_u {
                targets.push(target.clone());
            } else if exp_p.exists() {
                targets.push(exp_p.to_string_lossy().to_string());
            }
        }

        if !targets.is_empty() {
            let entries: Vec<SearchCacheEntry> = targets.into_iter().map(direct_entry).collect();
            let outcome = start_playback(entries, quality, true)?;

            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "status": "playing",
                        "count": outcome.queue_count,
                        "first": outcome.first.name
                    })
                );
            } else {
                println!(
                    "{} Playing {} direct items. Current: {}",
                    "▶".green().bold(),
                    outcome.queue_count,
                    outcome.first.name.cyan()
                );
            }
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

    let outcome = start_playback(songs, quality, true)?;

    if json {
        println!(
            "{}",
            serde_json::json!({
                "status": "playing",
                "id": outcome.first.cli_id,
                "name": outcome.first.name,
                "singer": outcome.first.singer,
                "source": outcome.first.source,
                "url": outcome.url,
                "queue_count": outcome.queue_count
            })
        );
    } else {
        println!(
            "\n{} Started playing: {} — {}",
            "▶".green().bold(),
            outcome.first.name.bold(),
            outcome.first.singer.cyan()
        );
        if let Some(ref album) = outcome.first.album_name {
            println!("  Album:  {}", album);
        }
        println!(
            "  Source: {} | Quality: {} | Queue: {} songs",
            outcome.first.source.green(),
            quality.to_string().yellow(),
            outcome.queue_count
        );
        println!();
    }

    Ok(())
}

fn save_currently_playing(entry: &SearchCacheEntry) -> Result<()> {
    let config = lux_core::config::Config::load().unwrap_or_default();
    let paths = lux_core::config::resolve_paths();
    let current_json_path = paths.cache_dir.join("current.json");
    if let Some(parent) = current_json_path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let state = crate::library::db::PlaybackState {
        song: entry.clone(),
        last_position: 0.0,
        volume: config.player.default_volume,
        updated_at: chrono::Local::now().to_rfc3339(),
    };

    let serialized = serde_json::to_string(&state)?;
    fs::write(current_json_path, serialized)?;
    Ok(())
}

/// Wait until mpv reports the freshly loaded file as seekable, then seek.
///
/// The fixed 300 ms sleep this replaces raced `loadfile` on slow sources and
/// discarded seek failures. Now bounded polling (up to
/// [`SEEK_POLL_ATTEMPTS`] x [`SEEK_POLL_INTERVAL`]) waits for the
/// `seekable` property; a timeout or failed seek is reported as a warning
/// plus non-fatal status line instead of aborting the resume.
const SEEK_POLL_ATTEMPTS: u32 = 50;
const SEEK_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Poll `probe` up to `max_attempts` times every `interval`; true on first
/// success. Test seam: pass `Duration::ZERO` to run without real waiting.
fn poll_until<F: FnMut() -> bool>(mut probe: F, max_attempts: u32, interval: Duration) -> bool {
    for _ in 0..max_attempts {
        if probe() {
            return true;
        }
        thread::sleep(interval);
    }
    false
}

fn resume_seek(client: &MpvClient, last_pos: f64, json: bool) {
    let socket = client.socket_path.clone();
    let ready = poll_until(
        || {
            crate::player::ipc::send_mpv_command(
                &socket,
                vec![json!("get_property"), json!("seekable")],
            )
            .map(|v| v.as_bool().unwrap_or(false))
            .unwrap_or(false)
        },
        SEEK_POLL_ATTEMPTS,
        SEEK_POLL_INTERVAL,
    );

    if !ready {
        let waited_secs = SEEK_POLL_ATTEMPTS as u64 * SEEK_POLL_INTERVAL.as_millis() as u64 / 1000;
        eprintln!(
            "warning: playback not seekable after {waited_secs}s; resumed from the beginning"
        );
        report_seek_skipped(json);
        return;
    }

    if let Err(e) = client.seek(&last_pos.to_string()) {
        eprintln!("warning: resume seek to {last_pos:.1}s failed: {e:#}");
        report_seek_skipped(json);
    }
}

fn report_seek_skipped(json: bool) {
    if json {
        println!(
            "{}",
            serde_json::json!({ "status": "resumed", "seek": "skipped" })
        );
    }
}

#[cfg(test)]
mod tests {
    use super::poll_until;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    #[test]
    fn returns_immediately_when_probe_succeeds() {
        let calls = Arc::new(AtomicU32::new(0));
        let c2 = calls.clone();
        let ok = poll_until(
            || {
                c2.fetch_add(1, Ordering::SeqCst);
                true
            },
            50,
            Duration::ZERO,
        );
        assert!(ok);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn exhausts_attempts_and_reports_calls() {
        let calls = Arc::new(AtomicU32::new(0));
        let c2 = calls.clone();
        let ok = poll_until(
            || {
                c2.fetch_add(1, Ordering::SeqCst);
                false
            },
            7,
            Duration::ZERO,
        );
        assert!(!ok);
        assert_eq!(calls.load(Ordering::SeqCst), 7);
    }

    #[test]
    fn succeeds_on_later_attempt() {
        let calls = Arc::new(AtomicU32::new(0));
        let c2 = calls.clone();
        let ok = poll_until(
            || c2.fetch_add(1, Ordering::SeqCst) >= 3,
            50,
            Duration::ZERO,
        );
        assert!(ok);
        assert_eq!(calls.load(Ordering::SeqCst), 4);
    }
}

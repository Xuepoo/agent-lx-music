pub mod board;
pub mod config;
pub mod discover;
pub mod download;
pub mod fav;
pub mod history;
pub mod local;
pub mod lyric;
pub mod now;
pub mod pic;
pub mod play;
pub mod playlist;
pub mod queue;
pub mod search;
pub mod source;
pub mod state;

use crate::cli::Commands;
use anyhow::{Result, anyhow};
use colored::Colorize;
use rand::seq::SliceRandom;

pub async fn dispatch(command: Commands, json: bool) -> Result<()> {
    match command {
        Commands::Config { action } => {
            config::run(action, json)?;
        }
        Commands::Search {
            keyword,
            source,
            page,
            limit,
            id_only,
        } => {
            search::run(keyword, source, page, limit, id_only, json).await?;
        }
        Commands::Play {
            id_or_url,
            quality,
            from_playlist,
            shuffle,
        } => {
            play::run(id_or_url, quality, from_playlist, shuffle, json).await?;
        }
        Commands::Next => {
            let client = crate::player::MpvClient::new();
            client.next()?;
            if !json {
                println!("{} Skipped to the next song.", "⏭".green().bold());
            } else {
                println!(
                    "{}",
                    serde_json::json!({ "status": "skipped", "direction": "next" })
                );
            }
        }
        Commands::Prev => {
            let client = crate::player::MpvClient::new();
            client.prev()?;
            if !json {
                println!("{} Skipped to the previous song.", "⏮".green().bold());
            } else {
                println!(
                    "{}",
                    serde_json::json!({ "status": "skipped", "direction": "prev" })
                );
            }
        }
        Commands::Now => {
            now::run(json)?;
        }
        Commands::Pause => {
            let client = crate::player::MpvClient::new();
            client.pause()?;
            if !json {
                println!("{} Playback paused.", "⏸".yellow().bold());
            } else {
                println!("{}", serde_json::json!({ "status": "paused" }));
            }
        }
        Commands::Resume => {
            let client = crate::player::MpvClient::new();
            client.resume()?;
            if !json {
                println!("{} Playback resumed.", "▶".green().bold());
            } else {
                println!("{}", serde_json::json!({ "status": "resumed" }));
            }
        }
        Commands::Stop => {
            let client = crate::player::MpvClient::new();
            client.stop()?;
            if !json {
                println!("{} Playback stopped.", "■".red().bold());
            } else {
                println!("{}", serde_json::json!({ "status": "stopped" }));
            }
        }
        Commands::Volume { value } => {
            let client = crate::player::MpvClient::new();
            if let Some(val) = value {
                if val.starts_with('+') || val.starts_with('-') {
                    let current = client.get_volume().unwrap_or(80);
                    let diff: i32 = val.parse().unwrap_or(0);
                    let new_vol = (current as i32 + diff).clamp(0, 100) as u8;
                    client.set_volume(new_vol)?;
                    if !json {
                        println!("Volume updated to {}%.", new_vol);
                    } else {
                        println!("{}", serde_json::json!({ "volume": new_vol }));
                    }
                } else {
                    let new_vol: u8 = val.parse().map_err(|_| anyhow!("Invalid volume value"))?;
                    client.set_volume(new_vol)?;
                    if !json {
                        println!("Volume set to {}%.", new_vol);
                    } else {
                        println!("{}", serde_json::json!({ "volume": new_vol }));
                    }
                }
            } else {
                let current = client.get_volume().unwrap_or(80);
                if !json {
                    println!("Current volume: {}%.", current);
                } else {
                    println!("{}", serde_json::json!({ "volume": current }));
                }
            }
        }
        Commands::Seek { value } => {
            let client = crate::player::MpvClient::new();
            client.seek(&value)?;
            if !json {
                println!("Seek to position: {}.", value);
            } else {
                println!(
                    "{}",
                    serde_json::json!({ "status": "seeked", "position": value })
                );
            }
        }
        Commands::Repeat { mode } => {
            let client = crate::player::MpvClient::new();
            if let Some(m) = mode {
                client.set_repeat(&m)?;
                if !json {
                    println!("Repeat mode set to '{}'.", m);
                } else {
                    println!("{}", serde_json::json!({ "repeat": m }));
                }
            } else {
                let loop_file = crate::player::ipc::send_mpv_command(
                    &client.socket_path,
                    vec![
                        serde_json::json!("get_property"),
                        serde_json::json!("loop-file"),
                    ],
                )
                .ok()
                .and_then(|v| v.as_str().map(|s| s.to_string()))
                .unwrap_or_else(|| "no".to_string());

                let loop_playlist = crate::player::ipc::send_mpv_command(
                    &client.socket_path,
                    vec![
                        serde_json::json!("get_property"),
                        serde_json::json!("loop-playlist"),
                    ],
                )
                .ok()
                .and_then(|v| v.as_str().map(|s| s.to_string()))
                .unwrap_or_else(|| "no".to_string());

                let repeat_mode = if loop_file == "inf" || loop_file == "yes" {
                    "one"
                } else if loop_playlist == "inf" || loop_playlist == "yes" {
                    "all"
                } else {
                    "off"
                };

                if !json {
                    println!("Repeat mode: {}.", repeat_mode);
                } else {
                    println!("{}", serde_json::json!({ "repeat": repeat_mode }));
                }
            }
        }
        Commands::Shuffle { mode } => {
            let client = crate::player::MpvClient::new();
            match mode.as_deref() {
                Some("on") => {
                    client.set_repeat("off")?;
                    client.ensure_running()?;

                    // Shuffle the authoritative Rust-side queue, then rebuild
                    // the mpv playlist order to mirror it. The playing entry
                    // is tracked through the permutation so playback keeps
                    // running uninterrupted.
                    let mut queue = crate::cmd::queue::load_or_init_queue()?;
                    let playing = client
                        .get_playing_index()
                        .ok()
                        .flatten()
                        .or(queue.current_index);

                    let n = queue.songs.len();
                    if n > 1 {
                        let mut labels: Vec<usize> = (0..n).collect();
                        labels.shuffle(&mut rand::thread_rng());

                        for (from, to) in plan_playlist_moves(&labels) {
                            crate::player::ipc::send_mpv_command(
                                &client.socket_path.clone(),
                                vec![
                                    serde_json::json!("playlist-move"),
                                    serde_json::json!(from),
                                    serde_json::json!(crate::cmd::queue::mpv_playlist_move_target(
                                        from, to
                                    )),
                                ],
                            )?;
                        }

                        queue.current_index =
                            playing.and_then(|c| labels.iter().position(|&l| l == c));
                        crate::cmd::queue::save_queue(&queue)?;
                    }

                    let _ = crate::player::ipc::send_mpv_command(
                        &client.socket_path,
                        vec![
                            serde_json::json!("set_property"),
                            serde_json::json!("shuffle"),
                            serde_json::json!(true),
                        ],
                    );
                    if !json {
                        println!("Shuffle mode enabled.");
                    } else {
                        println!("{}", serde_json::json!({ "shuffle": "on" }));
                    }
                }
                Some(_) => {
                    let _ = crate::player::ipc::send_mpv_command(
                        &client.socket_path,
                        vec![
                            serde_json::json!("set_property"),
                            serde_json::json!("shuffle"),
                            serde_json::json!(false),
                        ],
                    );
                    if !json {
                        println!("Shuffle mode disabled.");
                    } else {
                        println!("{}", serde_json::json!({ "shuffle": "off" }));
                    }
                }
                None => {
                    let shuffle_prop = crate::player::ipc::send_mpv_command(
                        &client.socket_path,
                        vec![
                            serde_json::json!("get_property"),
                            serde_json::json!("shuffle"),
                        ],
                    )
                    .ok()
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                    if !json {
                        println!("Shuffle: {}.", if shuffle_prop { "on" } else { "off" });
                    } else {
                        println!(
                            "{}",
                            serde_json::json!({ "shuffle": if shuffle_prop { "on" } else { "off" } })
                        );
                    }
                }
            }
        }
        Commands::State => {
            state::run(json)?;
        }
        Commands::Quit => {
            let client = crate::player::MpvClient::new();
            client.quit()?;
            if !json {
                println!("{} mpv daemon exited.", "✓".green().bold());
            } else {
                println!("{}", serde_json::json!({ "status": "exited" }));
            }
        }
        Commands::Source { action } => {
            source::run(action, json).await?;
        }
        Commands::Download { action } => {
            download::run(action, json).await?;
        }
        Commands::Playlist { action } => {
            playlist::run(action, json).await?;
        }
        Commands::Local { action } => {
            local::run(action, json).await?;
        }
        Commands::Queue { action } => {
            queue::run(action, json).await?;
        }
        Commands::Fav { action } => {
            fav::run(action, json).await?;
        }
        Commands::History { limit } => {
            history::run(limit, json).await?;
        }
        Commands::Lyric {
            id,
            translated,
            romanized,
            save,
        } => {
            lyric::run(id, translated, romanized, save, json).await?;
        }
        Commands::Pic { id, save, output } => {
            pic::run(id, save, output, json).await?;
        }
        Commands::Completions { shell } => {
            use clap::CommandFactory;
            let mut cmd = crate::cli::Cli::command();
            clap_complete::generate(shell, &mut cmd, "alx", &mut std::io::stdout());
        }
        Commands::Board { source, id, play } => {
            board::run_board(source, id, play, json).await?;
        }
        Commands::Discover {
            source,
            tag,
            action,
        } => {
            discover::run_discover(source, tag, action, json).await?;
        }
    }
    Ok(())
}

/// Compute the `playlist-move` sequence transforming identity order
/// `(0..n)` into `desired` (a permutation of `0..n`).
///
/// Slots are filled in ascending order so entries already in their final
/// position are never disturbed. Each returned pair `(from, slot)` means
/// "move the entry currently at index `from` to final position `slot`".
/// Time/space complexity: O(n^2) / O(n) — negligible for playlist sizes.
fn plan_playlist_moves(desired: &[usize]) -> Vec<(usize, usize)> {
    debug_assert!({
        let mut sorted: Vec<usize> = desired.to_vec();
        sorted.sort_unstable();
        sorted == (0..desired.len()).collect::<Vec<usize>>()
    });

    let mut working: Vec<usize> = (0..desired.len()).collect();
    let mut moves = Vec::new();
    for (i, &want) in desired.iter().enumerate() {
        let j = working
            .iter()
            .position(|&x| x == want)
            .expect("desired is a permutation");
        if j != i {
            moves.push((j, i));
            working.remove(j);
            working.insert(i, want);
        }
    }
    moves
}

#[cfg(test)]
mod tests {
    use super::plan_playlist_moves;
    use crate::cmd::queue::mpv_playlist_move_target;

    /// Apply a planned move with local remove+insert semantics.
    fn apply_local(order: &mut Vec<usize>, from: usize, to: usize) {
        let item = order.remove(from);
        order.insert(to, item);
    }

    /// Simulate mpv's interpretation of the command we actually send.
    fn simulate_mpv(order: &mut Vec<usize>, from: usize, to: usize) {
        let arg = mpv_playlist_move_target(from, to);
        let item = order.remove(from);
        let ins = if from < arg { arg - 1 } else { arg };
        order.insert(ins.min(order.len()), item);
    }

    fn assert_plan_matches(desired: &[usize]) {
        let moves = plan_playlist_moves(desired);

        let mut local: Vec<usize> = (0..desired.len()).collect();
        let mut mpv: Vec<usize> = (0..desired.len()).collect();
        for &(from, to) in &moves {
            apply_local(&mut local, from, to);
            simulate_mpv(&mut mpv, from, to);
        }
        assert_eq!(local, desired, "local bookkeeping must reach desired order");
        assert_eq!(mpv, desired, "mpv commands must land mpv at the same order");
    }

    #[test]
    fn plan_identity_needs_no_moves() {
        assert!(plan_playlist_moves(&[0, 1, 2, 3]).is_empty());
        assert!(plan_playlist_moves(&[]).is_empty());
        assert!(plan_playlist_moves(&[0]).is_empty());
    }

    #[test]
    fn plan_single_swap() {
        assert_plan_matches(&[1, 0, 2, 3]);
    }

    #[test]
    fn plan_full_reversal() {
        assert_plan_matches(&[3, 2, 1, 0]);
    }

    #[test]
    fn plan_rotation() {
        assert_plan_matches(&[2, 3, 4, 0, 1]);
    }

    #[test]
    fn plan_keeps_current_track_position_when_desired_first() {
        // Current song (label 2) shuffled to front; the plan must still be
        // executable without disturbing already-placed slots.
        assert_plan_matches(&[2, 0, 3, 1]);
    }

    #[test]
    fn plan_of_large_permutation_is_consistent() {
        assert_plan_matches(&[4, 0, 7, 2, 6, 1, 5, 3]);
    }
}

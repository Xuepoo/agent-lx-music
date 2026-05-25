pub mod config;
pub mod download;
pub mod fav;
pub mod history;
pub mod local;
pub mod now;
pub mod play;
pub mod playlist;
pub mod queue;
pub mod search;
pub mod source;

use crate::cli::Commands;
use anyhow::{Result, anyhow};
use colored::Colorize;

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
                if !json {
                    println!("Repeat mode configured successfully.");
                } else {
                    println!("{}", serde_json::json!({ "status": "ok" }));
                }
            }
        }
        Commands::Shuffle { mode } => {
            let client = crate::player::MpvClient::new();
            if let Some(m) = mode {
                if m == "on" {
                    client.set_repeat("off")?;
                    if !json {
                        println!("Shuffle mode enabled.");
                    } else {
                        println!("{}", serde_json::json!({ "shuffle": "on" }));
                    }
                } else {
                    if !json {
                        println!("Shuffle mode disabled.");
                    } else {
                        println!("{}", serde_json::json!({ "shuffle": "off" }));
                    }
                }
            } else {
                if !json {
                    println!("Shuffle mode configured successfully.");
                } else {
                    println!("{}", serde_json::json!({ "status": "ok" }));
                }
            }
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
            source::run(action, json)?;
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
    }
    Ok(())
}

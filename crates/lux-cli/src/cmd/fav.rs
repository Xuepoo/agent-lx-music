use crate::cli::{FavAction, PlaylistAction};
use crate::cmd::playlist;
use crate::library::db;
use anyhow::{Result, anyhow};
use colored::Colorize;
use std::fs;

pub async fn run(action: FavAction, json: bool) -> Result<()> {
    match action {
        FavAction::List => {
            playlist::run(
                PlaylistAction::Show {
                    name: "Favorites".to_string(),
                },
                json,
            )
            .await?;
        }
        FavAction::Add { id } => {
            if let Some(cli_id) = id {
                playlist::run(
                    PlaylistAction::Add {
                        playlist: "Favorites".to_string(),
                        id: cli_id,
                    },
                    json,
                )
                .await?;
            } else {
                // Read from current.json
                let paths = lux_core::config::resolve_paths();
                let current_json_path = paths.cache_dir.join("current.json");
                if !current_json_path.exists() {
                    return Err(anyhow!(
                        "No song is currently playing. Provide a CLI ID to add to favorites."
                    ));
                }

                let content = fs::read_to_string(current_json_path)?;
                let val: serde_json::Value = serde_json::from_str(&content)?;

                let song_val = if val.get("song").is_some() {
                    val.get("song").cloned().unwrap()
                } else {
                    val.clone()
                };

                let song: db::SearchCacheEntry = serde_json::from_value(song_val)?;
                db::init_db()?;
                db::add_to_playlist("Favorites", &song)?;

                if json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "status": "added",
                            "playlist": "Favorites",
                            "song": song.name
                        })
                    );
                } else {
                    println!(
                        "✓ Added \"{} - {}\" to playlist \"{}\".",
                        song.singer,
                        song.name,
                        "Favorites".green().bold()
                    );
                }
            }
        }
        FavAction::Remove { id } => {
            playlist::run(
                PlaylistAction::Remove {
                    playlist: "Favorites".to_string(),
                    id,
                },
                json,
            )
            .await?;
        }
        FavAction::Play { shuffle } => {
            playlist::run(
                PlaylistAction::Play {
                    name: "Favorites".to_string(),
                    shuffle,
                },
                json,
            )
            .await?;
        }
    }
    Ok(())
}

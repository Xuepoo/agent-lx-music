use crate::cli::PlaylistAction;
use crate::library::db::{self, SearchCacheEntry, add_to_history};
use crate::player::MpvClient;
use crate::source::SourceManager;
use anyhow::{Result, anyhow};
use colored::Colorize;
use rand::seq::SliceRandom;
use std::fs;

#[derive(tabled::Tabled)]
struct PlaylistListTableEntry {
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Description")]
    description: String,
    #[tabled(rename = "Songs")]
    songs: i32,
}

#[derive(tabled::Tabled)]
struct PlaylistShowTableEntry {
    #[tabled(rename = "Index")]
    index: usize,
    #[tabled(rename = "Title")]
    title: String,
    #[tabled(rename = "Singer")]
    singer: String,
    #[tabled(rename = "Album")]
    album: String,
    #[tabled(rename = "Source")]
    source: String,
}

pub async fn run(action: PlaylistAction, json: bool) -> Result<()> {
    match action {
        PlaylistAction::List => {
            let playlists = db::list_playlists()?;
            if json {
                println!("{}", serde_json::to_string_pretty(&playlists)?);
            } else {
                if playlists.is_empty() {
                    println!("No playlists found. Create one with 'rlx playlist create <name>'");
                } else {
                    let mut data = Vec::new();
                    for (name, desc, count) in playlists {
                        data.push(PlaylistListTableEntry {
                            name,
                            description: desc.unwrap_or_default(),
                            songs: count,
                        });
                    }
                    let mut table = tabled::Table::new(data);
                    table.with(tabled::settings::Style::rounded());
                    println!("{}", table);
                }
            }
        }
        PlaylistAction::Create { name, description } => {
            db::create_playlist(&name, description.as_deref())?;
            if json {
                println!(
                    "{}",
                    serde_json::json!({ "status": "created", "name": name })
                );
            } else {
                println!(
                    "✓ Playlist \"{}\" created successfully.",
                    name.green().bold()
                );
            }
        }
        PlaylistAction::Delete { name } => {
            db::delete_playlist(&name)?;
            if json {
                println!(
                    "{}",
                    serde_json::json!({ "status": "deleted", "name": name })
                );
            } else {
                println!("✓ Playlist \"{}\" deleted.", name.red().bold());
            }
        }
        PlaylistAction::Add { playlist, id } => {
            let song = db::get_song_by_cli_id(&id)?.ok_or_else(|| {
                anyhow!(
                    "Song with CLI ID '{}' not found in cache. Search first.",
                    id
                )
            })?;

            db::add_to_playlist(&playlist, &song)?;

            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "status": "added",
                        "playlist": playlist,
                        "song": song.name
                    })
                );
            } else {
                println!(
                    "✓ Added \"{} - {}\" to playlist \"{}\".",
                    song.singer, song.name, playlist
                );
            }
        }
        PlaylistAction::Remove { playlist, id } => {
            let song = db::get_song_by_cli_id(&id)?
                .ok_or_else(|| anyhow!("Song with CLI ID '{}' not found in cache.", id))?;

            db::remove_from_playlist(&playlist, &song.song_id, &song.source)?;

            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "status": "removed",
                        "playlist": playlist,
                        "song": song.name
                    })
                );
            } else {
                println!(
                    "✓ Removed \"{} - {}\" from playlist \"{}\".",
                    song.singer, song.name, playlist
                );
            }
        }
        PlaylistAction::Show { name } => {
            let songs = db::get_playlist_songs(&name)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&songs)?);
            } else {
                if songs.is_empty() {
                    println!("Playlist \"{}\" is empty.", name);
                } else {
                    let mut data = Vec::new();
                    for (i, song) in songs.iter().enumerate() {
                        data.push(PlaylistShowTableEntry {
                            index: i + 1,
                            title: song.name.clone(),
                            singer: song.singer.clone(),
                            album: song.album_name.clone().unwrap_or_default(),
                            source: song.source.clone(),
                        });
                    }
                    let mut table = tabled::Table::new(data);
                    table.with(tabled::settings::Style::rounded());
                    println!("{}", table);
                }
            }
        }
        PlaylistAction::Play { name, shuffle } => {
            let mut songs = db::get_playlist_songs(&name)?;
            if songs.is_empty() {
                return Err(anyhow!("Playlist \"{}\" is empty or does not exist.", name));
            }

            if shuffle {
                let mut rng = rand::thread_rng();
                songs.shuffle(&mut rng);
            }

            let config = lux_core::config::Config::load().unwrap_or_default();
            let client = MpvClient::new();
            let mgr = SourceManager::new();

            let first = &songs[0];
            if !json {
                println!(
                    "{} Loading playlist \"{}\" ({} songs)...",
                    "⚡".yellow().bold(),
                    name.cyan(),
                    songs.len()
                );
                println!(
                    "{} Resolving playable URL for first track '{}'...",
                    "⚡".yellow().bold(),
                    first.name.cyan()
                );
            }

            let resolved_first =
                mgr.resolve_url(&first.source, &first.song_id, config.source.default_quality)?;

            client.play_file_or_url(&resolved_first)?;
            let _ = save_currently_playing(first);
            let _ = add_to_history(first, None);

            if !json {
                println!(
                    "{} Started playing: {} — {}",
                    "▶".green().bold(),
                    first.name.bold(),
                    first.singer.cyan()
                );
            }

            // Lazy append next songs asynchronously to ensure fast startup
            for song in songs.iter().skip(1) {
                if let Ok(resolved_url) =
                    mgr.resolve_url(&song.source, &song.song_id, config.source.default_quality)
                {
                    let _ = client.append_file_or_url(&resolved_url);
                }
            }

            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "status": "playing_playlist",
                        "playlist": name,
                        "first_song": first.name
                    })
                );
            }
        }
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

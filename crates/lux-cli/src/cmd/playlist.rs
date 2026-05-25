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
        PlaylistAction::Import {
            file,
            name,
            download,
            quality,
        } => {
            let file_path = std::path::Path::new(&file);
            if !file_path.exists() {
                return Err(anyhow!("Playlist file '{}' does not exist.", file));
            }
            let content = fs::read_to_string(file_path)?;

            let imported_tracks =
                crate::library::playlist_parser::parse_universal_playlist(&content);
            if imported_tracks.is_empty() {
                return Err(anyhow!(
                    "No valid tracks found in playlist file '{}'.",
                    file
                ));
            }

            let playlist_name = name.unwrap_or_else(|| {
                file_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("Imported Playlist")
                    .to_string()
            });

            db::create_playlist(&playlist_name, Some("Imported Playlist"))?;

            if !json {
                println!(
                    "{} Importing {} songs into playlist \"{}\"...",
                    "⚡".yellow().bold(),
                    imported_tracks.len(),
                    playlist_name.green().bold()
                );
            }

            let parsed_quality = quality.and_then(|q| q.parse::<lux_core::types::Quality>().ok());
            let config = lux_core::config::Config::load().unwrap_or_default();
            let dl_quality = parsed_quality.unwrap_or(config.source.default_quality);

            let mut success_count = 0;
            let mut resolved_songs = Vec::new();

            for (idx, track) in imported_tracks.iter().enumerate() {
                if !json {
                    println!(
                        "  [{}/{}] Matching: {} — {} ...",
                        idx + 1,
                        imported_tracks.len(),
                        track.title.bold(),
                        track.artist.cyan()
                    );
                }

                match crate::library::playlist_parser::resolve_imported_track(track, parsed_quality)
                    .await
                {
                    Ok(Some(entry)) => {
                        if db::add_to_playlist(&playlist_name, &entry).is_ok() {
                            success_count += 1;
                            resolved_songs.push(entry.clone());

                            if download {
                                let _ = db::insert_download(
                                    &entry.song_id,
                                    &entry.source,
                                    &entry.name,
                                    &entry.singer,
                                    &dl_quality.to_string(),
                                );
                            }
                        }
                    }
                    _ => {
                        if !json {
                            println!(
                                "  {} Failed to match or resolve this song.",
                                "✗".red().bold()
                            );
                        }
                    }
                }
            }

            if download && success_count > 0 {
                let _ = crate::cmd::download::ensure_daemon_running();
            }

            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "status": "imported",
                        "playlist": playlist_name,
                        "total": imported_tracks.len(),
                        "success": success_count
                    })
                );
            } else {
                println!(
                    "\n{} Successfully imported {}/{} songs into playlist \"{}\".",
                    "✓".green().bold(),
                    success_count,
                    imported_tracks.len(),
                    playlist_name.green().bold()
                );
                if download && success_count > 0 {
                    println!(
                        "{} Detached background download daemon triggered.",
                        "⚡".yellow().bold()
                    );
                }
            }
        }
        PlaylistAction::Export {
            name,
            format,
            output,
        } => {
            let songs = db::get_playlist_songs(&name)?;
            if songs.is_empty() {
                return Err(anyhow!("Playlist \"{}\" is empty or does not exist.", name));
            }

            let export_content = match format.to_lowercase().as_str() {
                "json" => serde_json::to_string_pretty(&songs)?,
                "csv" => {
                    let mut csv = String::from("Track Name,Artist Name(s),Album\n");
                    for song in &songs {
                        let safe_name = song.name.replace(',', " ");
                        let safe_singer = song.singer.replace(',', " ");
                        let safe_album = song.album_name.as_deref().unwrap_or("").replace(',', " ");
                        csv.push_str(&format!("{},{},{}\n", safe_name, safe_singer, safe_album));
                    }
                    csv
                }
                "txt" => {
                    let mut txt = String::new();
                    for song in &songs {
                        txt.push_str(&format!("{} - {}\n", song.singer, song.name));
                    }
                    txt
                }
                _ => {
                    let mut m3u = String::from("#EXTM3U\n");
                    for song in &songs {
                        m3u.push_str(&format!("#EXTINF:-1,{} - {}\n", song.singer, song.name));
                        m3u.push_str(&format!("rlx://{}/{}\n", song.source, song.song_id));
                    }
                    m3u
                }
            };

            let out_file = if let Some(out_path) = output {
                let path = std::path::Path::new(&out_path);
                if path.is_dir() {
                    path.join(format!("{}.{}", name, format))
                } else {
                    path.to_path_buf()
                }
            } else {
                std::env::current_dir()?.join(format!("{}.{}", name, format))
            };

            if let Some(parent) = out_file.parent() {
                let _ = fs::create_dir_all(parent);
            }
            fs::write(&out_file, export_content)?;

            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "status": "exported",
                        "playlist": name,
                        "file": out_file.to_string_lossy()
                    })
                );
            } else {
                println!(
                    "✓ Playlist \"{}\" successfully exported to '{}'.",
                    name.green().bold(),
                    out_file.to_string_lossy().cyan()
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

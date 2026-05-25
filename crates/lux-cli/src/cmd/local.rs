#![allow(clippy::collapsible_if, clippy::collapsible_else_if)]
use crate::cli::LocalAction;
use crate::library::db::SearchCacheEntry;
use crate::player::MpvClient;
use anyhow::{Result, anyhow};
use colored::Colorize;
use id3::{Tag as Id3Tag, TagLike as Id3TagLike};
use metaflac::Tag as FlacTag;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::process::Command;

#[derive(tabled::Tabled)]
struct LocalSongTableEntry {
    #[tabled(rename = "Index")]
    index: usize,
    #[tabled(rename = "Title")]
    title: String,
    #[tabled(rename = "Singer")]
    singer: String,
    #[tabled(rename = "Album")]
    album: String,
    #[tabled(rename = "Duration")]
    duration: String,
    #[tabled(rename = "Path")]
    path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalSong {
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration: Option<String>,
    pub filepath: String,
}

pub async fn run(action: LocalAction, json: bool) -> Result<()> {
    let paths = lux_core::config::resolve_paths();
    let cache_file = paths.cache_dir.join("local_library.json");

    match action {
        LocalAction::Scan => {
            let config = lux_core::config::Config::load().unwrap_or_default();
            let mut songs = Vec::new();

            if config.download.use_beets_library && is_command_available("beet") {
                if !json {
                    println!("{} Syncing library via beets...", "⚡".yellow().bold());
                }
                songs = scan_via_beets()?;
            } else {
                let music_dir = config.get_resolved_download_dir();
                if !json {
                    println!(
                        "{} Recursively scanning local directory '{}'...",
                        "⚡".yellow().bold(),
                        music_dir.display()
                    );
                }
                if music_dir.exists() {
                    scan_directory(&music_dir, &mut songs)?;
                }
            }

            // Write to cache file
            if let Some(parent) = cache_file.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let serialized = serde_json::to_string_pretty(&songs)?;
            fs::write(&cache_file, serialized)?;

            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "status": "scanned",
                        "count": songs.len()
                    })
                );
            } else {
                println!(
                    "✓ Successfully scanned and indexed {} offline tracks.",
                    songs.len().to_string().green().bold()
                );
            }
        }
        LocalAction::List => {
            let songs = load_local_cache(&cache_file)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&songs)?);
            } else {
                if songs.is_empty() {
                    println!("Local offline index is empty. Run 'rlx local scan' first.");
                } else {
                    let mut data = Vec::new();
                    for (i, s) in songs.iter().enumerate() {
                        data.push(LocalSongTableEntry {
                            index: i + 1,
                            title: s.title.clone(),
                            singer: s.artist.clone(),
                            album: s.album.clone().unwrap_or_default(),
                            duration: s.duration.clone().unwrap_or_default(),
                            path: s.filepath.clone(),
                        });
                    }
                    let mut table = tabled::Table::new(data);
                    table.with(tabled::settings::Style::rounded());
                    println!("{}", table);
                }
            }
        }
        LocalAction::Play { query } => {
            let songs = load_local_cache(&cache_file)?;
            if songs.is_empty() {
                return Err(anyhow!(
                    "Local offline index is empty. Run 'rlx local scan' first."
                ));
            }

            let mut selected = None;

            // 1. Try parsing as 1-indexed table index
            if let Ok(idx) = query.parse::<usize>() {
                if idx > 0 && idx <= songs.len() {
                    selected = Some(&songs[idx - 1]);
                }
            }

            // 2. Try fuzzy matching title/artist/filepath
            if selected.is_none() {
                let lower_query = query.to_lowercase();
                for s in &songs {
                    if s.title.to_lowercase().contains(&lower_query)
                        || s.artist.to_lowercase().contains(&lower_query)
                        || s.filepath.to_lowercase().contains(&lower_query)
                    {
                        selected = Some(s);
                        break;
                    }
                }
            }

            let song =
                selected.ok_or_else(|| anyhow!("No local track matched query '{}'.", query))?;

            if !json {
                println!(
                    "{} Offline play: {} — {}...",
                    "▶".green().bold(),
                    song.title.bold(),
                    song.artist.cyan()
                );
            }

            let client = MpvClient::new();
            client.play_file_or_url(&song.filepath)?;

            // Save currently playing info for Now commands
            let entry = SearchCacheEntry {
                cli_id: "local".to_string(),
                song_id: "local".to_string(),
                name: song.title.clone(),
                singer: song.artist.clone(),
                source: "local".to_string(),
                interval: song.duration.clone(),
                album_name: song.album.clone(),
                album_id: None,
                pic_url: None,
                songmid: None,
                hash: None,
                extra: Some(song.filepath.clone()),
            };
            let _ = save_currently_playing(&entry);

            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "status": "playing",
                        "title": song.title,
                        "artist": song.artist,
                        "file": song.filepath
                    })
                );
            }
        }
    }
    Ok(())
}

fn is_command_available(cmd: &str) -> bool {
    Command::new("which")
        .arg(cmd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn scan_via_beets() -> Result<Vec<LocalSong>> {
    let out = Command::new("beet")
        .arg("export")
        .arg("--format")
        .arg("json")
        .output()?;

    if !out.status.success() {
        return Err(anyhow!(
            "Beets export failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }

    let json_str = String::from_utf8_lossy(&out.stdout);

    // Parse beets JSON structure. Beets returns a JSON array of items
    let items: Vec<serde_json::Value> = serde_json::from_str(&json_str)?;

    let mut songs = Vec::new();

    for item in items {
        let title = item
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown")
            .to_string();
        let artist = item
            .get("artist")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown")
            .to_string();
        let album = item
            .get("album")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let duration = item.get("length").and_then(|v| v.as_f64()).map(|secs| {
            let m = (secs / 60.0) as i32;
            let s = (secs % 60.0) as i32;
            format!("{:02}:{:02}", m, s)
        });

        let filepath = item
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        if filepath.is_empty() {
            continue;
        }

        songs.push(LocalSong {
            title,
            artist,
            album,
            duration,
            filepath,
        });
    }

    Ok(songs)
}

fn scan_directory(dir: &Path, songs: &mut Vec<LocalSong>) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            let _ = scan_directory(&path, songs);
        } else if path.is_file() {
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or_default()
                .to_lowercase();
            if ext == "mp3" || ext == "flac" {
                if let Some(song) = parse_local_file(&path) {
                    songs.push(song);
                }
            }
        }
    }

    Ok(())
}

fn parse_local_file(path: &Path) -> Option<LocalSong> {
    let filepath = path.to_string_lossy().to_string();
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_lowercase();

    let mut title = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Unknown")
        .to_string();
    let mut artist = "Unknown".to_string();
    let mut album = None;

    if ext == "flac" {
        if let Ok(tag) = FlacTag::read_from_path(path) {
            if let Some(vorbis) = tag.vorbis_comments() {
                if let Some(t) = vorbis.title() {
                    if !t.is_empty() {
                        title = t[0].clone();
                    }
                }
                if let Some(a) = vorbis.artist() {
                    if !a.is_empty() {
                        artist = a[0].clone();
                    }
                }
                if let Some(al) = vorbis.album() {
                    if !al.is_empty() {
                        album = Some(al[0].clone());
                    }
                }
            }
        }
    } else {
        if let Ok(tag) = Id3Tag::read_from_path(path) {
            if let Some(t) = tag.title() {
                title = t.to_string();
            }
            if let Some(a) = tag.artist() {
                artist = a.to_string();
            }
            if let Some(al) = tag.album() {
                album = Some(al.to_string());
            }
        }
    }

    let duration = get_audio_duration_via_ffprobe(path);

    Some(LocalSong {
        title,
        artist,
        album,
        duration,
        filepath,
    })
}

fn get_audio_duration_via_ffprobe(path: &Path) -> Option<String> {
    let out = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-show_entries")
        .arg("format=duration")
        .arg("-of")
        .arg("default=noprint_wrappers=1:nokey=1")
        .arg(path)
        .output()
        .ok()?;

    if out.status.success() {
        let secs_str = String::from_utf8_lossy(&out.stdout);
        if let Ok(secs) = secs_str.trim().parse::<f64>() {
            let m = (secs / 60.0) as i32;
            let s = (secs % 60.0) as i32;
            return Some(format!("{:02}:{:02}", m, s));
        }
    }
    None
}

fn load_local_cache(path: &Path) -> Result<Vec<LocalSong>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(path)?;
    let songs = serde_json::from_str(&content)?;
    Ok(songs)
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

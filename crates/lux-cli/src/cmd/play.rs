use crate::library::db::{SearchCacheEntry, add_to_history, get_song_by_cli_id};
use crate::player::MpvClient;
use crate::source::SourceManager;
use anyhow::{Result, anyhow};
use colored::Colorize;
use lux_core::types::Quality;
use std::fs;
use std::path::Path;

pub async fn run(
    id_or_urls: Vec<String>,
    quality_str: Option<String>,
    _from_playlist: Option<String>,
    _shuffle: bool,
    json: bool,
) -> Result<()> {
    if id_or_urls.is_empty() {
        return Err(anyhow!("No song ID, URL, or file path provided"));
    }

    let config = lux_core::config::Config::load().unwrap_or_default();
    let client = MpvClient::new();
    let first = &id_or_urls[0];

    // Check if it is a direct URL or local file path
    if first.starts_with("http://") || first.starts_with("https://") || Path::new(first).exists() {
        client.play_file_or_url(first)?;

        let filename = Path::new(first)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(first);

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
            extra: None,
        };

        save_currently_playing(&entry)?;

        if json {
            println!(
                "{}",
                serde_json::json!({
                    "status": "playing",
                    "file": first
                })
            );
        } else {
            println!(
                "{} Playing direct link/file: {}",
                "▶".green().bold(),
                first.cyan()
            );
        }
        return Ok(());
    }

    // Otherwise, treat as CLI ID from the database cache
    crate::library::db::init_db()?;
    let song = get_song_by_cli_id(first)?.ok_or_else(|| {
        anyhow!(
            "Song ID '{}' not found in search cache. Run 'rlx search' first.",
            first
        )
    })?;

    // Determine quality
    let quality = if let Some(ref q) = quality_str {
        q.parse::<Quality>()
            .map_err(|e| anyhow!("Invalid quality override: {}", e))?
    } else {
        config.source.default_quality
    };

    if !json {
        println!(
            "{} Resolving playable URL for '{}'...",
            "⚡".yellow().bold(),
            song.name.cyan()
        );
    }

    // Resolve URL using SourceManager
    let mgr = SourceManager::new();
    let resolved_url = mgr.resolve_url(&song.source, &song.song_id, quality)?;

    // Play in mpv
    client.play_file_or_url(&resolved_url)?;

    // Save currently playing info
    save_currently_playing(&song)?;

    // Add to history
    let _ = add_to_history(&song, None);

    if json {
        println!(
            "{}",
            serde_json::json!({
                "status": "playing",
                "id": song.cli_id,
                "name": song.name,
                "singer": song.singer,
                "source": song.source,
                "url": resolved_url
            })
        );
    } else {
        println!(
            "\n{} Started playing: {} — {}",
            "▶".green().bold(),
            song.name.bold(),
            song.singer.cyan()
        );
        if let Some(ref album) = song.album_name {
            println!("  Album:  {}", album);
        }
        println!(
            "  Source: {} | Quality: {}",
            song.source.green(),
            quality.to_string().yellow()
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

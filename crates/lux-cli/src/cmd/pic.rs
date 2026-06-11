use crate::library::db::{SearchCacheEntry, get_song_by_cli_id};
use anyhow::{Result, anyhow};
use colored::Colorize;
use std::fs;

pub async fn run(id: Option<String>, save: bool, output: Option<String>, json: bool) -> Result<()> {
    crate::library::db::init_db()?;

    let song = if let Some(ref target_id) = id {
        if let Some(s) = get_song_by_cli_id(target_id)? {
            s
        } else {
            // Also check if it matches a song_id directly in the search cache
            let conn = crate::library::db::get_db_conn()?;
            let mut stmt = conn.prepare(
                "SELECT cli_id, song_id, name, singer, source, interval, album_name,
                        album_id, pic_url, songmid, hash, extra FROM search_cache
                 WHERE song_id = ?1",
            )?;
            let mut rows = stmt.query([target_id])?;
            if let Some(row) = rows.next()? {
                SearchCacheEntry {
                    cli_id: row.get(0)?,
                    song_id: row.get(1)?,
                    name: row.get(2)?,
                    singer: row.get(3)?,
                    source: row.get(4)?,
                    interval: row.get(5)?,
                    album_name: row.get(6)?,
                    album_id: row.get(7)?,
                    pic_url: row.get(8)?,
                    songmid: row.get(9)?,
                    hash: row.get(10)?,
                    extra: row.get(11)?,
                }
            } else {
                if json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "error": format!("Song ID '{}' not found in cache. Run a search first.", target_id)
                        })
                    );
                    std::process::exit(1);
                } else {
                    return Err(anyhow!(
                        "Song ID '{}' not found in cache. Run a search first.",
                        target_id
                    ));
                }
            }
        }
    } else {
        // Load from current.json
        let paths = lux_core::config::resolve_paths();
        let current_json_path = paths.cache_dir.join("current.json");
        let song_opt: Option<SearchCacheEntry> = if current_json_path.exists() {
            if let Ok(content) = fs::read_to_string(&current_json_path) {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(song_val) = val.get("song") {
                        serde_json::from_value::<SearchCacheEntry>(song_val.clone()).ok()
                    } else {
                        serde_json::from_value::<SearchCacheEntry>(val).ok()
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let Some(s) = song_opt else {
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "error": "No active song is playing and no song ID was provided."
                    })
                );
                std::process::exit(1);
            } else {
                return Err(anyhow!(
                    "No active song is playing and no song ID was provided."
                ));
            }
        };
        s
    };

    let Some(ref url) = song.pic_url else {
        if json {
            println!(
                "{}",
                serde_json::json!({
                    "song_id": song.song_id,
                    "cli_id": song.cli_id,
                    "name": song.name,
                    "singer": song.singer,
                    "pic_url": serde_json::Value::Null
                })
            );
            return Ok(());
        } else {
            return Err(anyhow!("No cover art URL is available for this song."));
        }
    };

    if url.trim().is_empty() {
        if json {
            println!(
                "{}",
                serde_json::json!({
                    "song_id": song.song_id,
                    "cli_id": song.cli_id,
                    "name": song.name,
                    "singer": song.singer,
                    "pic_url": serde_json::Value::Null
                })
            );
            return Ok(());
        } else {
            return Err(anyhow!("Cover art URL is empty."));
        }
    }

    if save {
        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .build()?;

        let resp = client.get(url).send().await?;
        if !resp.status().is_success() {
            return Err(anyhow!(
                "Failed to download cover art: HTTP {}",
                resp.status()
            ));
        }

        let data = resp.bytes().await?;

        // Detect suffix via Magic Bytes
        let suffix = detect_image_suffix(&data);

        let target_path = if let Some(ref out_val) = output {
            let out_path = std::path::Path::new(out_val);
            if out_path.is_dir() || out_val.ends_with('/') {
                let _ = fs::create_dir_all(out_path);
                let clean_title = song.name.replace(['/', '\\'], "-");
                let clean_singer = song.singer.replace(['/', '\\'], "-");
                let filename = format!("{} - {}.{}", clean_singer, clean_title, suffix);
                out_path.join(filename)
            } else {
                if let Some(parent) = out_path.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                if out_path.extension().is_none() {
                    out_path.with_extension(suffix)
                } else {
                    out_path.to_path_buf()
                }
            }
        } else {
            let config = lux_core::config::Config::load().unwrap_or_default();
            let output_dir = config.get_resolved_download_dir();
            let _ = fs::create_dir_all(&output_dir);
            let clean_title = song.name.replace(['/', '\\'], "-");
            let clean_singer = song.singer.replace(['/', '\\'], "-");
            let filename = format!("{} - {}.{}", clean_singer, clean_title, suffix);
            output_dir.join(filename)
        };

        fs::write(&target_path, &data)?;

        if json {
            println!(
                "{}",
                serde_json::json!({
                    "song_id": song.song_id,
                    "cli_id": song.cli_id,
                    "name": song.name,
                    "singer": song.singer,
                    "pic_url": url,
                    "saved_to": target_path.to_string_lossy(),
                    "format": suffix
                })
            );
        } else {
            println!(
                "{} Downloaded and saved cover art to: {}",
                "✓".green().bold(),
                target_path.display().to_string().cyan()
            );
        }
    } else {
        if json {
            println!(
                "{}",
                serde_json::json!({
                    "song_id": song.song_id,
                    "cli_id": song.cli_id,
                    "name": song.name,
                    "singer": song.singer,
                    "pic_url": url
                })
            );
        } else {
            println!("{}", url);
        }
    }

    Ok(())
}

fn detect_image_suffix(data: &[u8]) -> &'static str {
    if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "jpg"
    } else if data.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        "png"
    } else if data.starts_with(&[0x52, 0x49, 0x46, 0x46])
        && data.len() >= 12
        && &data[8..12] == b"WEBP"
    {
        "webp"
    } else if data.starts_with(&[0x47, 0x49, 0x46, 0x38]) {
        "gif"
    } else {
        "jpg" // Default fallback
    }
}

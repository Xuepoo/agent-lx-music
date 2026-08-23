use crate::library::db::{SearchCacheEntry, get_song_by_cli_id};
use crate::source::SourceManager;
use anyhow::{Result, anyhow};
use colored::Colorize;
use std::fs;
use std::path::Path;

/// Pure overwrite policy: a save may proceed when the target does not
/// exist yet or an explicit `--force` was given.
fn should_write(path_exists: bool, force: bool) -> Result<()> {
    if !path_exists || force {
        Ok(())
    } else {
        Err(anyhow!("target lyrics file already exists"))
    }
}

/// Guard against silently clobbering an existing lyrics file.
///
/// The refusal error names the offending path so users can identify it
/// without re-running with debug output.
fn ensure_writable(target: &Path, force: bool) -> Result<()> {
    should_write(target.exists(), force).map_err(|_| {
        anyhow!(
            "Refusing to overwrite existing file '{}' (use --force to overwrite)",
            target.display()
        )
    })
}

pub async fn run(
    id: Option<String>,
    translated: bool,
    romanized: bool,
    save: bool,
    force: bool,
    json: bool,
) -> Result<()> {
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

    let mgr = SourceManager::new();
    let lyric_info = mgr.resolve_lyric(&song.source, &song.song_id)?;

    // Select which track to output
    let (track_name, track_content) = if translated {
        ("translated", lyric_info.tlyric.as_deref())
    } else if romanized {
        ("romanized", lyric_info.rlyric.as_deref())
    } else {
        ("main", Some(lyric_info.lyric.as_str()))
    };

    let content_to_print = match track_content {
        Some(content) if !content.trim().is_empty() => content,
        _ => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "song_id": song.song_id,
                        "cli_id": song.cli_id,
                        "name": song.name,
                        "singer": song.singer,
                        "track": track_name,
                        "lyric": serde_json::Value::Null
                    })
                );
                return Ok(());
            } else {
                if translated {
                    return Err(anyhow!("No translated lyrics available for this song."));
                } else if romanized {
                    return Err(anyhow!("No romanized lyrics available for this song."));
                } else {
                    return Err(anyhow!("Lyrics are empty or not available."));
                }
            }
        }
    };

    if save {
        let config = lux_core::config::Config::load().unwrap_or_default();
        let output_dir = config.get_resolved_download_dir();
        let _ = fs::create_dir_all(&output_dir);

        let clean_title = song.name.replace(['/', '\\'], "-");
        let clean_singer = song.singer.replace(['/', '\\'], "-");
        let final_name = config
            .download
            .filename_template
            .replace("{singer}", &clean_singer)
            .replace("{title}", &clean_title);

        let suffix = if translated {
            ".trans"
        } else if romanized {
            ".roma"
        } else {
            ""
        };
        let final_filename = format!("{}{}.lrc", final_name, suffix);
        let final_path = output_dir.join(&final_filename);

        ensure_writable(&final_path, force)?;

        fs::write(&final_path, content_to_print)?;

        if json {
            println!(
                "{}",
                serde_json::json!({
                    "song_id": song.song_id,
                    "cli_id": song.cli_id,
                    "name": song.name,
                    "singer": song.singer,
                    "track": track_name,
                    "saved_to": final_path.to_string_lossy()
                })
            );
        } else {
            println!(
                "{} Saved {} lyrics to: {}",
                "✓".green().bold(),
                track_name,
                final_path.display().to_string().cyan()
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
                    "track": track_name,
                    "lyric": content_to_print
                })
            );
        } else {
            println!("{}", content_to_print);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ensure_writable, should_write};
    use std::fs;
    use std::path::PathBuf;

    fn temp_test_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!(
            "alx-lyric-test-{}-{}-{}",
            tag,
            std::process::id(),
            nanos
        ))
    }

    #[test]
    fn should_write_allows_new_target() {
        assert!(should_write(false, false).is_ok());
    }

    #[test]
    fn should_write_only_allows_existing_target_when_forced() {
        assert!(should_write(true, false).is_err());
        assert!(should_write(true, true).is_ok());
        assert!(should_write(false, true).is_ok());
    }

    #[test]
    fn save_refuses_existing_lrc_without_force_and_keeps_content() {
        let dir = temp_test_dir("save-guard");
        fs::create_dir_all(&dir).unwrap();
        let target = dir.join("Artist - Song.lrc");
        fs::write(&target, "[00:01.00]old").unwrap();

        let err = ensure_writable(&target, false).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Refusing to overwrite"), "message: {msg}");
        assert!(
            msg.contains(target.to_string_lossy().as_ref()),
            "message must name the file: {msg}"
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), "[00:01.00]old");

        // Explicit --force unlocks the write.
        assert!(ensure_writable(&target, true).is_ok());

        fs::remove_dir_all(&dir).ok();
    }
}

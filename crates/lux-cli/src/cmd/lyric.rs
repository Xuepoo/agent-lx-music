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

/// Cache-miss error for an explicit song ID lookup. Rendered as
/// `{"error": ...}` on stderr by main when `--json` is active.
fn missing_song_error(cli_id: &str) -> anyhow::Error {
    anyhow!(
        "Song ID '{}' not found in cache. Run a search first.",
        cli_id
    )
}

/// Error for the implicit "currently playing song" lookup finding nothing.
fn no_active_song_error() -> anyhow::Error {
    anyhow!("No active song is playing and no song ID was provided.")
}

/// Shared error for a resolved song whose selected lyric track is missing.
pub fn missing_track_error(track: &str) -> anyhow::Error {
    match track {
        "translated" => anyhow!("No translated lyrics available for this song."),
        "romanized" => anyhow!("No romanized lyrics available for this song."),
        _ => anyhow!("Lyrics are empty or not available."),
    }
}

/// A fully resolved lyric fetch: the matched song, which track was
/// selected, and its content (`None` when the track is empty/missing).
pub struct LyricFetch {
    pub song: SearchCacheEntry,
    pub track: &'static str,
    pub content: Option<String>,
}

/// Resolve a song by explicit ID (CLI ID or platform song ID) or from the
/// currently-playing state, then fetch and select the requested lyric
/// track. Shared by `alx lyric` and the MCP `lyric_get` tool; presentation
/// and file-saving stay with the callers.
///
/// Time/space complexity: O(1) DB lookups + one network resolve / O(n) for
/// n lyric bytes.
pub fn fetch_lyric(id: Option<&str>, translated: bool, romanized: bool) -> Result<LyricFetch> {
    crate::library::db::init_db()?;

    let song = if let Some(target_id) = id {
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
                return Err(missing_song_error(target_id));
            }
        }
    } else {
        // Load from current.json
        let paths = lux_core::config::resolve_paths();
        let Some(s) = read_current_song(&paths.cache_dir) else {
            return Err(no_active_song_error());
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

    let content = match track_content {
        Some(content) if !content.trim().is_empty() => Some(content.to_string()),
        _ => None,
    };

    Ok(LyricFetch {
        song,
        track: track_name,
        content,
    })
}

/// Load the active song from `<cache_dir>/current.json`, if any.
///
/// Accepts either a bare entry object or a `{"song": {...}}` wrapper;
/// missing, unreadable, or malformed files yield `None`. Shared by the
/// lyric, status, and MCP layers.
pub(crate) fn read_current_song(cache_dir: &Path) -> Option<SearchCacheEntry> {
    let content = fs::read_to_string(cache_dir.join("current.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&content).ok()?;
    let song_val = value.get("song").unwrap_or(&value);
    serde_json::from_value::<SearchCacheEntry>(song_val.clone()).ok()
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
    let fetched = fetch_lyric(id.as_deref(), translated, romanized)?;
    let song = fetched.song;
    let track_name = fetched.track;

    let content_to_print = match fetched.content {
        Some(content) => content,
        None => {
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
                return Err(missing_track_error(track_name));
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

#[cfg(test)]
mod contract_tests {
    use super::{missing_song_error, no_active_song_error, read_current_song};
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
    fn missing_song_error_names_the_id_and_next_step() {
        let err = missing_song_error("abc123");
        let msg = err.to_string();
        assert!(msg.contains("abc123"), "message: {msg}");
        assert!(msg.contains("not found in cache"), "message: {msg}");
        assert!(msg.contains("Run a search first"), "message: {msg}");
    }

    #[test]
    fn no_active_song_error_has_stable_message() {
        assert_eq!(
            no_active_song_error().to_string(),
            "No active song is playing and no song ID was provided."
        );
    }

    #[test]
    fn read_current_song_returns_none_for_missing_or_invalid_file() {
        let dir = temp_test_dir("current-missing");
        fs::create_dir_all(&dir).unwrap();

        assert!(read_current_song(&dir).is_none());

        fs::write(dir.join("current.json"), "not-json{").unwrap();
        assert!(read_current_song(&dir).is_none());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_current_song_parses_bare_entry_and_song_wrapper() {
        let dir = temp_test_dir("current-parse");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("current.json");

        let bare = r#"{
            "cli_id": "c1", "song_id": "s1", "name": "Song", "singer": "Artist",
            "source": "kw", "interval": "04:12"
        }"#;
        fs::write(&path, bare).unwrap();
        let entry = read_current_song(&dir).expect("bare entry must parse");
        assert_eq!(entry.cli_id, "c1");
        assert_eq!(entry.song_id, "s1");
        assert_eq!(entry.source, "kw");

        fs::write(
            &path,
            r#"{"song": {"cli_id": "c2", "song_id": "s2", "name": "N",
                         "singer": "S", "source": "tx"}}"#,
        )
        .unwrap();
        let wrapped = read_current_song(&dir).expect("wrapped entry must parse");
        assert_eq!(wrapped.cli_id, "c2");
        assert_eq!(wrapped.song_id, "s2");

        fs::remove_dir_all(&dir).ok();
    }
}

use anyhow::Result;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceDbEntry {
    pub id: String,
    pub name: String,
    pub version: Option<String>,
    pub author: Option<String>,
    pub homepage: Option<String>,
    pub repository: Option<String>,
    pub script_path: String,
    pub source_url: Option<String>,
    pub content_hash: String,
    pub platforms: String, // JSON: ["kw","wy"]
    pub qualities: String, // JSON: {"kw":["128k"],"wy":["flac"]}
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

pub fn get_db_conn() -> Result<Connection> {
    let paths = lux_core::config::resolve_paths();
    if let Some(parent) = paths.db_file.parent() {
        fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(&paths.db_file)?;
    Ok(conn)
}

pub fn init_db() -> Result<()> {
    let conn = get_db_conn()?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS sources (
            id          TEXT PRIMARY KEY,
            name        TEXT NOT NULL,
            version     TEXT,
            author      TEXT,
            homepage    TEXT,
            repository  TEXT,
            script_path TEXT NOT NULL,
            source_url  TEXT,
            content_hash TEXT NOT NULL,
            platforms   TEXT NOT NULL,
            qualities   TEXT NOT NULL,
            enabled     INTEGER NOT NULL DEFAULT 1,
            created_at  TEXT NOT NULL,
            updated_at  TEXT NOT NULL
        );",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS search_cache (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            cli_id      TEXT NOT NULL UNIQUE,
            song_id     TEXT NOT NULL,
            name        TEXT NOT NULL,
            singer      TEXT NOT NULL,
            source      TEXT NOT NULL,
            interval    TEXT,
            album_name  TEXT,
            album_id    TEXT,
            pic_url     TEXT,
            songmid     TEXT,
            hash        TEXT,
            extra       TEXT,
            cached_at   TEXT NOT NULL,
            UNIQUE(song_id, source)
        );",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_search_cache_cached_at ON search_cache(cached_at);",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS playlists (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            name        TEXT NOT NULL UNIQUE,
            description TEXT,
            song_count  INTEGER NOT NULL DEFAULT 0,
            created_at  TEXT NOT NULL,
            updated_at  TEXT NOT NULL
        );",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS playlist_songs (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
            song_id     TEXT NOT NULL,
            source      TEXT NOT NULL,
            name        TEXT NOT NULL,
            singer      TEXT NOT NULL,
            album_name  TEXT,
            interval    TEXT,
            pic_url     TEXT,
            position    INTEGER NOT NULL,
            added_at    TEXT NOT NULL
        );",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_playlist_songs_playlist ON playlist_songs(playlist_id, position);",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS play_history (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            song_id     TEXT NOT NULL,
            source      TEXT NOT NULL,
            name        TEXT NOT NULL,
            singer      TEXT NOT NULL,
            album_name  TEXT,
            interval    TEXT,
            pic_url     TEXT,
            duration_played INTEGER,
            played_at   TEXT NOT NULL
        );",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_play_history_played_at ON play_history(played_at DESC);",
        [],
    )?;

    // Insert "Favorites" reserved playlist
    let now = chrono::Local::now().to_rfc3339();
    let _ = conn.execute(
        "INSERT OR IGNORE INTO playlists (name, description, created_at, updated_at) VALUES (?1, ?2, ?3, ?4)",
        params!["Favorites", "Default Favorites Playlist", now, now],
    );

    Ok(())
}

pub fn insert_or_update_source(entry: &SourceDbEntry) -> Result<()> {
    let conn = get_db_conn()?;
    conn.execute(
        "INSERT OR REPLACE INTO sources (
            id, name, version, author, homepage, repository,
            script_path, source_url, content_hash, platforms, qualities,
            enabled, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            entry.id,
            entry.name,
            entry.version,
            entry.author,
            entry.homepage,
            entry.repository,
            entry.script_path,
            entry.source_url,
            entry.content_hash,
            entry.platforms,
            entry.qualities,
            if entry.enabled { 1 } else { 0 },
            entry.created_at,
            entry.updated_at,
        ],
    )?;
    Ok(())
}

pub fn list_sources() -> Result<Vec<SourceDbEntry>> {
    let conn = get_db_conn()?;
    let mut stmt = conn.prepare(
        "SELECT id, name, version, author, homepage, repository,
                script_path, source_url, content_hash, platforms, qualities,
                enabled, created_at, updated_at FROM sources",
    )?;
    let entries = stmt.query_map([], |row| {
        let enabled_val: i32 = row.get(11)?;
        Ok(SourceDbEntry {
            id: row.get(0)?,
            name: row.get(1)?,
            version: row.get(2)?,
            author: row.get(3)?,
            homepage: row.get(4)?,
            repository: row.get(5)?,
            script_path: row.get(6)?,
            source_url: row.get(7)?,
            content_hash: row.get(8)?,
            platforms: row.get(9)?,
            qualities: row.get(10)?,
            enabled: enabled_val != 0,
            created_at: row.get(12)?,
            updated_at: row.get(13)?,
        })
    })?;

    let mut result = Vec::new();
    for entry in entries {
        result.push(entry?);
    }
    Ok(result)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchCacheEntry {
    pub cli_id: String,
    pub song_id: String,
    pub name: String,
    pub singer: String,
    pub source: String,
    pub interval: Option<String>,
    pub album_name: Option<String>,
    pub album_id: Option<String>,
    pub pic_url: Option<String>,
    pub songmid: Option<String>,
    pub hash: Option<String>,
    pub extra: Option<String>,
}

pub fn insert_search_cache(entry: &SearchCacheEntry) -> Result<()> {
    let conn = get_db_conn()?;
    let now = chrono::Local::now().to_rfc3339();
    conn.execute(
        "INSERT OR REPLACE INTO search_cache (
            cli_id, song_id, name, singer, source, interval, album_name,
            album_id, pic_url, songmid, hash, extra, cached_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            entry.cli_id,
            entry.song_id,
            entry.name,
            entry.singer,
            entry.source,
            entry.interval,
            entry.album_name,
            entry.album_id,
            entry.pic_url,
            entry.songmid,
            entry.hash,
            entry.extra,
            now,
        ],
    )?;
    Ok(())
}

pub fn get_song_from_cache(song_id: &str, source: &str) -> Result<Option<SearchCacheEntry>> {
    let conn = get_db_conn()?;
    let mut stmt = conn.prepare(
        "SELECT cli_id, song_id, name, singer, source, interval, album_name,
                album_id, pic_url, songmid, hash, extra FROM search_cache
         WHERE song_id = ?1 AND source = ?2",
    )?;
    let mut rows = stmt.query(params![song_id, source])?;
    if let Some(row) = rows.next()? {
        Ok(Some(SearchCacheEntry {
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
        }))
    } else {
        Ok(None)
    }
}

pub fn get_song_by_cli_id(cli_id: &str) -> Result<Option<SearchCacheEntry>> {
    let conn = get_db_conn()?;
    let mut stmt = conn.prepare(
        "SELECT cli_id, song_id, name, singer, source, interval, album_name,
                album_id, pic_url, songmid, hash, extra FROM search_cache
         WHERE cli_id LIKE ?1",
    )?;
    let mut rows = stmt.query(params![format!("{}%", cli_id)])?;
    if let Some(row) = rows.next()? {
        Ok(Some(SearchCacheEntry {
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
        }))
    } else {
        Ok(None)
    }
}

pub fn clear_search_cache() -> Result<()> {
    let conn = get_db_conn()?;
    conn.execute("DELETE FROM search_cache", [])?;
    Ok(())
}

pub fn add_to_history(entry: &SearchCacheEntry, duration_played: Option<i32>) -> Result<()> {
    let conn = get_db_conn()?;
    let now = chrono::Local::now().to_rfc3339();
    conn.execute(
        "INSERT INTO play_history (
            song_id, source, name, singer, album_name, interval, pic_url, duration_played, played_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            entry.song_id,
            entry.source,
            entry.name,
            entry.singer,
            entry.album_name,
            entry.interval,
            entry.pic_url,
            duration_played,
            now,
        ],
    )?;
    Ok(())
}

pub fn get_history(limit: usize) -> Result<Vec<SearchCacheEntry>> {
    let conn = get_db_conn()?;
    let mut stmt = conn.prepare(
        "SELECT song_id, source, name, singer, album_name, interval, pic_url
         FROM play_history ORDER BY played_at DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit], |row| {
        Ok(SearchCacheEntry {
            cli_id: "".to_string(),
            song_id: row.get(0)?,
            source: row.get(1)?,
            name: row.get(2)?,
            singer: row.get(3)?,
            album_name: row.get(4)?,
            interval: row.get(5)?,
            pic_url: row.get(6)?,
            album_id: None,
            songmid: None,
            hash: None,
            extra: None,
        })
    })?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

pub fn create_playlist(name: &str, description: Option<&str>) -> Result<()> {
    let conn = get_db_conn()?;
    let now = chrono::Local::now().to_rfc3339();
    conn.execute(
        "INSERT OR IGNORE INTO playlists (name, description, created_at, updated_at) VALUES (?1, ?2, ?3, ?4)",
        params![name, description, now, now],
    )?;
    Ok(())
}

pub fn list_playlists() -> Result<Vec<(String, Option<String>, i32)>> {
    let conn = get_db_conn()?;
    let mut stmt = conn.prepare("SELECT name, description, song_count FROM playlists")?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, i32>(2)?,
        ))
    })?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

pub fn delete_playlist(name: &str) -> Result<()> {
    let conn = get_db_conn()?;
    conn.execute("DELETE FROM playlists WHERE name = ?1", params![name])?;
    Ok(())
}

pub fn add_to_playlist(playlist_name: &str, entry: &SearchCacheEntry) -> Result<()> {
    let conn = get_db_conn()?;
    let playlist_id: i32 = conn.query_row(
        "SELECT id FROM playlists WHERE name = ?1",
        params![playlist_name],
        |row| row.get(0),
    )?;

    let now = chrono::Local::now().to_rfc3339();
    let max_pos: Option<i32> = conn
        .query_row(
            "SELECT MAX(position) FROM playlist_songs WHERE playlist_id = ?1",
            params![playlist_id],
            |row| row.get(0),
        )
        .ok();
    let position = max_pos.unwrap_or(0) + 1;

    conn.execute(
        "INSERT INTO playlist_songs (
            playlist_id, song_id, source, name, singer, album_name, interval, pic_url, position, added_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            playlist_id,
            entry.song_id,
            entry.source,
            entry.name,
            entry.singer,
            entry.album_name,
            entry.interval,
            entry.pic_url,
            position,
            now,
        ],
    )?;

    // Update song count
    conn.execute(
        "UPDATE playlists SET song_count = song_count + 1 WHERE id = ?1",
        params![playlist_id],
    )?;

    Ok(())
}

pub fn get_playlist_songs(playlist_name: &str) -> Result<Vec<SearchCacheEntry>> {
    let conn = get_db_conn()?;
    let playlist_id: i32 = conn.query_row(
        "SELECT id FROM playlists WHERE name = ?1",
        params![playlist_name],
        |row| row.get(0),
    )?;

    let mut stmt = conn.prepare(
        "SELECT song_id, source, name, singer, album_name, interval, pic_url
         FROM playlist_songs WHERE playlist_id = ?1 ORDER BY position ASC",
    )?;
    let rows = stmt.query_map(params![playlist_id], |row| {
        Ok(SearchCacheEntry {
            cli_id: "".to_string(),
            song_id: row.get(0)?,
            source: row.get(1)?,
            name: row.get(2)?,
            singer: row.get(3)?,
            album_name: row.get(4)?,
            interval: row.get(5)?,
            pic_url: row.get(6)?,
            album_id: None,
            songmid: None,
            hash: None,
            extra: None,
        })
    })?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_search_cache_operations() {
        let temp_dir = env::temp_dir().join("rust-lx-test-db-ops");
        if temp_dir.exists() {
            let _ = std::fs::remove_dir_all(&temp_dir);
        }
        unsafe {
            env::set_var("RUST_LX_HOME", temp_dir.to_str().unwrap());
        }

        assert!(init_db().is_ok());

        let entry = SearchCacheEntry {
            cli_id: "testcli1".to_string(),
            song_id: "testsong1".to_string(),
            name: "Test Song".to_string(),
            singer: "Test Singer".to_string(),
            source: "wy".to_string(),
            interval: Some("03:30".to_string()),
            album_name: Some("Test Album".to_string()),
            album_id: Some("123".to_string()),
            pic_url: Some("http://pic.com".to_string()),
            songmid: Some("testsong1".to_string()),
            hash: None,
            extra: None,
        };

        // Test insertion
        assert!(insert_search_cache(&entry).is_ok());

        // Test retrieval by song_id + source
        let retrieved = get_song_from_cache("testsong1", "wy").unwrap();
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.name, "Test Song");
        assert_eq!(retrieved.cli_id, "testcli1");

        // Test retrieval by cli_id prefix
        let retrieved_by_cli = get_song_by_cli_id("testc").unwrap();
        assert!(retrieved_by_cli.is_some());
        assert_eq!(retrieved_by_cli.unwrap().name, "Test Song");

        // Test play history
        assert!(add_to_history(&entry, Some(10)).is_ok());
        let hist = get_history(5).unwrap();
        assert_eq!(hist.len(), 1);
        assert_eq!(hist[0].name, "Test Song");

        // Test playlists
        assert!(create_playlist("My List", Some("Desc")).is_ok());
        assert!(add_to_playlist("My List", &entry).is_ok());
        let songs = get_playlist_songs("My List").unwrap();
        assert_eq!(songs.len(), 1);
        assert_eq!(songs[0].name, "Test Song");

        // Cleanup
        let _ = std::fs::remove_dir_all(&temp_dir);
        unsafe {
            env::remove_var("RUST_LX_HOME");
        }
    }
}

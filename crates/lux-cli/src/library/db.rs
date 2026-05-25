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

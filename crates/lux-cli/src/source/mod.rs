#![allow(clippy::collapsible_if)]
pub mod bridge;
pub mod loader;
pub mod runtime;

use anyhow::{Result, anyhow};
use lux_core::types::Quality;
#[cfg(feature = "lux-native")]
use lux_core::types::Source;

pub struct SourceManager;

impl Default for SourceManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SourceManager {
    pub fn new() -> Self {
        SourceManager
    }

    pub fn resolve_url(&self, platform: &str, song_id: &str, quality: Quality) -> Result<String> {
        // 1. Fetch installed JS sources from local database
        let db_entries = crate::library::db::list_sources().unwrap_or_default();

        // 2. Query enabled JS sources in order
        for entry in db_entries {
            if !entry.enabled {
                continue;
            }

            // Quick static check if the platform is supported by this JS script
            let platforms: Vec<String> = serde_json::from_str(&entry.platforms).unwrap_or_default();
            if !platforms.contains(&platform.to_string()) {
                continue;
            }

            // Load and execute in sandbox
            let Ok(script) = std::fs::read_to_string(&entry.script_path) else {
                continue;
            };
            if let Ok(sandbox) = runtime::JsSandbox::new() {
                // Populate basic song info
                let music_info = serde_json::json!({
                    "songmid": song_id,
                    "name": "unknown",
                    "singer": "unknown",
                    "hash": if platform == "kg" { Some(song_id) } else { None }
                });

                if let Ok(url) = sandbox.execute_resolve(
                    &script,
                    platform,
                    song_id,
                    quality.as_str(),
                    music_info,
                ) {
                    return Ok(url);
                }
            }
        }

        // 3. Fallback to native Rust parsers if compiled in
        #[cfg(feature = "lux-native")]
        {
            let src_enum = Source::from(platform);
            if let Some(native_src) = lux_native::get_native_source(&src_enum) {
                let result = tokio::task::block_in_place(|| {
                    let rt = tokio::runtime::Handle::current();
                    rt.block_on(async { native_src.get_url(song_id, quality).await })
                });
                if let Ok(url) = result {
                    return Ok(url);
                }
            }
        }

        Err(anyhow!(
            "Failed to resolve playable URL for song '{}' on platform '{}' (All sources failed)",
            song_id,
            platform
        ))
    }

    pub fn resolve_lyric(
        &self,
        platform: &str,
        song_id: &str,
    ) -> Result<lux_core::traits::LyricInfo> {
        // 0. Check local SQLite cache first
        if let Ok(Some(cached)) = crate::library::db::get_cached_lyrics(song_id, platform) {
            return Ok(cached);
        }

        // 1. Fetch installed JS sources from local database
        let db_entries = crate::library::db::list_sources().unwrap_or_default();

        // 2. Query enabled JS sources in order
        for entry in db_entries {
            if !entry.enabled {
                continue;
            }

            let platforms: Vec<String> = serde_json::from_str(&entry.platforms).unwrap_or_default();
            if !platforms.contains(&platform.to_string()) {
                continue;
            }

            let Ok(script) = std::fs::read_to_string(&entry.script_path) else {
                continue;
            };
            if let Ok(sandbox) = runtime::JsSandbox::new() {
                let music_info = serde_json::json!({
                    "songmid": song_id,
                    "name": "unknown",
                    "singer": "unknown",
                    "hash": if platform == "kg" { Some(song_id) } else { None }
                });

                if let Ok(lyric_str) = sandbox.execute_lyric(&script, platform, song_id, music_info)
                {
                    if let Ok(js_lyric) = serde_json::from_str::<serde_json::Value>(&lyric_str) {
                        let lyric = js_lyric
                            .get("lyric")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| lyric_str.clone());
                        let tlyric = js_lyric
                            .get("tlyric")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        let rlyric = js_lyric
                            .get("rlyric")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        let lxlyric = js_lyric
                            .get("lxlyric")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());

                        let info = lux_core::traits::LyricInfo {
                            lyric,
                            tlyric,
                            rlyric,
                            lxlyric,
                        };
                        let _ = crate::library::db::insert_lyrics_cache(song_id, platform, &info);
                        return Ok(info);
                    } else {
                        let info = lux_core::traits::LyricInfo {
                            lyric: lyric_str,
                            tlyric: None,
                            rlyric: None,
                            lxlyric: None,
                        };
                        let _ = crate::library::db::insert_lyrics_cache(song_id, platform, &info);
                        return Ok(info);
                    }
                }
            }
        }

        // 3. Fallback to native Rust parsers if compiled in
        #[cfg(feature = "lux-native")]
        {
            let src_enum = Source::from(platform);
            if let Some(native_src) = lux_native::get_native_source(&src_enum) {
                let result = tokio::task::block_in_place(|| {
                    let rt = tokio::runtime::Handle::current();
                    rt.block_on(async { native_src.get_lyric(song_id).await })
                });
                if let Ok(Some(info)) = result {
                    let _ = crate::library::db::insert_lyrics_cache(song_id, platform, &info);
                    return Ok(info);
                }
            }
        }

        Err(anyhow!(
            "Failed to resolve lyric for song '{}' on platform '{}' (All sources failed)",
            song_id,
            platform
        ))
    }
}

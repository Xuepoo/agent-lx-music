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
        let js_priority = lux_core::config::Config::load()
            .map(|c| c.source.js_priority)
            .unwrap_or(true);

        lux_core::log_verbose!(
            "Resolving URL for song_id: '{}', platform: '{}', quality: '{}', js_priority: {}",
            song_id,
            platform,
            quality.as_str(),
            js_priority
        );

        let validate_url = |url: &str| -> bool {
            let url_trimmed = url.trim();
            if url_trimmed.is_empty() {
                return false;
            }
            if !url_trimmed.starts_with("http://") && !url_trimmed.starts_with("https://") {
                return false;
            }
            if url_trimmed.contains("horse.mp3") || url_trimmed.contains("example.com") {
                return false;
            }
            true
        };

        let query_js = || -> Option<String> {
            lux_core::log_verbose!(
                "Attempting URL resolution via JS dynamic sources for song_id: '{}', platform: '{}'",
                song_id,
                platform
            );
            let config = lux_core::config::Config::load().unwrap_or_default();
            let priority_list = config.source.priority;

            let mut db_entries = crate::library::db::list_sources().unwrap_or_default();
            db_entries.sort_by_key(|entry| {
                priority_list
                    .iter()
                    .position(|p| p == &entry.id)
                    .unwrap_or(usize::MAX)
            });

            for entry in db_entries {
                if !entry.enabled {
                    continue;
                }

                // Circuit Breaker check
                if let Ok(Some(broken_until)) =
                    crate::library::db::get_source_circuit_broken_until(&entry.id)
                {
                    lux_core::log_verbose!(
                        "Source '{}' (id: {}) is currently circuit-broken until {}. Skipping.",
                        entry.name,
                        entry.id,
                        broken_until
                    );
                    continue;
                }

                let platforms: Vec<String> =
                    serde_json::from_str(&entry.platforms).unwrap_or_default();
                if !platforms.contains(&platform.to_string()) {
                    continue;
                }

                lux_core::log_verbose!(
                    "Executing JS resolver in '{}' (path: {})",
                    entry.name,
                    entry.script_path
                );
                let Ok(script) = std::fs::read_to_string(&entry.script_path) else {
                    let _ = crate::library::db::record_source_fail(&entry.id);
                    continue;
                };
                if let Ok(sandbox) = runtime::JsSandbox::new() {
                    let music_info = serde_json::json!({
                        "songmid": song_id,
                        "name": "unknown",
                        "singer": "unknown",
                        "hash": if platform == "kg" { Some(song_id) } else { None }
                    });

                    match sandbox.execute_resolve(
                        &script,
                        platform,
                        song_id,
                        quality.as_str(),
                        music_info,
                    ) {
                        Ok(url) => {
                            if validate_url(&url) {
                                let _ = crate::library::db::record_source_success(&entry.id);
                                lux_core::log_verbose!(
                                    "JS resolver in '{}' successfully resolved valid URL: {}",
                                    entry.name,
                                    url
                                );
                                return Some(url);
                            } else {
                                let _ = crate::library::db::record_source_fail(&entry.id);
                                lux_core::log_verbose!(
                                    "JS resolver in '{}' returned invalid/blocked URL: {}",
                                    entry.name,
                                    url
                                );
                            }
                        }
                        Err(e) => {
                            let _ = crate::library::db::record_source_fail(&entry.id);
                            lux_core::log_verbose!(
                                "JS resolver in '{}' execution failed: {:?}",
                                entry.name,
                                e
                            );
                        }
                    }
                } else {
                    let _ = crate::library::db::record_source_fail(&entry.id);
                }
            }
            lux_core::log_verbose!(
                "JS dynamic sources failed to resolve URL for song_id: '{}'",
                song_id
            );
            None
        };

        let query_native = || -> Option<String> {
            lux_core::log_verbose!(
                "Attempting URL resolution via Native sources for song_id: '{}', platform: '{}'",
                song_id,
                platform
            );
            #[cfg(feature = "lux-native")]
            {
                let src_enum = Source::from(platform);
                if let Some(native_src) = lux_native::get_native_source(&src_enum) {
                    lux_core::log_verbose!("Calling native get_url for platform '{}'", platform);
                    let result = tokio::task::block_in_place(|| {
                        let rt = tokio::runtime::Handle::current();
                        rt.block_on(async { native_src.get_url(song_id, quality).await })
                    });
                    match result {
                        Ok(url) => {
                            if validate_url(&url) {
                                lux_core::log_verbose!(
                                    "Native platform '{}' successfully resolved URL: {}",
                                    platform,
                                    url
                                );
                                return Some(url);
                            } else {
                                lux_core::log_verbose!(
                                    "Native platform '{}' returned invalid URL: {}",
                                    platform,
                                    url
                                );
                            }
                        }
                        Err(e) => {
                            lux_core::log_verbose!(
                                "Native platform '{}' failed to resolve URL: {:?}",
                                platform,
                                e
                            );
                        }
                    }
                } else {
                    lux_core::log_verbose!(
                        "No native source compiled/available for platform '{}'",
                        platform
                    );
                }
            }
            #[cfg(not(feature = "lux-native"))]
            lux_core::log_verbose!("Native sources not compiled in this build");
            None
        };

        if js_priority {
            if let Some(url) = query_js() {
                return Ok(url);
            }
            if let Some(url) = query_native() {
                return Ok(url);
            }
        } else {
            if let Some(url) = query_native() {
                return Ok(url);
            }
            if let Some(url) = query_js() {
                return Ok(url);
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

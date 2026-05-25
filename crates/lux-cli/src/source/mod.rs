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
}

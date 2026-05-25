use crate::error::{LuxError, Result};
use crate::types::Quality;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceSettings {
    pub default_source: String,
    pub default_quality: Quality,
    pub quality_fallback: Vec<Quality>,
    pub js_priority: bool,
    pub priority: Vec<String>,
}

impl Default for SourceSettings {
    fn default() -> Self {
        Self {
            default_source: "all".to_string(),
            default_quality: Quality::Q320k,
            quality_fallback: vec![Quality::Q320k, Quality::Q128k, Quality::Flac],
            js_priority: true,
            priority: vec![
                "sixyin_v1.2.1".to_string(),
                "ikun_v22".to_string(),
                "huibq_v1.2.0".to_string(),
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceOverride {
    pub enabled: bool,
    pub quality: Option<Quality>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerSettings {
    pub default_volume: u8,
    pub repeat: String, // "off", "one", "all"
    pub shuffle: bool,
    pub mpv_args: Vec<String>,
    pub mpv_socket: Option<String>,
    pub enable_mpris: bool,
    pub auto_resume: bool,
}

impl Default for PlayerSettings {
    fn default() -> Self {
        Self {
            default_volume: 80,
            repeat: "off".to_string(),
            shuffle: false,
            mpv_args: vec![],
            mpv_socket: None,
            enable_mpris: true,
            auto_resume: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadSettings {
    pub output_dir: String,
    pub filename_template: String,
    pub embed_metadata: bool,
    pub embed_lyrics: bool,
    pub embed_lyrics_lx: bool,
    pub embed_lyrics_translated: bool,
    pub embed_lyrics_romanized: bool,
    pub embed_cover: bool,
    pub save_lyrics_file: bool,
    pub save_cover_file: bool,
    pub lrc_encoding: String,
    pub max_concurrent: usize,
    pub skip_existing: bool,
    pub use_other_source: bool,
    pub group_by_source: bool,
    pub timeout: u64,
    pub beet_import: bool,
    pub use_beets_library: bool,
}

impl Default for DownloadSettings {
    fn default() -> Self {
        Self {
            output_dir: "~/Music/rust-lx".to_string(),
            filename_template: "{singer} - {title}".to_string(),
            embed_metadata: true,
            embed_lyrics: true,
            embed_lyrics_lx: true,
            embed_lyrics_translated: false,
            embed_lyrics_romanized: false,
            embed_cover: true,
            save_lyrics_file: false,
            save_cover_file: false,
            lrc_encoding: "utf8".to_string(),
            max_concurrent: 3,
            skip_existing: true,
            use_other_source: true,
            group_by_source: false,
            timeout: 60,
            beet_import: false,
            use_beets_library: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistorySettings {
    pub max_age_days: u64,
}

impl Default for HistorySettings {
    fn default() -> Self {
        Self { max_age_days: 90 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplaySettings {
    pub color: String,       // "auto", "always", "never"
    pub table_style: String, // "rounded", "ascii", "compact"
    pub show_progress: bool,
}

impl Default for DisplaySettings {
    fn default() -> Self {
        Self {
            color: "auto".to_string(),
            table_style: "rounded".to_string(),
            show_progress: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkSettings {
    pub proxy: Option<String>,
    pub timeout: u64,
    pub max_retries: usize,
}

impl Default for NetworkSettings {
    fn default() -> Self {
        Self {
            proxy: None,
            timeout: 15,
            max_retries: 2,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    pub source: SourceSettings,
    #[serde(default)]
    pub sources: HashMap<String, SourceOverride>,
    #[serde(default)]
    pub player: PlayerSettings,
    #[serde(default)]
    pub download: DownloadSettings,
    pub history: HistorySettings,
    pub display: DisplaySettings,
    pub network: NetworkSettings,
}

#[derive(Debug, Clone)]
pub struct XdgPaths {
    pub config_file: PathBuf,
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub sources_dir: PathBuf,
    pub db_file: PathBuf,
}

pub fn resolve_paths() -> XdgPaths {
    let home = std::env::var("RUST_LX_HOME").ok().map(PathBuf::from);

    let config_file = if let Some(ref h) = home {
        h.join("config.toml")
    } else if let Ok(c) = std::env::var("RUST_LX_CONFIG") {
        PathBuf::from(c)
    } else {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("/home/fuyu/.config"))
            .join("rust-lx/config.toml")
    };

    let data_dir = if let Some(ref h) = home {
        h.join("data")
    } else if let Ok(d) = std::env::var("RUST_LX_DATA") {
        PathBuf::from(d)
    } else {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("/home/fuyu/.local/share"))
            .join("rust-lx")
    };

    let cache_dir = if let Some(ref h) = home {
        h.join("cache")
    } else if let Ok(c) = std::env::var("RUST_LX_CACHE") {
        PathBuf::from(c)
    } else {
        dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("/home/fuyu/.cache"))
            .join("rust-lx")
    };

    XdgPaths {
        config_file,
        sources_dir: data_dir.join("sources"),
        db_file: data_dir.join("rust-lx.db"),
        data_dir,
        cache_dir,
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let paths = resolve_paths();
        if !paths.config_file.exists() {
            Self::init_default(&paths)?;
        }

        let content = fs::read_to_string(&paths.config_file)
            .map_err(|e| LuxError::Config(format!("Failed to read config file: {}", e)))?;

        let config: Config = toml::from_str(&content)
            .map_err(|e| LuxError::Config(format!("Failed to parse TOML config: {}", e)))?;

        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        let paths = resolve_paths();
        if let Some(parent) = paths.config_file.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| LuxError::Io(format!("Failed to create config dir: {}", e)))?;
        }

        let content = toml::to_string_pretty(self)
            .map_err(|e| LuxError::Config(format!("Failed to serialize TOML config: {}", e)))?;

        fs::write(&paths.config_file, content)
            .map_err(|e| LuxError::Io(format!("Failed to write config file: {}", e)))?;

        Ok(())
    }

    fn init_default(paths: &XdgPaths) -> Result<()> {
        if let Some(parent) = paths.config_file.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| LuxError::Io(format!("Failed to create config dir: {}", e)))?;
        }
        fs::create_dir_all(&paths.sources_dir)
            .map_err(|e| LuxError::Io(format!("Failed to create sources dir: {}", e)))?;
        fs::create_dir_all(&paths.cache_dir)
            .map_err(|e| LuxError::Io(format!("Failed to create cache dir: {}", e)))?;

        let default_toml = get_default_config_toml();
        fs::write(&paths.config_file, default_toml)
            .map_err(|e| LuxError::Io(format!("Failed to write default config: {}", e)))?;

        Ok(())
    }

    pub fn get_resolved_download_dir(&self) -> PathBuf {
        let path_str = &self.download.output_dir;
        if let Some(stripped) = path_str.strip_prefix("~/")
            && let Some(home) = dirs::home_dir()
        {
            return home.join(stripped);
        }
        PathBuf::from(path_str)
    }
}

fn get_default_config_toml() -> &'static str {
    r#"# ~/.config/rust-lx/config.toml
# Default configuration for rust-lx (rlx)

[source]
# Default search source: "all" searches all sources in parallel
default_source = "all"

# Default quality (fallback order: try each until success)
# Valid: "128k", "192k", "320k", "flac", "flac24bit", "ape", "wav"
default_quality = "320k"

# Quality fallback chain (tried in order when default unavailable)
quality_fallback = ["320k", "128k", "flac"]

# Prefer JS sources over native parsers for same platform
js_priority = true

# Source priority list — controls search order and URL resolution fallback
# Sources not listed here get appended at the end in alphabetical order
priority = ["sixyin_v1.2.1", "ikun_v22", "huibq_v1.2.0"]

[sources]
# Source-specific overrides (optional)
# [sources.sixyin_v1.2.1]
# enabled = true
# quality = "flac"

[player]
default_volume = 80
repeat = "off" # "off", "one", "all"
shuffle = false
mpv_args = []
enable_mpris = true
auto_resume = true

[download]
output_dir = "~/Music/rust-lx"
filename_template = "{singer} - {title}"
embed_metadata = true
embed_lyrics = true
embed_lyrics_lx = true
embed_lyrics_translated = false
embed_lyrics_romanized = false
embed_cover = true
save_lyrics_file = false
save_cover_file = false
lrc_encoding = "utf8"
max_concurrent = 3
skip_existing = true
use_other_source = true
group_by_source = false
timeout = 60
beet_import = false
use_beets_library = false

[history]
max_age_days = 90

[display]
color = "auto" # "auto", "always", "never"
table_style = "rounded" # "rounded", "ascii", "compact"
show_progress = true

[network]
# proxy = "socks5://127.0.0.1:1080"
timeout = 15
max_retries = 2
"#
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_resolve_paths_with_home_override() {
        let temp_dir = env::temp_dir().join("rust-lx-test-home");
        unsafe {
            env::set_var("RUST_LX_HOME", temp_dir.to_str().unwrap());
        }

        let paths = resolve_paths();
        assert_eq!(paths.config_file, temp_dir.join("config.toml"));
        assert_eq!(paths.data_dir, temp_dir.join("data"));
        assert_eq!(paths.cache_dir, temp_dir.join("cache"));
        assert_eq!(paths.sources_dir, temp_dir.join("data").join("sources"));

        unsafe {
            env::remove_var("RUST_LX_HOME");
        }
    }

    #[test]
    fn test_config_default_load_save() {
        let temp_dir = env::temp_dir().join("rust-lx-test-default-load-save");
        if temp_dir.exists() {
            let _ = fs::remove_dir_all(&temp_dir);
        }
        unsafe {
            env::set_var("RUST_LX_HOME", temp_dir.to_str().unwrap());
        }

        // Loading should initialize the default config
        let config = Config::load();
        assert!(config.is_ok());

        let mut config = config.unwrap();
        assert_eq!(config.source.default_source, "all");
        assert_eq!(config.source.default_quality, Quality::Q320k);

        // Modify a setting and save
        config.player.default_volume = 90;
        assert!(config.save().is_ok());

        // Reload to verify it preserved the change
        let reloaded = Config::load().unwrap();
        assert_eq!(reloaded.player.default_volume, 90);

        let _ = fs::remove_dir_all(&temp_dir);
        unsafe {
            env::remove_var("RUST_LX_HOME");
        }
    }
}

use crate::error::{LuxError, Result};
use crate::types::Quality;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

fn default_platform_priority() -> Vec<String> {
    vec![
        "wy".to_string(),
        "kw".to_string(),
        "tx".to_string(),
        "mg".to_string(),
        "kg".to_string(),
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceSettings {
    pub default_source: String,
    pub default_quality: Quality,
    pub quality_fallback: Vec<Quality>,
    pub js_priority: bool,
    pub priority: Vec<String>,
    #[serde(default = "default_platform_priority")]
    pub platform_priority: Vec<String>,
}

impl Default for SourceSettings {
    fn default() -> Self {
        Self {
            default_source: "all".to_string(),
            default_quality: Quality::Q320k,
            quality_fallback: vec![Quality::Q320k, Quality::Q128k, Quality::Flac],
            js_priority: true,
            priority: vec!["_4".to_string(), "_9.393DeepSeek".to_string()],
            platform_priority: default_platform_priority(),
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
            output_dir: "~/Music/agent-lx-music".to_string(),
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
    let home = std::env::var("ALX_HOME").ok().map(PathBuf::from);

    let config_file = if let Some(ref h) = home {
        h.join("config.toml")
    } else if let Ok(c) = std::env::var("ALX_CONFIG") {
        PathBuf::from(c)
    } else {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("/home/fuyu/.config"))
            .join("agent-lx-music/config.toml")
    };

    let data_dir = if let Some(ref h) = home {
        h.join("data")
    } else if let Ok(d) = std::env::var("ALX_DATA") {
        PathBuf::from(d)
    } else {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("/home/fuyu/.local/share"))
            .join("agent-lx-music")
    };

    let cache_dir = if let Some(ref h) = home {
        h.join("cache")
    } else if let Ok(c) = std::env::var("ALX_CACHE") {
        PathBuf::from(c)
    } else {
        dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("/home/fuyu/.cache"))
            .join("agent-lx-music")
    };

    XdgPaths {
        config_file,
        sources_dir: data_dir.join("sources"),
        db_file: data_dir.join("agent-lx-music.db"),
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

        // DEF-015/#164: stage the new contents in the same directory as the
        // target (same filesystem, so rename is atomic), fsync, then swap it
        // in. A crash mid-write can no longer truncate or half-write
        // config.toml; staging leftovers are cleaned up on failure.
        let staging_path = staging_path_for(&paths.config_file);
        if let Err(e) = write_staging_file(&staging_path, content.as_bytes()) {
            let _ = fs::remove_file(&staging_path);
            return Err(e);
        }
        if let Err(e) = fs::rename(&staging_path, &paths.config_file) {
            let _ = fs::remove_file(&staging_path);
            return Err(LuxError::Io(format!(
                "Failed to replace config file: {}",
                e
            )));
        }

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
        expand_path(&self.download.output_dir)
    }
}

/// Staging file path for atomic config writes: sibling of the target so the
/// final rename never crosses a filesystem boundary.
fn staging_path_for(config_file: &std::path::Path) -> PathBuf {
    let mut name = config_file.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    config_file.with_file_name(name)
}

fn write_staging_file(staging_path: &PathBuf, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let mut file = fs::File::create(staging_path)
        .map_err(|e| LuxError::Io(format!("Failed to create staging file: {}", e)))?;
    file.write_all(bytes)
        .map_err(|e| LuxError::Io(format!("Failed to write staging file: {}", e)))?;
    file.sync_all()
        .map_err(|e| LuxError::Io(format!("Failed to sync staging file: {}", e)))?;
    Ok(())
}

pub fn expand_path(path_str: &str) -> PathBuf {
    let mut resolved = path_str.to_string();

    // 1. Handle tilde (~) expansion
    if resolved.starts_with("~/")
        && let Some(home) = dirs::home_dir()
    {
        resolved = resolved.replacen(
            "~/",
            &format!("{}/", home.to_string_lossy().trim_end_matches('/')),
            1,
        );
    } else if resolved == "~"
        && let Some(home) = dirs::home_dir()
    {
        resolved = home.to_string_lossy().to_string();
    }

    // 2. Expand environment variables ($VAR and ${VAR})
    let mut final_path = String::new();
    let chars: Vec<char> = resolved.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '$' && i + 1 < chars.len() {
            if chars[i + 1] == '{' {
                let mut j = i + 2;
                let mut var_name = String::new();
                while j < chars.len() && chars[j] != '}' {
                    var_name.push(chars[j]);
                    j += 1;
                }
                if j < chars.len() && chars[j] == '}' {
                    if let Ok(val) = std::env::var(&var_name) {
                        final_path.push_str(&val);
                    }
                    i = j + 1;
                    continue;
                }
            } else {
                let mut j = i + 1;
                let mut var_name = String::new();
                while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_') {
                    var_name.push(chars[j]);
                    j += 1;
                }
                if !var_name.is_empty() {
                    if let Ok(val) = std::env::var(&var_name) {
                        final_path.push_str(&val);
                    }
                    i = j;
                    continue;
                }
            }
        }
        final_path.push(chars[i]);
        i += 1;
    }

    let p = PathBuf::from(final_path);
    if p.is_relative() {
        return std::env::current_dir().map(|cwd| cwd.join(&p)).unwrap_or(p);
    }
    p
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
priority = ["_4", "_9.393DeepSeek"]

# Platform search and matching priority order
platform_priority = ["wy", "kw", "tx", "mg", "kg"]

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
    use std::path::PathBuf;
    use std::sync::Mutex;

    static TEST_MUTEX: Mutex<()> = Mutex::new(());

    fn sandbox_dir(name: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("{}-{}", name, std::process::id()));
        if dir.exists() {
            let _ = fs::remove_dir_all(&dir);
        }
        dir
    }

    /// DEF-013/#162: remove every path-influencing alx variable so each test
    /// starts from a deterministic baseline.
    fn clear_alx_path_env() {
        for var in ["ALX_HOME", "ALX_CONFIG", "ALX_DATA", "ALX_CACHE"] {
            unsafe {
                env::remove_var(var);
            }
        }
    }

    /// DEF-013/#162 published contract: when ALX_HOME is set, every root
    /// derives under it (<home>/config.toml, <home>/data, <home>/cache) and
    /// the remaining path variables are ignored in that mode.
    #[test]
    fn test_alx_home_layout_contract() {
        let _guard = TEST_MUTEX.lock().unwrap();
        clear_alx_path_env();

        let home = sandbox_dir("alx-test-home-contract");
        unsafe {
            env::set_var("ALX_HOME", home.to_str().unwrap());
        }

        let paths = resolve_paths();
        assert_eq!(paths.config_file, home.join("config.toml"));
        assert_eq!(paths.data_dir, home.join("data"));
        assert_eq!(paths.cache_dir, home.join("cache"));
        assert_eq!(paths.sources_dir, home.join("data").join("sources"));
        assert_eq!(paths.db_file, home.join("data").join("agent-lx-music.db"));

        // ALX_HOME short-circuits the per-root overrides while active.
        unsafe {
            env::set_var("ALX_CONFIG", "/elsewhere/custom.toml");
        }
        let paths = resolve_paths();
        assert_eq!(paths.config_file, home.join("config.toml"));

        unsafe {
            env::remove_var("ALX_CONFIG");
            env::remove_var("ALX_HOME");
        }
        let _ = fs::remove_dir_all(&home);
    }

    /// DEF-013/#162: without ALX_HOME the pure XDG layout applies, and the
    /// documented single-root overrides stay available.
    #[test]
    fn test_pure_xdg_mode_when_alx_home_unset() {
        let _guard = TEST_MUTEX.lock().unwrap();
        clear_alx_path_env();

        let xdg_config = sandbox_dir("alx-test-xdg-config");
        let xdg_data = sandbox_dir("alx-test-xdg-data");
        let xdg_cache = sandbox_dir("alx-test-xdg-cache");
        unsafe {
            env::set_var("XDG_CONFIG_HOME", xdg_config.to_str().unwrap());
            env::set_var("XDG_DATA_HOME", xdg_data.to_str().unwrap());
            env::set_var("XDG_CACHE_HOME", xdg_cache.to_str().unwrap());
        }

        let app_data = xdg_data.join("agent-lx-music");
        let paths = resolve_paths();
        assert_eq!(
            paths.config_file,
            xdg_config.join("agent-lx-music").join("config.toml")
        );
        assert_eq!(paths.data_dir, app_data.clone());
        assert_eq!(paths.sources_dir, app_data.join("sources"));
        assert_eq!(paths.db_file, app_data.join("agent-lx-music.db"));
        assert_eq!(paths.cache_dir, xdg_cache.join("agent-lx-music"));

        // Per-root overrides remain effective when ALX_HOME is unset.
        let custom_config = xdg_cache.join("custom-config.toml");
        unsafe {
            env::set_var("ALX_CONFIG", custom_config.to_str().unwrap());
        }
        assert_eq!(resolve_paths().config_file, custom_config);
        unsafe {
            env::remove_var("ALX_CONFIG");
        }

        for var in ["XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME"] {
            unsafe {
                env::remove_var(var);
            }
        }
        for dir in [&xdg_config, &xdg_data, &xdg_cache] {
            let _ = fs::remove_dir_all(dir);
        }
    }

    /// DEF-015/#164: save() stages to a .tmp sibling, fsyncs, and atomically
    /// renames over the target; no staging file is left behind on success.
    #[test]
    fn test_save_is_atomic_and_leaves_no_staging_file() {
        let _guard = TEST_MUTEX.lock().unwrap();
        clear_alx_path_env();

        let home = sandbox_dir("alx-test-atomic-save");
        unsafe {
            env::set_var("ALX_HOME", home.to_str().unwrap());
        }

        let mut config = Config::load().expect("first load initializes default config");
        config.player.default_volume = 66;
        config.save().unwrap();

        // The swapped-in file is complete and parses back with the change.
        let reloaded = Config::load().unwrap();
        assert_eq!(reloaded.player.default_volume, 66);

        // No staging leftovers in the config directory.
        let leftovers: Vec<_> = fs::read_dir(home.join("config.toml").parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "staging files left behind: {leftovers:?}"
        );

        unsafe {
            env::remove_var("ALX_HOME");
        }
        let _ = fs::remove_dir_all(&home);
    }

    /// DEF-015/#164: when the final rename cannot proceed, save() fails and
    /// cleans up the staging file instead of corrupting the target.
    #[test]
    fn test_save_failure_cleans_up_staging_file() {
        let _guard = TEST_MUTEX.lock().unwrap();
        clear_alx_path_env();

        let home = sandbox_dir("alx-test-atomic-save-failure");
        fs::create_dir_all(&home).unwrap();

        // Deterministic failure injection without relying on permission
        // bits (which are meaningless under root): point ALX_CONFIG at a
        // directory so rename(2) always fails with EISDIR.
        let target_dir = home.join("config.toml");
        fs::create_dir_all(&target_dir).unwrap();
        unsafe {
            env::set_var("ALX_CONFIG", target_dir.to_str().unwrap());
        }

        let config: Config = toml::from_str(get_default_config_toml()).unwrap();
        let err = config.save().unwrap_err();
        assert!(err.to_string().contains("replace config file"));

        // Staging file was removed and the "target" was left untouched.
        let leftovers: Vec<_> = fs::read_dir(&home)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "staging files left behind: {leftovers:?}"
        );
        assert!(target_dir.is_dir());

        unsafe {
            env::remove_var("ALX_CONFIG");
        }
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn test_all_config_operations() {
        let _guard = TEST_MUTEX.lock().unwrap();

        // 1. Test resolve paths with home override
        let temp_dir_home = env::temp_dir().join("alx-test-home");
        if temp_dir_home.exists() {
            let _ = fs::remove_dir_all(&temp_dir_home);
        }
        unsafe {
            env::set_var("ALX_HOME", temp_dir_home.to_str().unwrap());
        }

        let paths = resolve_paths();
        assert_eq!(paths.config_file, temp_dir_home.join("config.toml"));
        assert_eq!(paths.data_dir, temp_dir_home.join("data"));
        assert_eq!(paths.cache_dir, temp_dir_home.join("cache"));
        assert_eq!(
            paths.sources_dir,
            temp_dir_home.join("data").join("sources")
        );

        unsafe {
            env::remove_var("ALX_HOME");
        }

        // 2. Test config default load and save
        let temp_dir_load = env::temp_dir().join("alx-test-default-load-save");
        if temp_dir_load.exists() {
            let _ = fs::remove_dir_all(&temp_dir_load);
        }
        unsafe {
            env::set_var("ALX_HOME", temp_dir_load.to_str().unwrap());
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

        let _ = fs::remove_dir_all(&temp_dir_load);
        let _ = fs::remove_dir_all(&temp_dir_home);
        unsafe {
            env::remove_var("ALX_HOME");
        }
    }

    #[test]
    fn test_expand_path() {
        let _guard = TEST_MUTEX.lock().unwrap();

        // Test tilde expansion
        if let Some(home) = dirs::home_dir() {
            let expanded = expand_path("~/Music/agent-lx-music");
            assert_eq!(expanded, home.join("Music/agent-lx-music"));

            let expanded_only_tilde = expand_path("~");
            assert_eq!(expanded_only_tilde, home);
        }

        // Test environment variable expansion
        unsafe {
            env::set_var("TEST_DIR", "foo");
            env::set_var("TEST_SUB", "bar");
        }

        let expanded_simple = expand_path("/tmp/$TEST_DIR/baz");
        assert_eq!(expanded_simple, PathBuf::from("/tmp/foo/baz"));

        let expanded_braces = expand_path("/tmp/${TEST_DIR}_test/$TEST_SUB");
        assert_eq!(expanded_braces, PathBuf::from("/tmp/foo_test/bar"));

        // Test relative path resolution
        if let Ok(cwd) = std::env::current_dir() {
            let expanded_relative = expand_path("some/relative/path.mp3");
            assert_eq!(expanded_relative, cwd.join("some/relative/path.mp3"));
        }

        unsafe {
            env::remove_var("TEST_DIR");
            env::remove_var("TEST_SUB");
        }
    }
}

#![allow(clippy::collapsible_if, clippy::collapsible_else_if)]
pub mod ipc;

use anyhow::{Result, anyhow};
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::Duration;

pub struct MpvClient {
    pub socket_path: PathBuf,
    pub default_volume: u8,
}

impl Default for MpvClient {
    fn default() -> Self {
        let paths = lux_core::config::resolve_paths();
        let socket_path = paths.cache_dir.join("mpv.sock");
        Self {
            socket_path,
            default_volume: 80,
        }
    }
}

#[cfg(unix)]
impl MpvClient {
    pub fn new() -> Self {
        let config = lux_core::config::Config::load().unwrap_or_default();
        let paths = lux_core::config::resolve_paths();
        let socket_path = paths.cache_dir.join("mpv.sock");
        Self {
            socket_path,
            default_volume: config.player.default_volume,
        }
    }

    pub fn ensure_running(&self) -> Result<()> {
        if std::os::unix::net::UnixStream::connect(&self.socket_path).is_ok() {
            return Ok(());
        }

        // Clean up stale socket file if it exists
        if self.socket_path.exists() {
            let _ = fs::remove_file(&self.socket_path);
        }

        if let Some(parent) = self.socket_path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        // Spawn mpv background process
        let volume_arg = format!("--volume={}", self.default_volume);
        let ipc_arg = format!("--input-ipc-server={}", self.socket_path.display());

        let mut cmd = Command::new("setsid");
        cmd.arg("mpv")
            .arg("--idle")
            .arg("--no-video")
            .arg("--vo=null")
            .arg(&ipc_arg)
            .arg(&volume_arg);

        // Append optional user mpv arguments from config
        let config = lux_core::config::Config::load().unwrap_or_default();

        // Auto-mount mpv-mpris if enabled
        if config.player.enable_mpris {
            let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/home/fuyu"));
            let mpris_paths = vec![
                home.join(".config/mpv/scripts/mpris.so"),
                PathBuf::from("/usr/lib/mpv/scripts/mpris.so"),
                PathBuf::from("/usr/lib/mpv/mpris.so"),
                PathBuf::from("/usr/lib/mpv-mpris/mpris.so"),
                PathBuf::from("/usr/lib/x86_64-linux-gnu/mpv/scripts/mpris.so"),
            ];
            for path in mpris_paths {
                if path.exists() {
                    cmd.arg(format!("--script={}", path.display()));
                    break;
                }
            }
        }

        for arg in &config.player.mpv_args {
            cmd.arg(arg);
        }

        // Redirect stdout/stderr to a log file to allow inspection of background execution issues
        let paths = lux_core::config::resolve_paths();
        let log_path = paths.cache_dir.join("mpv.log");
        let log_file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&log_path)
            .unwrap();
        let err_file = log_file.try_clone().unwrap();

        cmd.stdout(log_file)
            .stderr(err_file)
            .stdin(std::process::Stdio::null());

        let mut child = cmd
            .spawn()
            .map_err(|e| anyhow!("Failed to start mpv: {}", e))?;

        // Wait up to 1000ms for mpv to start and create the Unix socket
        for _ in 0..20 {
            thread::sleep(Duration::from_millis(50));
            if std::os::unix::net::UnixStream::connect(&self.socket_path).is_ok() {
                // Spawn background playback position ticker
                Self::trigger_playback_ticker(self.socket_path.clone());
                return Ok(());
            }
            // Check if process exited early
            if let Ok(Some(status)) = child.try_wait() {
                return Err(anyhow!("mpv process exited early with status: {}", status));
            }
        }

        Err(anyhow!("Timeout waiting for mpv IPC socket to be created"))
    }

    fn trigger_playback_ticker(socket_path: PathBuf) {
        static LAUNCHED_TICKER: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);
        if !LAUNCHED_TICKER.swap(true, std::sync::atomic::Ordering::Relaxed) {
            std::thread::spawn(move || {
                loop {
                    std::thread::sleep(Duration::from_secs(5));
                    let client = MpvClient {
                        socket_path: socket_path.clone(),
                        default_volume: 80,
                    };
                    if let Ok(Some((_path, pos, _duration, vol, paused))) =
                        client.get_playback_status()
                    {
                        if !paused && pos > 0.0 {
                            let _ = Self::update_current_playing_position(pos, vol);
                        }
                        if let Ok(Some(new_idx)) = client.get_playing_index() {
                            let _ = Self::sync_queue_playing_index(new_idx);
                        }
                    }
                }
            });
        }
    }

    fn update_current_playing_position(pos: f64, vol: u8) -> Result<()> {
        let paths = lux_core::config::resolve_paths();
        let current_json_path = paths.cache_dir.join("current.json");
        if current_json_path.exists() {
            if let Ok(content) = fs::read_to_string(&current_json_path) {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                    let song = if val.get("song").is_some() {
                        val.get("song").cloned().unwrap()
                    } else {
                        val.clone()
                    };

                    let new_state = serde_json::json!({
                        "song": song,
                        "last_position": pos,
                        "volume": vol,
                        "updated_at": chrono::Local::now().to_rfc3339()
                    });

                    let serialized = serde_json::to_string(&new_state)?;
                    let _ = fs::write(current_json_path, serialized);
                }
            }
        }
        Ok(())
    }

    fn sync_queue_playing_index(new_idx: usize) -> Result<()> {
        let paths = lux_core::config::resolve_paths();
        let queue_json_path = paths.cache_dir.join("queue.json");
        if !queue_json_path.exists() {
            return Ok(());
        }

        let content = fs::read_to_string(&queue_json_path)?;
        #[derive(serde::Serialize, serde::Deserialize)]
        struct LocalPlayQueue {
            songs: Vec<crate::library::db::SearchCacheEntry>,
            current_index: Option<usize>,
        }

        let mut queue: LocalPlayQueue = serde_json::from_str(&content)?;
        if queue.current_index != Some(new_idx) {
            queue.current_index = Some(new_idx);

            if new_idx < queue.songs.len() {
                let song = &queue.songs[new_idx];

                // 1. Save to current.json
                let current_json_path = paths.cache_dir.join("current.json");
                let new_state = serde_json::json!({
                    "song": song,
                    "last_position": 0.0,
                    "volume": 80,
                    "updated_at": chrono::Local::now().to_rfc3339()
                });
                let _ = fs::write(current_json_path, serde_json::to_string(&new_state)?);

                // 2. Add to history
                let _ = crate::library::db::add_to_history(song, None);
            }

            let serialized = serde_json::to_string(&queue)?;
            fs::write(queue_json_path, serialized)?;
        }
        Ok(())
    }

    pub fn play_file_or_url(&self, path_or_url: &str) -> Result<()> {
        self.ensure_running()?;
        let _ = ipc::send_mpv_command(
            &self.socket_path,
            vec![json!("loadfile"), json!(path_or_url), json!("replace")],
        )?;
        Ok(())
    }

    pub fn append_file_or_url(&self, path_or_url: &str) -> Result<()> {
        self.ensure_running()?;
        let _ = ipc::send_mpv_command(
            &self.socket_path,
            vec![json!("loadfile"), json!(path_or_url), json!("append")],
        )?;
        Ok(())
    }

    pub fn pause(&self) -> Result<()> {
        self.ensure_running()?;
        let _ = ipc::send_mpv_command(
            &self.socket_path,
            vec![json!("set_property"), json!("pause"), json!(true)],
        )?;
        Ok(())
    }

    pub fn resume(&self) -> Result<()> {
        self.ensure_running()?;
        let _ = ipc::send_mpv_command(
            &self.socket_path,
            vec![json!("set_property"), json!("pause"), json!(false)],
        )?;
        Ok(())
    }

    pub fn stop(&self) -> Result<()> {
        self.ensure_running()?;
        let _ = ipc::send_mpv_command(&self.socket_path, vec![json!("stop")])?;
        Ok(())
    }

    pub fn set_volume(&self, vol: u8) -> Result<()> {
        self.ensure_running()?;
        let _ = ipc::send_mpv_command(
            &self.socket_path,
            vec![json!("set_property"), json!("volume"), json!(vol)],
        )?;
        Ok(())
    }

    pub fn get_volume(&self) -> Result<u8> {
        self.ensure_running()?;
        let val = ipc::send_mpv_command(
            &self.socket_path,
            vec![json!("get_property"), json!("volume")],
        )?;
        let vol = val.as_f64().unwrap_or(80.0) as u8;
        Ok(vol)
    }

    pub fn seek(&self, val: &str) -> Result<()> {
        self.ensure_running()?;
        if val.starts_with('+') || val.starts_with('-') {
            let offset: f64 = val.parse()?;
            let _ = ipc::send_mpv_command(
                &self.socket_path,
                vec![json!("seek"), json!(offset), json!("relative")],
            )?;
        } else if val.contains(':') {
            let parts: Vec<&str> = val.split(':').collect();
            if parts.len() == 2 {
                let mins: f64 = parts[0]
                    .parse()
                    .map_err(|_| anyhow::anyhow!("Invalid minutes"))?;
                let secs: f64 = parts[1]
                    .parse()
                    .map_err(|_| anyhow::anyhow!("Invalid seconds"))?;
                if mins < 0.0 || !(0.0..60.0).contains(&secs) {
                    return Err(anyhow::anyhow!(
                        "Invalid seek time: seconds must be less than 60 and both values must be non-negative"
                    ));
                }
                let total = mins * 60.0 + secs;
                let _ = ipc::send_mpv_command(
                    &self.socket_path,
                    vec![json!("seek"), json!(total), json!("absolute")],
                )?;
            } else if parts.len() == 3 {
                let hours: f64 = parts[0]
                    .parse()
                    .map_err(|_| anyhow::anyhow!("Invalid hours"))?;
                let mins: f64 = parts[1]
                    .parse()
                    .map_err(|_| anyhow::anyhow!("Invalid minutes"))?;
                let secs: f64 = parts[2]
                    .parse()
                    .map_err(|_| anyhow::anyhow!("Invalid seconds"))?;
                if hours < 0.0 || !(0.0..60.0).contains(&mins) || !(0.0..60.0).contains(&secs) {
                    return Err(anyhow::anyhow!(
                        "Invalid seek time: minutes and seconds must be less than 60 and all values must be non-negative"
                    ));
                }
                let total = hours * 3600.0 + mins * 60.0 + secs;
                let _ = ipc::send_mpv_command(
                    &self.socket_path,
                    vec![json!("seek"), json!(total), json!("absolute")],
                )?;
            } else {
                return Err(anyhow::anyhow!(
                    "Invalid seek time format: use MM:SS or HH:MM:SS"
                ));
            }
        } else if val.ends_with('%') {
            let percent: f64 = val
                .trim_end_matches('%')
                .parse()
                .map_err(|_| anyhow::anyhow!("Invalid percentage"))?;
            if !(0.0..=100.0).contains(&percent) {
                return Err(anyhow::anyhow!(
                    "Invalid seek percentage: must be between 0 and 100"
                ));
            }
            let _ = ipc::send_mpv_command(
                &self.socket_path,
                vec![json!("seek"), json!(percent), json!("absolute-percent")],
            )?;
        } else {
            let secs: f64 = val
                .parse()
                .map_err(|_| anyhow::anyhow!("Invalid seconds"))?;
            if secs < 0.0 {
                return Err(anyhow::anyhow!(
                    "Invalid seek time: seconds must be non-negative"
                ));
            }
            let _ = ipc::send_mpv_command(
                &self.socket_path,
                vec![json!("seek"), json!(secs), json!("absolute")],
            )?;
        }
        Ok(())
    }

    pub fn set_repeat(&self, mode: &str) -> Result<()> {
        self.ensure_running()?;
        match mode {
            "one" => {
                let _ = ipc::send_mpv_command(
                    &self.socket_path,
                    vec![json!("set_property"), json!("loop-file"), json!("inf")],
                )?;
                let _ = ipc::send_mpv_command(
                    &self.socket_path,
                    vec![json!("set_property"), json!("loop-playlist"), json!("no")],
                )?;
            }
            "all" => {
                let _ = ipc::send_mpv_command(
                    &self.socket_path,
                    vec![json!("set_property"), json!("loop-file"), json!("no")],
                )?;
                let _ = ipc::send_mpv_command(
                    &self.socket_path,
                    vec![json!("set_property"), json!("loop-playlist"), json!("inf")],
                )?;
            }
            _ => {
                let _ = ipc::send_mpv_command(
                    &self.socket_path,
                    vec![json!("set_property"), json!("loop-file"), json!("no")],
                )?;
                let _ = ipc::send_mpv_command(
                    &self.socket_path,
                    vec![json!("set_property"), json!("loop-playlist"), json!("no")],
                )?;
            }
        }
        Ok(())
    }

    #[allow(clippy::type_complexity)]
    pub fn get_playback_status(&self) -> Result<Option<(String, f64, f64, u8, bool)>> {
        if std::os::unix::net::UnixStream::connect(&self.socket_path).is_err() {
            return Ok(None);
        }

        let path = ipc::send_mpv_command(
            &self.socket_path,
            vec![json!("get_property"), json!("path")],
        )
        .ok()
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_default();

        if path.is_empty() {
            return Ok(None);
        }

        let pos = ipc::send_mpv_command(
            &self.socket_path,
            vec![json!("get_property"), json!("time-pos")],
        )
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);

        let duration = ipc::send_mpv_command(
            &self.socket_path,
            vec![json!("get_property"), json!("duration")],
        )
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);

        let vol = ipc::send_mpv_command(
            &self.socket_path,
            vec![json!("get_property"), json!("volume")],
        )
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(80.0) as u8;

        let paused = ipc::send_mpv_command(
            &self.socket_path,
            vec![json!("get_property"), json!("pause")],
        )
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

        Ok(Some((path, pos, duration, vol, paused)))
    }

    pub fn get_playing_index(&self) -> Result<Option<usize>> {
        if std::os::unix::net::UnixStream::connect(&self.socket_path).is_err() {
            return Ok(None);
        }
        let val = ipc::send_mpv_command(
            &self.socket_path,
            vec![json!("get_property"), json!("playlist-playing-pos")],
        )?;
        if val.is_null() {
            return Ok(None);
        }
        let idx = val.as_i64().map(|v| v as usize);
        Ok(idx)
    }

    pub fn next(&self) -> Result<()> {
        self.ensure_running()?;
        let _ = ipc::send_mpv_command(&self.socket_path, vec![json!("playlist-next")]);
        Ok(())
    }

    pub fn prev(&self) -> Result<()> {
        self.ensure_running()?;
        let _ = ipc::send_mpv_command(&self.socket_path, vec![json!("playlist-prev")]);
        Ok(())
    }

    pub fn quit(&self) -> Result<()> {
        if std::os::unix::net::UnixStream::connect(&self.socket_path).is_ok() {
            let _ = ipc::send_mpv_command(&self.socket_path, vec![json!("quit")]);
        }
        if self.socket_path.exists() {
            let _ = fs::remove_file(&self.socket_path);
        }
        Ok(())
    }
}

#[cfg(not(unix))]
impl MpvClient {
    pub fn new() -> Self {
        let config = lux_core::config::Config::load().unwrap_or_default();
        let paths = lux_core::config::resolve_paths();
        let socket_path = paths.cache_dir.join("mpv.sock");
        Self {
            socket_path,
            default_volume: config.player.default_volume,
        }
    }

    pub fn ensure_running(&self) -> Result<()> {
        Err(anyhow!("Player is not supported on this platform"))
    }

    pub fn play_file_or_url(&self, _path_or_url: &str) -> Result<()> {
        Err(anyhow!("Player is not supported on this platform"))
    }

    pub fn append_file_or_url(&self, _path_or_url: &str) -> Result<()> {
        Err(anyhow!("Player is not supported on this platform"))
    }

    pub fn pause(&self) -> Result<()> {
        Err(anyhow!("Player is not supported on this platform"))
    }

    pub fn resume(&self) -> Result<()> {
        Err(anyhow!("Player is not supported on this platform"))
    }

    pub fn stop(&self) -> Result<()> {
        Err(anyhow!("Player is not supported on this platform"))
    }

    pub fn set_volume(&self, _vol: u8) -> Result<()> {
        Err(anyhow!("Player is not supported on this platform"))
    }

    pub fn get_volume(&self) -> Result<u8> {
        Err(anyhow!("Player is not supported on this platform"))
    }

    pub fn seek(&self, _val: &str) -> Result<()> {
        Err(anyhow!("Player is not supported on this platform"))
    }

    pub fn set_repeat(&self, _mode: &str) -> Result<()> {
        Err(anyhow!("Player is not supported on this platform"))
    }

    pub fn get_playback_status(&self) -> Result<Option<(String, f64, f64, u8, bool)>> {
        Ok(None)
    }

    pub fn get_playing_index(&self) -> Result<Option<usize>> {
        Ok(None)
    }

    pub fn next(&self) -> Result<()> {
        Err(anyhow!("Player is not supported on this platform"))
    }

    pub fn prev(&self) -> Result<()> {
        Err(anyhow!("Player is not supported on this platform"))
    }

    pub fn quit(&self) -> Result<()> {
        Ok(())
    }
}

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

        let mut cmd = Command::new("mpv");
        cmd.arg("--idle")
            .arg("--no-video")
            .arg(&ipc_arg)
            .arg(&volume_arg);

        // Append optional user mpv arguments from config
        let config = lux_core::config::Config::load().unwrap_or_default();
        for arg in &config.player.mpv_args {
            cmd.arg(arg);
        }

        // Redirect stdout/stderr to inherit to debug
        cmd.stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit());

        let mut child = cmd
            .spawn()
            .map_err(|e| anyhow!("Failed to start mpv: {}", e))?;

        // Wait up to 1000ms for mpv to start and create the Unix socket
        for _ in 0..20 {
            thread::sleep(Duration::from_millis(50));
            if std::os::unix::net::UnixStream::connect(&self.socket_path).is_ok() {
                return Ok(());
            }
            // Check if process exited early
            if let Ok(Some(status)) = child.try_wait() {
                return Err(anyhow!("mpv process exited early with status: {}", status));
            }
        }

        Err(anyhow!("Timeout waiting for mpv IPC socket to be created"))
    }

    pub fn play_file_or_url(&self, path_or_url: &str) -> Result<()> {
        self.ensure_running()?;
        let _ = ipc::send_mpv_command(
            &self.socket_path,
            vec![json!("loadfile"), json!(path_or_url), json!("replace")],
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
                let mins: f64 = parts[0].parse()?;
                let secs: f64 = parts[1].parse()?;
                let total = mins * 60.0 + secs;
                let _ = ipc::send_mpv_command(
                    &self.socket_path,
                    vec![json!("seek"), json!(total), json!("absolute")],
                )?;
            }
        } else if val.ends_with('%') {
            let percent: f64 = val.trim_end_matches('%').parse()?;
            let _ = ipc::send_mpv_command(
                &self.socket_path,
                vec![json!("seek"), json!(percent), json!("absolute-percent")],
            )?;
        } else {
            let secs: f64 = val.parse()?;
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

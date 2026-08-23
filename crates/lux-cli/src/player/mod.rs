#![allow(clippy::collapsible_if, clippy::collapsible_else_if)]
pub mod ipc;
pub mod spawn_lock;

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
        let config = lux_core::config::Config::load().unwrap_or_default();
        let paths = lux_core::config::resolve_paths();
        let socket_path = paths.cache_dir.join("mpv.sock");
        Self {
            socket_path,
            default_volume: config.player.default_volume,
        }
    }
}

const SYNC_LUA: &str = r#"
local utils = require 'mp.utils'

local function get_cache_dir()
    local home = os.getenv("HOME")
    if not home then return "/tmp" end
    return home .. "/.cache/agent-lx-music"
end

local function update_state()
    print("Lua: update_state triggered")
    local pos = mp.get_property_number("time-pos", 0)
    local vol = mp.get_property_number("volume", 100)
    local paused = mp.get_property_bool("pause", false)
    local index = mp.get_property_number("playlist-playing-pos", 0)
    
    local cache_dir = get_cache_dir()
    local queue_path = cache_dir .. "/queue.json"
    local current_path = cache_dir .. "/current.json"
    
    local f = io.open(queue_path, "r")
    if not f then 
        print("Lua: queue.json not found at " .. queue_path)
        return 
    end
    local content = f:read("*all")
    f:close()
    
    local queue = utils.parse_json(content)
    if not queue or not queue.songs then 
        print("Lua: Failed to parse queue.json")
        return 
    end
    
    if queue.current_index ~= index then
        print("Lua: Updating queue index to " .. index)
        queue.current_index = index
        local fq = io.open(queue_path, "w")
        if fq then
            fq:write(utils.format_json(queue))
            fq:close()
        end
    end
    
    local song = queue.songs[index + 1]
    if song then
        print("Lua: Updating current.json for song " .. song.name)
        local state = {
            song = song,
            last_position = pos,
            volume = math.floor(vol),
            updated_at = os.date("!%Y-%m-%dT%H:%M:%SZ")
        }
        local fc = io.open(current_path, "w")
        if fc then
            fc:write(utils.format_json(state))
            fc:close()
        end
    else
        print("Lua: No song found in queue at index " .. index)
    end
end

mp.observe_property("playlist-playing-pos", "number", update_state)
mp.observe_property("pause", "bool", update_state)
mp.register_event("file-loaded", update_state)

-- Periodically sync position
mp.add_periodic_timer(5, update_state)
"#;

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

    /// Probe-only variant of [`MpvClient::ensure_running`] that never spawns
    /// a daemon. Use for stateless commands where silently cold-starting
    /// headless mpv is a surprising side effect.
    pub fn try_ensure_running(&self) -> Result<()> {
        if std::os::unix::net::UnixStream::connect(&self.socket_path).is_ok() {
            Ok(())
        } else {
            Err(anyhow!(
                "Player daemon is not running (start playback with 'alx play' first)"
            ))
        }
    }

    pub fn ensure_running(&self) -> Result<()> {
        // Serialize probe→spawn across processes: two concurrent invocations
        // must not both unlink/bind/spawn. Hold the lock for the whole
        // critical section.
        let _spawn_guard =
            spawn_lock::SpawnLock::acquire(&self.socket_path.with_extension("lock"))?;

        // Re-probe inside the critical section: another process may have
        // finished spawning mpv while we waited for the lock.
        if std::os::unix::net::UnixStream::connect(&self.socket_path).is_ok() {
            return Ok(());
        }

        // Clean up stale socket file if it exists (safe: nothing can have
        // freshly bound it while we hold the spawn lock).
        if self.socket_path.exists() {
            let _ = fs::remove_file(&self.socket_path);
        }

        if let Some(parent) = self.socket_path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        // Deploy sync Lua script
        let paths = lux_core::config::resolve_paths();
        let scripts_dir = paths.cache_dir.join("scripts");
        let _ = fs::create_dir_all(&scripts_dir);
        let sync_script_path = scripts_dir.join("sync.lua");
        let _ = fs::write(&sync_script_path, SYNC_LUA);

        // Spawn mpv background process
        let volume_arg = format!("--volume={}", self.default_volume);
        let ipc_arg = format!("--input-ipc-server={}", self.socket_path.display());

        let mut cmd = Command::new("setsid");
        cmd.arg("mpv")
            .arg("--idle")
            .arg("--no-video")
            .arg("--vo=null")
            .arg(format!("--script={}", sync_script_path.display()))
            .arg(&ipc_arg)
            .arg(&volume_arg);

        // Append optional user mpv arguments from config
        let config = lux_core::config::Config::load().unwrap_or_default();

        // Auto-mount mpv-mpris if enabled
        if config.player.enable_mpris {
            // Without a resolvable home directory the user-config script
            // location is unknown; skip auto-mount entirely rather than
            // guessing a hardcoded path.
            if let Some(home) = dirs::home_dir() {
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
        }

        for arg in &config.player.mpv_args {
            cmd.arg(arg);
        }

        // Redirect stdout/stderr to a log file to allow inspection of
        // background execution issues; fall back to /dev/null when the cache
        // directory is unwritable — a missing log must not abort playback.
        let paths = lux_core::config::resolve_paths();
        let log_path = paths.cache_dir.join("mpv.log");
        let (log_file, err_file) = match fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&log_path)
        {
            Ok(file) => match file.try_clone() {
                Ok(err) => (file.into(), err.into()),
                Err(e) => {
                    eprintln!(
                        "warning: cannot duplicate mpv log file {}: {e}",
                        log_path.display()
                    );
                    (std::process::Stdio::null(), std::process::Stdio::null())
                }
            },
            Err(e) => {
                eprintln!(
                    "warning: cannot open mpv log file {}: {e}",
                    log_path.display()
                );
                (std::process::Stdio::null(), std::process::Stdio::null())
            }
        };

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
        let vol = val.as_f64().unwrap_or(self.default_volume as f64) as u8;
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
        .unwrap_or(self.default_volume as f64) as u8;

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
        Ok(normalize_playing_index(val))
    }

    pub fn next(&self) -> Result<()> {
        self.try_ensure_running()?;
        ipc::send_mpv_command(&self.socket_path, vec![json!("playlist-next")])?;
        Ok(())
    }

    pub fn prev(&self) -> Result<()> {
        self.try_ensure_running()?;
        ipc::send_mpv_command(&self.socket_path, vec![json!("playlist-prev")])?;
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

/// Map a failed skip (`playlist-next`/`playlist-prev`) to a clear,
/// user-facing message. Boundary failures reported by mpv ("no more
/// files", "no file") become end/start-of-queue notices; anything else is
/// surfaced verbatim so IPC/socket problems stay diagnosable.
pub fn describe_skip_error(direction: &str, err: &anyhow::Error) -> String {
    let msg = format!("{err:#}");
    if msg.contains("no more files") || msg.contains("no file") {
        if direction == "next" {
            "End of queue reached; nothing to skip forward to.".to_string()
        } else {
            "Start of queue reached; nothing to skip back to.".to_string()
        }
    } else {
        msg
    }
}

/// Map an mpv `playlist-playing-pos` response to a queue position.
///
/// `null` (no playlist) and negative values (idle: `-1`) mean "nothing is
/// playing" — never saturate to index 0.
fn normalize_playing_index(val: serde_json::Value) -> Option<usize> {
    val.as_i64()
        .filter(|v| *v >= 0)
        .and_then(|v| usize::try_from(v).ok())
}

#[cfg(test)]
mod tests {
    use super::describe_skip_error;
    use super::normalize_playing_index;
    use anyhow::anyhow;
    use serde_json::{Value, json};

    #[test]
    fn idle_negative_position_maps_to_none() {
        assert_eq!(normalize_playing_index(json!(-1)), None);
    }

    #[test]
    fn absent_property_maps_to_none() {
        assert_eq!(normalize_playing_index(Value::Null), None);
        assert_eq!(normalize_playing_index(json!("nonsense")), None);
        assert_eq!(normalize_playing_index(json!(3.7)), None);
    }

    #[test]
    fn valid_positions_pass_through() {
        assert_eq!(normalize_playing_index(json!(0)), Some(0));
        assert_eq!(normalize_playing_index(json!(42)), Some(42));
    }

    #[test]
    fn boundary_errors_map_to_end_of_queue() {
        let err = anyhow!("mpv error: no more files");
        assert_eq!(
            describe_skip_error("next", &err),
            "End of queue reached; nothing to skip forward to."
        );
    }

    #[test]
    fn boundary_errors_map_to_start_of_queue() {
        let err = anyhow!("mpv error: no more files");
        assert_eq!(
            describe_skip_error("prev", &err),
            "Start of queue reached; nothing to skip back to."
        );
    }

    #[test]
    fn connection_errors_pass_through_for_diagnosis() {
        // Daemon absence is reported by try_ensure_running before any skip
        // happens; if such an error ever reaches the mapper it must stay
        // verbatim rather than be misread as a queue boundary.
        let err = anyhow!("Failed to connect to mpv socket: No such file or directory");
        assert_eq!(
            describe_skip_error("next", &err),
            "Failed to connect to mpv socket: No such file or directory"
        );
    }

    #[test]
    fn unknown_errors_pass_through_verbatim() {
        let err = anyhow!("mpv error: something exploded");
        assert_eq!(
            describe_skip_error("next", &err),
            "mpv error: something exploded"
        );
    }
}

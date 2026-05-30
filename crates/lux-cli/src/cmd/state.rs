use crate::player::MpvClient;
use anyhow::Result;
use serde_json::json;
use std::fs;

pub fn run(json: bool) -> Result<()> {
    let client = MpvClient::new();
    let paths = lux_core::config::resolve_paths();

    // 1. Check if mpv daemon is active
    #[cfg(unix)]
    let stream_ok = std::os::unix::net::UnixStream::connect(&client.socket_path).is_ok();
    #[cfg(not(unix))]
    let stream_ok = false;

    if !stream_ok {
        if json {
            println!(
                "{}",
                json!({
                    "status": "stopped",
                    "daemon_active": false,
                    "volume": 0,
                    "repeat": "off",
                    "shuffle": false,
                    "position": 0.0,
                    "duration": 0.0,
                    "queue_length": 0,
                    "current_song": null
                })
            );
        } else {
            println!("mpv daemon: inactive (stopped)");
        }
        return Ok(());
    }

    // 2. Query playback status
    let status_res = client.get_playback_status().unwrap_or(None);
    let (pos, duration, vol, paused) = if let Some((_path, p, dur, v, ps)) = status_res {
        (p, dur, v, ps)
    } else {
        (0.0, 0.0, client.default_volume, true)
    };

    // 3. Query repeat & shuffle properties via mpv IPC
    let loop_file = crate::player::ipc::send_mpv_command(
        &client.socket_path,
        vec![json!("get_property"), json!("loop-file")],
    )
    .ok()
    .and_then(|v| v.as_str().map(|s| s.to_string()))
    .unwrap_or_else(|| "no".to_string());

    let loop_playlist = crate::player::ipc::send_mpv_command(
        &client.socket_path,
        vec![json!("get_property"), json!("loop-playlist")],
    )
    .ok()
    .and_then(|v| v.as_str().map(|s| s.to_string()))
    .unwrap_or_else(|| "no".to_string());

    let repeat_mode = if loop_file == "inf" || loop_file == "yes" {
        "one"
    } else if loop_playlist == "inf" || loop_playlist == "yes" {
        "all"
    } else {
        "off"
    };

    let shuffle_prop = crate::player::ipc::send_mpv_command(
        &client.socket_path,
        vec![json!("get_property"), json!("shuffle")],
    )
    .ok()
    .and_then(|v| v.as_bool())
    .unwrap_or(false);

    // 4. Load current playing song details
    let current_json_path = paths.cache_dir.join("current.json");
    let current_song: Option<serde_json::Value> = if current_json_path.exists() {
        fs::read_to_string(&current_json_path)
            .ok()
            .and_then(|content| serde_json::from_str::<serde_json::Value>(&content).ok())
            .and_then(|val| {
                if val.get("song").is_some() {
                    val.get("song").cloned()
                } else {
                    Some(val)
                }
            })
    } else {
        None
    };

    // 5. Load active queue length
    let queue_json_path = paths.cache_dir.join("queue.json");
    let queue_length = if queue_json_path.exists() {
        fs::read_to_string(&queue_json_path)
            .ok()
            .and_then(|content| serde_json::from_str::<serde_json::Value>(&content).ok())
            .and_then(|val| val["songs"].as_array().map(|arr| arr.len()))
            .unwrap_or(0)
    } else {
        0
    };

    let status_str = if paused { "paused" } else { "playing" };

    if json {
        println!(
            "{}",
            json!({
                "status": status_str,
                "daemon_active": true,
                "volume": vol,
                "repeat": repeat_mode,
                "shuffle": shuffle_prop,
                "position": pos,
                "duration": duration,
                "queue_length": queue_length,
                "current_song": current_song
            })
        );
    } else {
        use colored::Colorize;
        println!("{}", "── agent-lx-music Player State ──".bold().green());
        println!(
            "Status:       {}",
            if paused {
                "Paused ⏸".yellow()
            } else {
                "Playing ▶".green()
            }
        );
        println!("Volume:       {}%", vol);
        println!("Repeat:       {}", repeat_mode);
        println!("Shuffle:      {}", if shuffle_prop { "on" } else { "off" });
        println!("Progress:     {:.0}s / {:.0}s", pos, duration);
        println!("Queue Length: {} songs", queue_length);
        if let Some(ref song) = current_song {
            let title = song["name"].as_str().unwrap_or("Unknown");
            let artist = song["singer"].as_str().unwrap_or("Unknown");
            let source = song["source"].as_str().unwrap_or("unknown");
            println!("Current Song: {} - {} [{}]", title.bold(), artist, source);
        }
    }

    Ok(())
}

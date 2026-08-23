use crate::library::db::SearchCacheEntry;
use crate::player::MpvClient;
use anyhow::Result;
use colored::Colorize;
use std::fs;

pub fn run(json: bool) -> Result<()> {
    let client = MpvClient::new();
    let status = client.get_playback_status()?;

    if let Some((_path, pos, duration, vol, paused)) = status {
        // Retrieve current metadata from ~/.cache/rust-lx/current.json
        let paths = lux_core::config::resolve_paths();
        let current_json_path = paths.cache_dir.join("current.json");
        let song_opt: Option<SearchCacheEntry> = if current_json_path.exists() {
            if let Ok(content) = fs::read_to_string(current_json_path) {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(song_val) = val.get("song") {
                        serde_json::from_value::<SearchCacheEntry>(song_val.clone()).ok()
                    } else {
                        serde_json::from_value::<SearchCacheEntry>(val).ok()
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        if json {
            println!(
                "{}",
                serde_json::json!({
                    "status": if paused { "paused" } else { "playing" },
                    "position": pos,
                    "duration": duration,
                    "volume": vol,
                    "song": song_opt
                })
            );
        } else {
            let status_indicator = if paused {
                "⏸".yellow().bold()
            } else {
                "♫".green().bold()
            };

            if let Some(song) = song_opt {
                println!(
                    "\n{} {} — {}",
                    status_indicator,
                    song.name.bold(),
                    song.singer.cyan()
                );

                if let Some(ref album) = song.album_name {
                    print!("  Album: {} | ", album);
                } else {
                    print!("  ");
                }
                println!(
                    "Source: {} | ID: {}",
                    song.source.green(),
                    song.cli_id.yellow()
                );
            } else {
                println!("\n{} Playing direct URL/stream", status_indicator);
            }

            // Construct progress bar (20 chars wide)
            let (filled, empty) = progress_bar(pos, duration);
            let bar = format!("{}{}", filled.green(), empty.dimmed());

            let current_time = format_time(pos);
            let total_time = format_time(duration);

            println!(
                "  [{}] {} / {}  Vol: {}%\n",
                bar,
                current_time.bold(),
                total_time.bold(),
                vol
            );
        }
    } else {
        if json {
            println!(
                "{}",
                serde_json::json!({
                    "status": "stopped",
                    "message": "No playback is running"
                })
            );
        } else {
            println!("♫ No song is playing currently.");
        }
    }

    Ok(())
}

fn format_time(secs_f: f64) -> String {
    let secs = secs_f.round() as i64;
    format!("{:02}:{:02}", secs / 60, secs % 60)
}

/// 20-char progress bar split into (filled, empty) runs.
///
/// `pos` can exceed `duration` (live streams report bogus durations) and
/// both are clamped so the subtraction can never underflow.
const BAR_WIDTH: usize = 20;

fn progress_bar(pos: f64, duration: f64) -> (String, String) {
    let percent = if duration > 0.0 { pos / duration } else { 0.0 };
    let filled = ((percent * BAR_WIDTH as f64).round() as i64).clamp(0, BAR_WIDTH as i64) as usize;
    ("█".repeat(filled), "░".repeat(BAR_WIDTH - filled))
}

#[cfg(test)]
mod tests {
    use super::progress_bar;

    fn widths((f, e): &(String, String)) -> (usize, usize) {
        (f.chars().count(), e.chars().count())
    }

    #[test]
    fn pos_beyond_duration_clamps_to_full_bar() {
        assert_eq!(widths(&progress_bar(250.0, 200.0)), (20, 0));
        assert_eq!(widths(&progress_bar(1e9, 3.5)), (20, 0));
    }

    #[test]
    fn zero_duration_yields_empty_bar() {
        assert_eq!(widths(&progress_bar(12.0, 0.0)), (0, 20));
        assert_eq!(widths(&progress_bar(0.0, 0.0)), (0, 20));
    }

    #[test]
    fn exact_bounds() {
        assert_eq!(widths(&progress_bar(0.0, 100.0)), (0, 20));
        assert_eq!(widths(&progress_bar(100.0, 100.0)), (20, 0));
        assert_eq!(widths(&progress_bar(50.0, 100.0)), (10, 10));
    }

    #[test]
    fn negative_position_clamps_to_zero() {
        assert_eq!(widths(&progress_bar(-3.0, 100.0)), (0, 20));
    }
}

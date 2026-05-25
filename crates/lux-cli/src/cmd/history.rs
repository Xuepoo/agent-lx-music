use crate::library::db;
use anyhow::Result;
use colored::Colorize;

pub async fn run(limit: usize, json_out: bool) -> Result<()> {
    db::init_db()?;
    let entries = db::list_history_entries(limit)?;

    if json_out {
        println!("{}", serde_json::to_string_pretty(&entries)?);
        return Ok(());
    }

    if entries.is_empty() {
        println!("No recent play history found.");
        return Ok(());
    }

    println!("\nRecent plays:");
    for entry in entries {
        let rel_time = calculate_relative_time(&entry.played_at);

        let play_dur = if let Some(played) = entry.duration_played {
            let mins = played / 60;
            let secs = played % 60;
            if let Some(ref total) = entry.interval {
                format!("played {:02}:{:02} / {}", mins, secs, total)
            } else {
                format!("played {:02}:{:02}", mins, secs)
            }
        } else if let Some(ref total) = entry.interval {
            format!("played {}", total)
        } else {
            "played".to_string()
        };

        println!(
            "  {:<8}  {} — {} ({})",
            rel_time.dimmed(),
            entry.name.bold(),
            entry.singer.cyan(),
            play_dur.dimmed()
        );
    }
    println!();

    Ok(())
}

fn calculate_relative_time(played_at_str: &str) -> String {
    if let Ok(played_at) = chrono::DateTime::parse_from_rfc3339(played_at_str) {
        let now = chrono::Local::now();
        let played_at_local = played_at.with_timezone(&chrono::Local);
        let duration = now.signed_duration_since(played_at_local);

        if duration.num_weeks() > 0 {
            format!("{}w ago", duration.num_weeks())
        } else if duration.num_days() > 0 {
            format!("{}d ago", duration.num_days())
        } else if duration.num_hours() > 0 {
            format!("{}h ago", duration.num_hours())
        } else if duration.num_minutes() > 0 {
            format!("{}m ago", duration.num_minutes())
        } else {
            "just now".to_string()
        }
    } else {
        "unknown".to_string()
    }
}

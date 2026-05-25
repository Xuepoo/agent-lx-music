pub mod config;
pub mod source;

use crate::cli::Commands;
use anyhow::Result;

pub fn dispatch(command: Commands, json: bool) -> Result<()> {
    match command {
        Commands::Config { action } => {
            config::run(action, json)?;
        }
        Commands::Search {
            keyword,
            source,
            page,
            limit,
            id_only,
        } => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "message": "Search subcommand is not fully implemented yet in the skeleton phase",
                        "params": {
                            "keyword": keyword,
                            "source": source,
                            "page": page,
                            "limit": limit,
                            "id_only": id_only
                        }
                    })
                );
            } else {
                println!(
                    "Search: '{}' on source '{}' (page {}, limit {}) [skeleton phase placeholder].",
                    keyword, source, page, limit
                );
            }
        }
        Commands::Play {
            id_or_url,
            quality,
            from_playlist,
            shuffle,
        } => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "message": "Play subcommand is not fully implemented yet in the skeleton phase",
                        "params": {
                            "id_or_url": id_or_url,
                            "quality": quality,
                            "from_playlist": from_playlist,
                            "shuffle": shuffle
                        }
                    })
                );
            } else {
                println!(
                    "Play: {:?} with quality {:?} [skeleton phase placeholder].",
                    id_or_url, quality
                );
            }
        }
        Commands::Now => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "status": "stopped",
                        "message": "No playback is running [skeleton phase placeholder]"
                    })
                );
            } else {
                println!("♫ No song is playing currently.");
            }
        }
        Commands::Source { action } => {
            source::run(action, json)?;
        }
    }
    Ok(())
}

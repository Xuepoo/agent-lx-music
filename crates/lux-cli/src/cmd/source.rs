use crate::cli::SourceAction;
use crate::library::db::list_sources;
use crate::source::loader::add_source_script;
use anyhow::Result;
use colored::Colorize;
use tabled::{Table, Tabled};

#[derive(Tabled)]
struct SourceTableEntry {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Version")]
    version: String,
    #[tabled(rename = "Author")]
    author: String,
    #[tabled(rename = "Platforms")]
    platforms: String,
    #[tabled(rename = "Enabled")]
    enabled: String,
}

pub fn run(action: SourceAction, json: bool) -> Result<()> {
    match action {
        SourceAction::Add { path_or_url } => {
            // Ensure database schema exists
            crate::library::db::init_db()?;

            add_source_script(&path_or_url)?;
            if !json {
                println!(
                    "{} {} successfully registered!",
                    "✓".green().bold(),
                    "Custom music source".bold()
                );
            }
        }
        SourceAction::List => {
            crate::library::db::init_db()?;
            let entries = list_sources()?;

            if json {
                println!("{}", serde_json::to_string_pretty(&entries)?);
            } else if entries.is_empty() {
                println!("No custom music sources registered yet.");
                println!("Register one using: rlx source add <path_or_url>");
            } else {
                let table_data: Vec<SourceTableEntry> = entries
                    .into_iter()
                    .map(|entry| {
                        let platforms: Vec<String> =
                            serde_json::from_str(&entry.platforms).unwrap_or_default();
                        SourceTableEntry {
                            id: entry.id,
                            name: entry.name,
                            version: entry.version.unwrap_or_else(|| "N/A".to_string()),
                            author: entry.author.unwrap_or_else(|| "N/A".to_string()),
                            platforms: platforms.join(", "),
                            enabled: if entry.enabled {
                                "YES".green().to_string()
                            } else {
                                "NO".red().to_string()
                            },
                        }
                    })
                    .collect();

                let table = Table::new(table_data).to_string();
                println!("{}", table);
            }
        }
    }
    Ok(())
}

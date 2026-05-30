use crate::cli::SourceAction;
use crate::library::db::{delete_source, get_source, list_sources, update_source_script};
use crate::source::loader::add_source_script;
use crate::source::runtime::JsSandbox;
use anyhow::{Result, anyhow};
use colored::Colorize;
use md5::Digest;
use std::fs;
use std::path::Path;
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

#[derive(Tabled, serde::Serialize)]
struct TestResultEntry {
    #[tabled(rename = "Platform")]
    platform: String,
    #[tabled(rename = "Action")]
    action: String,
    #[tabled(rename = "Status")]
    status: String,
    #[tabled(rename = "Message/Details")]
    message: String,
}

#[allow(clippy::collapsible_if)]
pub async fn run(action: SourceAction, json: bool) -> Result<()> {
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
                println!("Register one using: alx source add <path_or_url>");
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
        SourceAction::Remove { id } => {
            crate::library::db::init_db()?;
            if let Some(entry) = get_source(&id)? {
                // Delete script file from disk
                let path = Path::new(&entry.script_path);
                if path.exists() {
                    let _ = fs::remove_file(path);
                }
                // Delete database registry
                delete_source(&id)?;

                if json {
                    println!("{}", serde_json::json!({ "status": "removed", "id": id }));
                } else {
                    println!(
                        "{} Source '{}' successfully removed.",
                        "✓".green().bold(),
                        id
                    );
                }
            } else {
                return Err(anyhow!("Source with ID '{}' not found.", id));
            }
        }
        SourceAction::Update { id, all } => {
            crate::library::db::init_db()?;
            let client = reqwest::Client::new();

            let targets = if all {
                list_sources()?
            } else if let Some(target_id) = id {
                if let Some(entry) = get_source(&target_id)? {
                    vec![entry]
                } else {
                    return Err(anyhow!("Source with ID '{}' not found.", target_id));
                }
            } else {
                return Err(anyhow!(
                    "Please specify a Source ID or use --all to update all."
                ));
            };

            let mut updated_count = 0;

            for entry in targets {
                let Some(url) = entry.source_url.clone() else {
                    if !all && !json {
                        println!("Source '{}' has no remote URL configured.", entry.id);
                    }
                    continue;
                };
                if !url.starts_with("http") {
                    continue;
                }

                if !json {
                    println!("Fetching updates for source '{}' from {}...", entry.id, url);
                }

                let fetch_res = client.get(&url).send().await?.text().await;

                match fetch_res {
                    Ok(new_script) => {
                        let digest = md5::Md5::digest(new_script.as_bytes());
                        let new_hash = format!("{:x}", digest);

                        if new_hash == entry.content_hash {
                            if !json {
                                println!("Source '{}' is already up to date.", entry.id);
                            }
                            continue;
                        }

                        // Write to disk
                        let _ = fs::write(&entry.script_path, &new_script);
                        // Update DB
                        let now = chrono::Local::now().to_rfc3339();
                        update_source_script(&entry.id, &new_hash, &now)?;
                        updated_count += 1;

                        if !json {
                            println!(
                                "{} Source '{}' updated successfully.",
                                "✓".green().bold(),
                                entry.id
                            );
                        }
                    }
                    Err(e) => {
                        if !json {
                            eprintln!("Failed to update source '{}': {}", entry.id, e);
                        }
                    }
                }
            }

            if json {
                println!(
                    "{}",
                    serde_json::json!({ "status": "updated", "count": updated_count })
                );
            }
        }
        SourceAction::Test {
            id,
            keyword,
            platform,
        } => {
            crate::library::db::init_db()?;
            let entry =
                get_source(&id)?.ok_or_else(|| anyhow!("Source with ID '{}' not found.", id))?;
            let script = fs::read_to_string(&entry.script_path)?;

            let sandbox = JsSandbox::new()?;
            let mut test_results = Vec::new();

            // Phase 1: Initialize Health Check
            let init_res = sandbox.execute_init(&script);
            let init_ok = init_res.is_ok();
            test_results.push(TestResultEntry {
                platform: "All".to_string(),
                action: "Initialize".to_string(),
                status: if init_ok {
                    "PASS".green().to_string()
                } else {
                    "FAIL".red().to_string()
                },
                message: match &init_res {
                    Ok(v) => format!(
                        "Name: {}, Version: {}",
                        v["name"].as_str().unwrap_or("N/A"),
                        v["version"].as_str().unwrap_or("N/A")
                    ),
                    Err(e) => e.to_string(),
                },
            });

            if init_ok {
                let init_val = init_res.as_ref().unwrap();
                let sources_obj = init_val.get("sources");

                let platforms_list: Vec<String> =
                    serde_json::from_str(&entry.platforms).unwrap_or_default();
                for plat in platforms_list {
                    if let Some(ref p_override) = platform {
                        if &plat != p_override {
                            continue;
                        }
                    }

                    // Check actions for this platform from inited data
                    let mut supports_search = false;
                    let mut supports_resolve = false;

                    if let Some(sources_map) = sources_obj.and_then(|s| s.as_object()) {
                        if let Some(plat_meta) = sources_map.get(&plat).and_then(|p| p.as_object())
                        {
                            if let Some(actions_arr) =
                                plat_meta.get("actions").and_then(|a| a.as_array())
                            {
                                for act in actions_arr {
                                    if let Some(act_str) = act.as_str() {
                                        if act_str == "musicSearch" {
                                            supports_search = true;
                                        }
                                        if act_str == "musicUrl" {
                                            supports_resolve = true;
                                        }
                                    }
                                }
                            } else {
                                // Default to true if actions are omitted
                                supports_search = true;
                                supports_resolve = true;
                            }
                        } else {
                            supports_search = true;
                            supports_resolve = true;
                        }
                    } else {
                        supports_search = true;
                        supports_resolve = true;
                    }

                    if supports_search {
                        // Phase 2: Search Verification
                        let search_sandbox = JsSandbox::new()?;
                        let search_res =
                            search_sandbox.execute_search(&script, &plat, &keyword, 1, 5);
                        let search_ok = search_res.is_ok();

                        test_results.push(TestResultEntry {
                            platform: plat.clone(),
                            action: "Search".to_string(),
                            status: if search_ok {
                                "PASS".green().to_string()
                            } else {
                                "FAIL".red().to_string()
                            },
                            message: match search_res {
                                Ok(ref s) => {
                                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(s) {
                                        let len =
                                            val["list"].as_array().map(|a| a.len()).unwrap_or(0);
                                        format!("Found {} songs", len)
                                    } else {
                                        "Invalid JSON search result format".to_string()
                                    }
                                }
                                Err(ref e) => e.to_string(),
                            },
                        });

                        // Phase 3: URL Resolution Verification
                        if search_ok && supports_resolve {
                            let search_json = search_res.unwrap();
                            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&search_json)
                            {
                                if let Some(first_song) =
                                    val["list"].as_array().and_then(|a| a.first())
                                {
                                    let songmid = first_song["songmid"]
                                        .as_str()
                                        .or_else(|| first_song["id"].as_str())
                                        .unwrap_or("")
                                        .to_string();

                                    let resolve_sandbox = JsSandbox::new()?;
                                    let resolve_res = resolve_sandbox.execute_resolve(
                                        &script,
                                        &plat,
                                        &songmid,
                                        "128k",
                                        first_song.clone(),
                                    );
                                    let resolve_ok = resolve_res.is_ok();

                                    test_results.push(TestResultEntry {
                                        platform: plat.clone(),
                                        action: "Resolve URL".to_string(),
                                        status: if resolve_ok {
                                            "PASS".green().to_string()
                                        } else {
                                            "FAIL".red().to_string()
                                        },
                                        message: match resolve_res {
                                            Ok(url) => {
                                                if url.trim().starts_with("http")
                                                    && !url.contains("horse.mp3")
                                                    && !url.contains("example.com")
                                                {
                                                    format!("Success: {:.40}...", url)
                                                } else {
                                                    format!(
                                                        "Hijacked / Invalid URL resolved: {}",
                                                        url
                                                    )
                                                }
                                            }
                                            Err(e) => e.to_string(),
                                        },
                                    });
                                }
                            }
                        }
                    } else {
                        test_results.push(TestResultEntry {
                            platform: plat.clone(),
                            action: "Search".to_string(),
                            status: "SKIP".yellow().to_string(),
                            message: "Not supported by source actions".to_string(),
                        });
                        test_results.push(TestResultEntry {
                            platform: plat.clone(),
                            action: "Resolve URL".to_string(),
                            status: "SKIP".yellow().to_string(),
                            message: "Requires Search support to retrieve song info".to_string(),
                        });
                    }
                }
            }

            if json {
                let serialized = serde_json::to_string_pretty(&test_results)?;
                println!("{}", serialized);
            } else {
                println!(
                    "\n{} testing source '{}':\n",
                    "⚡".yellow().bold(),
                    id.cyan()
                );
                let table = Table::new(test_results).to_string();
                println!("{}", table);
            }
        }
        SourceAction::Info { id } => {
            crate::library::db::init_db()?;
            if let Some(entry) = get_source(&id)? {
                let size = fs::metadata(&entry.script_path)
                    .map(|m| m.len())
                    .unwrap_or(0);
                if json {
                    let mut val = serde_json::to_value(&entry)?;
                    val["file_size_bytes"] = serde_json::json!(size);
                    println!("{}", serde_json::to_string_pretty(&val)?);
                } else {
                    println!("{}", "── Custom Music Source Info ──".bold().green());
                    println!("ID:           {}", entry.id.cyan());
                    println!("Name:         {}", entry.name.bold());
                    println!(
                        "Version:      {}",
                        entry.version.unwrap_or_else(|| "N/A".to_string())
                    );
                    println!(
                        "Author:       {}",
                        entry.author.unwrap_or_else(|| "N/A".to_string())
                    );
                    println!(
                        "Homepage:     {}",
                        entry.homepage.unwrap_or_else(|| "N/A".to_string())
                    );
                    println!(
                        "Repository:   {}",
                        entry.repository.unwrap_or_else(|| "N/A".to_string())
                    );
                    println!("Script Path:  {}", entry.script_path);
                    println!("File Size:    {} bytes", size);
                    println!(
                        "Remote URL:   {}",
                        entry.source_url.unwrap_or_else(|| "N/A".to_string())
                    );
                    println!("Content MD5:  {}", entry.content_hash);
                    let platforms: Vec<String> =
                        serde_json::from_str(&entry.platforms).unwrap_or_default();
                    println!("Platforms:    {}", platforms.join(", "));
                    println!(
                        "Enabled:      {}",
                        if entry.enabled {
                            "YES".green()
                        } else {
                            "NO".red()
                        }
                    );
                    println!("Created At:   {}", entry.created_at);
                    println!("Updated At:   {}", entry.updated_at);
                }
            } else {
                return Err(anyhow!("Source with ID '{}' not found.", id));
            }
        }
    }
    Ok(())
}

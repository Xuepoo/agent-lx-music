use crate::library::db::{SearchCacheEntry, insert_search_cache};
use anyhow::Result;
use colored::Colorize;
use lux_core::types::{MusicInfo, Source};
use md5::Digest;
use tabled::{Table, Tabled};

#[derive(Tabled)]
struct SearchTableEntry {
    #[tabled(rename = "Index")]
    index: usize,
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Title")]
    title: String,
    #[tabled(rename = "Artist")]
    singer: String,
    #[tabled(rename = "Album")]
    album: String,
    #[tabled(rename = "Duration")]
    duration: String,
    #[tabled(rename = "Platform")]
    source: String,
}

#[allow(
    clippy::collapsible_if,
    clippy::manual_map,
    clippy::manual_range_contains
)]
pub async fn run(
    keyword: String,
    source_str: String,
    page: usize,
    limit: usize,
    id_only: bool,
    json: bool,
) -> Result<()> {
    // 1. Determine which platforms to search
    let platforms = if source_str == "all" {
        vec![
            "wy".to_string(),
            "kw".to_string(),
            "kg".to_string(),
            "tx".to_string(),
            "mg".to_string(),
        ]
    } else {
        vec![source_str.clone()]
    };

    let mut tasks = Vec::new();

    // A. Native search tasks
    for platform in &platforms {
        let src = Source::from(platform.clone());
        let keyword_clone = keyword.clone();
        let task = tokio::spawn(async move {
            let _ = (&src, &keyword_clone);
            #[cfg(feature = "lux-native")]
            {
                if let Some(native_src) = lux_native::get_native_source(&src) {
                    return native_src
                        .search(&keyword_clone, page, limit)
                        .await
                        .ok()
                        .map(|r| r.list);
                }
            }
            None
        });
        tasks.push(task);
    }

    // B. JS dynamic search tasks
    let db_entries = crate::library::db::list_sources().unwrap_or_default();
    for entry in db_entries {
        if !entry.enabled {
            continue;
        }
        let supported_platforms: Vec<String> =
            serde_json::from_str(&entry.platforms).unwrap_or_default();
        for platform in &platforms {
            if supported_platforms.contains(platform) {
                let keyword_clone = keyword.clone();
                let platform_clone = platform.clone();
                let script_path = entry.script_path.clone();
                let task = tokio::spawn(async move {
                    let Ok(script) = std::fs::read_to_string(&script_path) else {
                        return None;
                    };
                    let Ok(sandbox) = crate::source::runtime::JsSandbox::new() else {
                        return None;
                    };
                    if let Ok(res_str) = sandbox.execute_search(
                        &script,
                        &platform_clone,
                        &keyword_clone,
                        page,
                        limit,
                    ) {
                        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&res_str) {
                            let mut list = Vec::new();
                            if let Some(arr) = val["list"].as_array() {
                                for item in arr {
                                    let songmid = item["songmid"]
                                        .as_str()
                                        .or_else(|| item["id"].as_str())
                                        .unwrap_or("")
                                        .to_string();
                                    if songmid.is_empty() {
                                        continue;
                                    }
                                    let name =
                                        item["name"].as_str().unwrap_or("Unknown").to_string();
                                    let singer = item["singer"]
                                        .as_str()
                                        .or_else(|| item["artist"].as_str())
                                        .unwrap_or("Unknown")
                                        .to_string();
                                    let album_name = item["albumName"]
                                        .as_str()
                                        .or_else(|| item["album"].as_str())
                                        .map(|s| s.to_string());
                                    let album_id = item["albumId"]
                                        .as_str()
                                        .or_else(|| item["albumid"].as_str())
                                        .map(|s| s.to_string());
                                    let pic_url = item["picUrl"]
                                        .as_str()
                                        .or_else(|| item["img"].as_str())
                                        .map(|s| s.to_string());

                                    let interval = if let Some(sec_str) = item["interval"].as_str()
                                    {
                                        Some(sec_str.to_string())
                                    } else if let Some(sec_val) = item["interval"].as_i64() {
                                        Some(format!("{:02}:{:02}", sec_val / 60, sec_val % 60))
                                    } else {
                                        None
                                    };

                                    list.push(MusicInfo {
                                        songmid,
                                        name,
                                        singer,
                                        source: Source::from(platform_clone.clone()),
                                        album_name,
                                        album_id,
                                        interval,
                                        pic_url,
                                        hash: item["hash"].as_str().map(|s| s.to_string()),
                                        extra: None,
                                    });
                                }
                            }
                            return Some(list);
                        }
                    }
                    None
                });
                tasks.push(task);
            }
        }
    }

    // C. Gather results
    let mut raw_music_list = Vec::new();
    for task in tasks {
        if let Ok(Some(songs)) = task.await {
            raw_music_list.extend(songs);
        }
    }

    // 2. Cross-platform Deduplication
    let parse_duration_to_seconds = |interval: Option<&str>| -> i64 {
        let Some(s) = interval else {
            return 0;
        };
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() == 2 {
            let mins: i64 = parts[0].parse().unwrap_or(0);
            let secs: i64 = parts[1].parse().unwrap_or(0);
            mins * 60 + secs
        } else {
            s.parse::<i64>().unwrap_or(0)
        }
    };

    let normalize_text = |t: &str| -> String {
        t.to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric() || c.is_whitespace())
            .collect::<String>()
            .trim()
            .to_string()
    };

    let mut dedup_map: std::collections::HashMap<(String, String), MusicInfo> =
        std::collections::HashMap::new();

    for song in raw_music_list {
        let name_norm = normalize_text(&song.name);
        let singer_norm = normalize_text(&song.singer);
        let key = (name_norm, singer_norm);

        if let Some(existing) = dedup_map.get_mut(&key) {
            // Pick the richer metadata (pic_url exists, longer duration)
            let existing_dur = parse_duration_to_seconds(existing.interval.as_deref());
            let current_dur = parse_duration_to_seconds(song.interval.as_deref());

            let existing_has_pic = existing.pic_url.is_some();
            let current_has_pic = song.pic_url.is_some();

            if (!existing_has_pic && current_has_pic)
                || (existing_has_pic == current_has_pic && current_dur > existing_dur)
            {
                *existing = song;
            }
        } else {
            dedup_map.insert(key, song);
        }
    }

    let mut merged_list: Vec<MusicInfo> = dedup_map.into_values().collect();

    // 3. Dynamic Relevance Ranking
    let compute_relevance_score = |song: &MusicInfo, keyword: &str| -> i32 {
        let name_lower = song.name.to_lowercase();
        let singer_lower = song.singer.to_lowercase();
        let kw_lower = keyword.to_lowercase();

        let mut score = 0i32;

        // A. Title match
        if name_lower == kw_lower {
            score += 1000;
        } else if name_lower.starts_with(&kw_lower) {
            score += 500;
        } else if name_lower.contains(&kw_lower) {
            score += 200;
        }

        // B. Artist match
        if singer_lower == kw_lower {
            score += 600;
        } else if singer_lower.contains(&kw_lower) {
            score += 150;
        }

        // C. Mixed title + artist match
        if kw_lower.contains(&name_lower) && kw_lower.contains(&singer_lower) {
            score += 800;
        }

        // D. Penalty filters (降权 DJ / Remix / Live / Cover)
        if name_lower.contains("dj") || name_lower.contains("remix") || name_lower.contains("混音")
        {
            score -= 800;
        }
        if name_lower.contains("live") || name_lower.contains("现场") {
            score -= 500;
        }
        if name_lower.contains("cover")
            || name_lower.contains("翻唱")
            || name_lower.contains("伴奏")
            || name_lower.contains("inst")
        {
            score -= 600;
        }

        // E. Duration sanity weighting (BUG-01 馬叫声测试音源过滤)
        let dur = parse_duration_to_seconds(song.interval.as_deref());
        if dur > 0 {
            if dur < 30 {
                // Penalize dummy sound clips heavily
                score -= 1000;
            } else if dur >= 180 && dur <= 360 {
                // Typical pop songs get dynamic bonus
                score += 100;
            }
        } else {
            score -= 50;
        }

        // F. Cover Art bonus
        if song.pic_url.is_some() {
            score += 50;
        }

        score
    };

    merged_list.sort_by(|a, b| {
        let score_a = compute_relevance_score(a, &keyword);
        let score_b = compute_relevance_score(b, &keyword);
        score_b.cmp(&score_a) // Descending relevance
    });

    // Truncate to user-defined limit
    if merged_list.len() > limit {
        merged_list.truncate(limit);
    }

    // 4. Cache results in local SQLite database
    crate::library::db::init_db()?;
    let mut cache_entries = Vec::new();

    for song in &merged_list {
        // Generate stable 8-character CLI ID
        let hash_input = format!("{}-{}", song.source.as_str(), song.songmid);
        let digest = md5::Md5::digest(hash_input.as_bytes());
        let cli_id = format!("{:x}", digest)[..8].to_string();

        let entry = SearchCacheEntry {
            cli_id: cli_id.clone(),
            song_id: song.songmid.clone(),
            name: song.name.clone(),
            singer: song.singer.clone(),
            source: song.source.as_str().to_string(),
            interval: song.interval.clone(),
            album_name: song.album_name.clone(),
            album_id: song.album_id.clone(),
            pic_url: song.pic_url.clone(),
            songmid: Some(song.songmid.clone()),
            hash: song.hash.clone(),
            extra: song.extra.clone(),
        };

        let _ = insert_search_cache(&entry);
        cache_entries.push(entry);
    }

    // 5. Present outputs
    if id_only {
        for entry in &cache_entries {
            println!("{}", entry.cli_id);
        }
    } else if json {
        let serialized = serde_json::to_string_pretty(&cache_entries)?;
        println!("{}", serialized);
    } else if cache_entries.is_empty() {
        println!("No songs found for '{}'.", keyword.bold());
    } else {
        let table_data: Vec<SearchTableEntry> = cache_entries
            .into_iter()
            .enumerate()
            .map(|(i, entry)| SearchTableEntry {
                index: i + 1,
                id: entry.cli_id.cyan().to_string(),
                title: entry.name,
                singer: entry.singer,
                album: entry.album_name.unwrap_or_else(|| "N/A".to_string()),
                duration: entry.interval.unwrap_or_else(|| "00:00".to_string()),
                source: match entry.source.as_str() {
                    "wy" => "NetEase".red().bold().to_string(),
                    "kw" => "Kuwo".blue().bold().to_string(),
                    "kg" => "Kugou".green().bold().to_string(),
                    "tx" => "QQ".yellow().bold().to_string(),
                    "mg" => "Migu".magenta().bold().to_string(),
                    other => other.to_string(),
                },
            })
            .collect();

        let table = Table::new(table_data)
            .with(tabled::settings::Style::rounded())
            .to_string();

        println!("\nSearch results for '{}':\n", keyword.bold().yellow());
        println!("{}", table);
        println!(
            "\nUse {} to play a song, e.g. {}\n",
            "alx play <id>".bold(),
            "alx play <id>".cyan()
        );
    }

    Ok(())
}

use crate::library::db::{SearchCacheEntry, insert_search_cache};
use anyhow::Result;
use colored::Colorize;
use lux_core::types::{MusicInfo, SearchResult, Source};
use md5::Digest;
use std::collections::HashSet;
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

pub async fn run(
    keyword: String,
    source_str: String,
    page: usize,
    limit: usize,
    id_only: bool,
    json: bool,
) -> Result<()> {
    // 1. Determine which sources to query
    let sources_to_query = if source_str == "all" {
        vec![Source::NetEase, Source::Kuwo]
    } else {
        vec![Source::from(source_str.clone())]
    };

    // 2. Fetch results in parallel using tokio tasks
    let mut tasks = Vec::new();
    for src in sources_to_query {
        let keyword_clone = keyword.clone();
        let task = tokio::spawn(async move {
            let _ = &keyword_clone;
            let _ = &src;
            let _ = page;
            let _ = limit;
            #[cfg(feature = "lux-native")]
            {
                if let Some(native_src) = lux_native::get_native_source(&src) {
                    return native_src.search(&keyword_clone, page, limit).await.ok();
                }
            }
            None
        });
        tasks.push(task);
    }

    let mut all_results: Vec<SearchResult> = Vec::new();
    for task in tasks {
        if let Ok(Some(res)) = task.await {
            all_results.push(res);
        }
    }

    // 3. Merge and deduplicate results
    let mut merged_list: Vec<MusicInfo> = Vec::new();
    let mut seen = HashSet::new();

    for res in all_results {
        for song in res.list {
            let unique_key = format!("{}-{}", song.source, song.songmid);
            if !seen.contains(&unique_key) {
                seen.insert(unique_key);
                merged_list.push(song);
            }
        }
    }

    // 4. Rank results based on keyword match relevance
    merged_list.sort_by_key(|song| {
        let name_lower = song.name.to_lowercase();
        let singer_lower = song.singer.to_lowercase();
        let kw_lower = keyword.to_lowercase();

        let mut score = 0i32;
        if name_lower == kw_lower {
            score -= 100;
        } else if name_lower.starts_with(&kw_lower) {
            score -= 50;
        } else if name_lower.contains(&kw_lower) {
            score -= 20;
        }

        if singer_lower == kw_lower {
            score -= 80;
        } else if singer_lower.contains(&kw_lower) {
            score -= 10;
        }
        score
    });

    // Truncate to limit if needed
    if merged_list.len() > limit {
        merged_list.truncate(limit);
    }

    // 5. Cache results in database and map to SearchCacheEntry
    crate::library::db::init_db()?;
    let mut cache_entries = Vec::new();

    for song in &merged_list {
        // Generate stable 8-char CLI ID from md5 hash of platform + songmid
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

    // 6. Handle outputs
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
            "rlx play <id>".bold(),
            "rlx play <id>".cyan()
        );
    }

    Ok(())
}

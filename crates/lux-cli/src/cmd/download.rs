#![allow(clippy::collapsible_if, clippy::collapsible_else_if)]
use crate::cli::DownloadAction;
use crate::library::db::{self, DownloadEntry};
use crate::source::SourceManager;
use anyhow::{Result, anyhow};
use colored::Colorize;
use id3::frame::{Content as Id3Content, Picture as Id3Picture, PictureType as Id3PictureType};
use id3::{Frame as Id3Frame, Tag as Id3Tag, TagLike as Id3TagLike};
use metaflac::Tag as FlacTag;
use metaflac::block::PictureType as FlacPictureType;
use std::fs::{self, File};
use std::io::{Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;

#[derive(tabled::Tabled)]
struct DownloadStatusTableEntry {
    #[tabled(rename = "ID")]
    id: i64,
    #[tabled(rename = "Title")]
    title: String,
    #[tabled(rename = "Singer")]
    singer: String,
    #[tabled(rename = "Quality")]
    quality: String,
    #[tabled(rename = "Status")]
    status: String,
    #[tabled(rename = "Progress")]
    progress: String,
    #[tabled(rename = "Progress Bar")]
    progress_bar: String,
}

#[derive(tabled::Tabled)]
struct DownloadHistoryTableEntry {
    #[tabled(rename = "ID")]
    id: i64,
    #[tabled(rename = "Title")]
    title: String,
    #[tabled(rename = "Singer")]
    singer: String,
    #[tabled(rename = "Quality")]
    quality: String,
    #[tabled(rename = "Status")]
    status: String,
    #[tabled(rename = "Error")]
    error: String,
    #[tabled(rename = "Created At")]
    created_at: String,
}

pub async fn run(action: DownloadAction, json: bool) -> Result<()> {
    match action {
        DownloadAction::Add { ids, quality, file } => {
            let config = lux_core::config::Config::load().unwrap_or_default();
            let selected_quality = quality
                .as_ref()
                .and_then(|q| std::str::FromStr::from_str(q).ok())
                .unwrap_or(config.source.default_quality);

            let mut added_songs = Vec::new();

            if let Some(file_path_str) = file {
                let file_path = std::path::Path::new(&file_path_str);
                if !file_path.exists() {
                    return Err(anyhow!("Playlist file '{}' does not exist.", file_path_str));
                }
                let content = fs::read_to_string(file_path)?;
                let imported_tracks =
                    crate::library::playlist_parser::parse_universal_playlist(&content);
                if imported_tracks.is_empty() {
                    return Err(anyhow!(
                        "No valid tracks found in playlist file '{}'.",
                        file_path_str
                    ));
                }

                if !json {
                    println!(
                        "{} Parsing playlist and matching {} songs...",
                        "⚡".yellow().bold(),
                        imported_tracks.len()
                    );
                }

                let parsed_quality = quality
                    .as_ref()
                    .and_then(|q| q.parse::<lux_core::types::Quality>().ok());

                for (idx, track) in imported_tracks.iter().enumerate() {
                    if !json {
                        println!(
                            "  [{}/{}] Matching: {} — {} ...",
                            idx + 1,
                            imported_tracks.len(),
                            track.title.bold(),
                            track.artist.cyan()
                        );
                    }

                    match crate::library::playlist_parser::resolve_imported_track(
                        track,
                        parsed_quality,
                    )
                    .await
                    {
                        Ok(Some(entry)) => {
                            let insert_res = db::insert_download(
                                &entry.song_id,
                                &entry.source,
                                &entry.name,
                                &entry.singer,
                                selected_quality.as_str(),
                            );
                            if insert_res.is_ok() {
                                added_songs.push(entry);
                            }
                        }
                        _ => {
                            if !json {
                                println!(
                                    "  {} Failed to match or resolve this song.",
                                    "✗".red().bold()
                                );
                            }
                        }
                    }
                }
            } else {
                for cli_id in ids {
                    let song_entry = db::get_song_by_cli_id(&cli_id)?.ok_or_else(|| {
                        anyhow!("CLI ID '{}' not found in cache. Search first.", cli_id)
                    })?;

                    db::insert_download(
                        &song_entry.song_id,
                        &song_entry.source,
                        &song_entry.name,
                        &song_entry.singer,
                        selected_quality.as_str(),
                    )?;

                    added_songs.push(song_entry);
                }
            }

            // Lazy spawn daemon if not running
            if !added_songs.is_empty() {
                ensure_daemon_running()?;
            }

            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "status": "added",
                        "songs": added_songs.iter().map(|s| s.name.clone()).collect::<Vec<String>>()
                    })
                );
            } else {
                for song in &added_songs {
                    println!(
                        "✓ Added \"{} - {}\" to download queue.",
                        song.singer, song.name
                    );
                }
                if !added_songs.is_empty() {
                    println!("Background download daemon triggered.");
                } else {
                    println!("No songs were added to the download queue.");
                }
            }
        }
        DownloadAction::Daemon => {
            run_daemon().await?;
        }
        DownloadAction::Status => {
            let pending = db::list_downloads(Some("pending"))?;
            let downloading = db::list_downloads(Some("downloading"))?;
            let mut active = downloading;
            active.extend(pending);

            if json {
                println!("{}", serde_json::to_string_pretty(&active)?);
            } else {
                if active.is_empty() {
                    println!("No active or pending download tasks.");
                } else {
                    let mut data = Vec::new();
                    for dl in active {
                        let bar_len = 15;
                        let filled = (dl.progress * bar_len as f64) as usize;
                        let bar =
                            format!("[{}{}]", "=".repeat(filled), " ".repeat(bar_len - filled));
                        let progress_pct = format!("{:.1}%", dl.progress * 100.0);
                        data.push(DownloadStatusTableEntry {
                            id: dl.id,
                            title: dl.name,
                            singer: dl.singer,
                            quality: dl.quality,
                            status: dl.status,
                            progress: progress_pct,
                            progress_bar: bar,
                        });
                    }
                    let mut table = tabled::Table::new(data);
                    table.with(tabled::settings::Style::rounded());
                    println!("{}", table);
                }
            }
        }
        DownloadAction::List => {
            let completed = db::list_downloads(Some("completed"))?;
            let failed = db::list_downloads(Some("failed"))?;
            let mut history = completed;
            history.extend(failed);

            if json {
                println!("{}", serde_json::to_string_pretty(&history)?);
            } else {
                if history.is_empty() {
                    println!("No download history found.");
                } else {
                    let mut data = Vec::new();
                    for dl in history {
                        data.push(DownloadHistoryTableEntry {
                            id: dl.id,
                            title: dl.name,
                            singer: dl.singer,
                            quality: dl.quality,
                            status: dl.status,
                            error: dl.error_message.unwrap_or_default(),
                            created_at: dl.created_at,
                        });
                    }
                    let mut table = tabled::Table::new(data);
                    table.with(tabled::settings::Style::rounded());
                    println!("{}", table);
                }
            }
        }
        DownloadAction::Retry { ids } => {
            for id in ids {
                db::retry_download(id)?;
            }
            ensure_daemon_running()?;
            if json {
                println!("{}", serde_json::json!({ "status": "retried" }));
            } else {
                println!("✓ Selected tasks re-queued for download. Daemon triggered.");
            }
        }
    }
    Ok(())
}

pub fn ensure_daemon_running() -> Result<()> {
    let paths = lux_core::config::resolve_paths();
    let pid_file = paths.cache_dir.join("download.pid");

    let need_spawn = if pid_file.exists() {
        if let Ok(pid_str) = fs::read_to_string(&pid_file) {
            if let Ok(pid) = pid_str.trim().parse::<i32>() {
                // Check if pid is running in system (using kill(pid, 0) logic or standard command)
                let status = Command::new("kill")
                    .arg("-0")
                    .arg(pid.to_string())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
                if let Ok(exit_status) = status {
                    !exit_status.success()
                } else {
                    true
                }
            } else {
                true
            }
        } else {
            true
        }
    } else {
        true
    };

    if need_spawn {
        let exe = std::env::current_exe()?;
        let _child = Command::new(exe)
            .arg("download")
            .arg("daemon")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;
    }

    Ok(())
}

async fn run_daemon() -> Result<()> {
    // 1. Reset stale downloading state tasks back to pending
    let _ = db::reset_downloading_to_pending();

    let paths = lux_core::config::resolve_paths();
    let pid_file = paths.cache_dir.join("download.pid");

    if let Some(parent) = pid_file.parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(&pid_file, std::process::id().to_string())?;

    let config = lux_core::config::Config::load().unwrap_or_default();
    let max_concurrent = config.download.max_concurrent;
    let sem = Arc::new(Semaphore::new(max_concurrent));

    let mut last_active = Instant::now();
    let idle_timeout = Duration::from_secs(60);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(config.download.timeout))
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .build()?;
    let source_manager = Arc::new(SourceManager::new());

    loop {
        let pending_tasks = db::list_downloads(Some("pending"))?;
        if pending_tasks.is_empty() {
            let downloading_tasks = db::list_downloads(Some("downloading"))?;
            if downloading_tasks.is_empty() {
                if last_active.elapsed() >= idle_timeout {
                    // Daemon idle timeout - gracefully exit
                    let _ = fs::remove_file(&pid_file);
                    break;
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
                continue;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
            continue;
        }

        // Active task found
        last_active = Instant::now();

        for task in pending_tasks {
            let permit = sem.clone().acquire_owned().await?;
            let client = client.clone();
            let sm = source_manager.clone();

            tokio::spawn(async move {
                let _permit = permit;
                let _ = process_single_task(task, client, sm).await;
            });
        }

        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    Ok(())
}

async fn process_single_task(
    task: DownloadEntry,
    client: reqwest::Client,
    sm: Arc<SourceManager>,
) -> Result<()> {
    db::update_download_status(task.id, "downloading", None)?;

    use lux_core::types::Quality;
    let initial_quality = std::str::FromStr::from_str(&task.quality).unwrap_or(Quality::Q320k);
    let qualities_to_try = get_fallback_qualities(initial_quality);

    let mut download_result = Err(anyhow!("No qualities to try"));
    let mut final_successful_quality = initial_quality;

    for &quality in &qualities_to_try {
        let _ = db::update_download_quality(task.id, quality.as_str());

        match execute_download_with_quality(&task, quality, &client, &sm).await {
            Ok(path) => {
                download_result = Ok(path);
                final_successful_quality = quality;
                break;
            }
            Err(e) => {
                download_result = Err(e);
            }
        }
    }

    match download_result {
        Ok(final_path) => {
            // Update to final successful quality and complete status
            let _ = db::update_download_quality(task.id, final_successful_quality.as_str());
            db::update_download_status(task.id, "completed", None)?;

            // Post-download Beets Import Hook
            let config = lux_core::config::Config::load().unwrap_or_default();
            if config.download.beet_import {
                let _ = Command::new("beet")
                    .arg("import")
                    .arg("--quiet")
                    .arg("--copy")
                    .arg(final_path)
                    .spawn();
            }
        }
        Err(e) => {
            db::update_download_status(task.id, "failed", Some(&e.to_string()))?;
        }
    }

    Ok(())
}

async fn execute_download_with_quality(
    task: &DownloadEntry,
    quality: lux_core::types::Quality,
    client: &reqwest::Client,
    sm: &SourceManager,
) -> Result<PathBuf> {
    let config = lux_core::config::Config::load().unwrap_or_default();
    let paths = lux_core::config::resolve_paths();

    let output_dir = config.get_resolved_download_dir();
    let _ = fs::create_dir_all(&output_dir);

    // Resolve downloadable Stream URL
    let stream_url = sm.resolve_url(&task.source, &task.song_id, quality)?;

    // Determine target extension
    let extension = if stream_url.contains(".flac") {
        "flac"
    } else {
        "mp3"
    };

    // Atomically write into a .part file inside cache dir
    let part_filename = format!("{}_{}.part", task.song_id, quality.as_str());
    let part_path = paths.cache_dir.join(&part_filename);

    let mut file = File::options()
        .create(true)
        .write(true)
        .read(true)
        .truncate(false)
        .open(&part_path)?;

    let mut existing_bytes = file.metadata()?.len();

    // Resumable HTTP Range requests
    let mut request = client.get(&stream_url);

    let support_range = if existing_bytes > 0 {
        if let Ok(head_res) = client.head(&stream_url).send().await {
            if let Some(accept) = head_res.headers().get(reqwest::header::ACCEPT_RANGES) {
                accept == "bytes"
            } else {
                false
            }
        } else {
            false
        }
    } else {
        false
    };

    if support_range && existing_bytes > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={}-", existing_bytes));
        file.seek(SeekFrom::End(0))?;
    } else {
        existing_bytes = 0;
        file.set_len(0)?;
        file.seek(SeekFrom::Start(0))?;
    }

    let mut response = request.send().await?;
    if !response.status().is_success() && response.status() != reqwest::StatusCode::PARTIAL_CONTENT
    {
        return Err(anyhow!(
            "Failed to start download stream: HTTP status {}",
            response.status()
        ));
    }

    let content_len = response.content_length().unwrap_or(0);
    let total_bytes = content_len + existing_bytes;

    let mut bytes_downloaded = existing_bytes;

    // Stream download loop
    while let Some(chunk) = response.chunk().await? {
        file.write_all(&chunk)?;
        bytes_downloaded += chunk.len() as u64;

        if total_bytes > 0 {
            let progress = bytes_downloaded as f64 / total_bytes as f64;
            db::update_download_progress(task.id, progress, bytes_downloaded, total_bytes)?;
        }
    }

    file.flush()?;
    drop(file);

    // Fetch Lyrics LRC
    let lyric_info = if config.download.embed_lyrics {
        sm.resolve_lyric(&task.source, &task.song_id).ok()
    } else {
        None
    };

    // Fetch Cover Art APIC
    let cover_bytes = if config.download.embed_cover {
        if let Some(song_cache) = db::get_song_from_cache(&task.song_id, &task.source)? {
            if let Some(ref pic_url) = song_cache.pic_url {
                if let Ok(pic_res) = client.get(pic_url).send().await {
                    pic_res.bytes().await.ok().map(|b| b.to_vec())
                } else {
                    None
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

    // Metadata Tagging Engine
    if config.download.embed_metadata {
        if extension == "flac" {
            let mut tag = FlacTag::read_from_path(&part_path)?;
            let vorbis = tag.vorbis_comments_mut();
            vorbis.set_title(vec![task.name.clone()]);
            vorbis.set_artist(vec![task.singer.clone()]);

            if let Some(song_cache) = db::get_song_from_cache(&task.song_id, &task.source)? {
                if let Some(ref album) = song_cache.album_name {
                    vorbis.set_album(vec![album.clone()]);
                }
            }

            if let Some(ref lrc) = lyric_info {
                vorbis.set("LYRICS", vec![lrc.lyric.clone()]);
            }

            if let Some(ref cov) = cover_bytes {
                tag.add_picture("image/jpeg", FlacPictureType::CoverFront, cov.clone());
            }

            tag.save()?;
        } else {
            // MP3 ID3v2 Tagging
            let mut tag = Id3Tag::read_from_path(&part_path).unwrap_or_default();
            tag.set_title(task.name.clone());
            tag.set_artist(task.singer.clone());

            if let Some(song_cache) = db::get_song_from_cache(&task.song_id, &task.source)? {
                if let Some(ref album) = song_cache.album_name {
                    tag.set_album(album.clone());
                }
            }

            if let Some(ref lrc) = lyric_info {
                let lyric_content = id3::frame::Lyrics {
                    lang: "eng".to_string(),
                    description: "".to_string(),
                    text: lrc.lyric.clone(),
                };
                tag.add_frame(Id3Frame::with_content(
                    "USLT",
                    Id3Content::Lyrics(lyric_content),
                ));
            }

            if let Some(ref cov) = cover_bytes {
                let pic = Id3Picture {
                    mime_type: "image/jpeg".to_string(),
                    picture_type: Id3PictureType::CoverFront,
                    description: "Cover".to_string(),
                    data: cov.clone(),
                };
                tag.add_frame(Id3Frame::with_content("APIC", Id3Content::Picture(pic)));
            }

            tag.write_to_path(&part_path, id3::Version::Id3v24)?;
        }
    }

    // Rename atomically to the final filename in output_dir
    let clean_title = task.name.replace(['/', '\\'], "-");
    let clean_singer = task.singer.replace(['/', '\\'], "-");
    let final_name = config
        .download
        .filename_template
        .replace("{singer}", &clean_singer)
        .replace("{title}", &clean_title);
    let final_filename = format!("{}.{}", final_name, extension);
    let final_path = output_dir.join(&final_filename);

    if let Err(e) = fs::rename(&part_path, &final_path) {
        if e.raw_os_error() == Some(18) {
            fs::copy(&part_path, &final_path)?;
            fs::remove_file(&part_path)?;
        } else {
            return Err(e.into());
        }
    }

    Ok(final_path)
}

pub fn get_fallback_qualities(initial: lux_core::types::Quality) -> Vec<lux_core::types::Quality> {
    use lux_core::types::Quality;
    let all_qualities = [
        Quality::Flac24bit,
        Quality::Flac,
        Quality::Q320k,
        Quality::Q192k,
        Quality::Q128k,
    ];

    let mut qualities_to_try = vec![initial];
    if let Some(pos) = all_qualities.iter().position(|&q| q == initial) {
        for &q in &all_qualities[pos + 1..] {
            qualities_to_try.push(q);
        }
    }
    qualities_to_try
}

#[cfg(test)]
mod tests {
    use super::*;
    use lux_core::types::Quality;

    #[test]
    fn test_fallback_qualities() {
        // 1. flac24bit should try all in descending order
        let chain_24 = get_fallback_qualities(Quality::Flac24bit);
        assert_eq!(
            chain_24,
            vec![
                Quality::Flac24bit,
                Quality::Flac,
                Quality::Q320k,
                Quality::Q192k,
                Quality::Q128k
            ]
        );

        // 2. Q320k should start with 320k and fallback to 192k -> 128k
        let chain_320 = get_fallback_qualities(Quality::Q320k);
        assert_eq!(
            chain_320,
            vec![Quality::Q320k, Quality::Q192k, Quality::Q128k]
        );

        // 3. Q128k should only have 128k
        let chain_128 = get_fallback_qualities(Quality::Q128k);
        assert_eq!(chain_128, vec![Quality::Q128k]);
    }
}

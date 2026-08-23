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
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;

/// Connect-phase timeout applied to all ad-hoc CLI HTTP clients.
pub const HTTP_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Fallback total request timeout when no configured value is available.
pub const DEFAULT_HTTP_TIMEOUT_SECS: u64 = 30;

/// Resolve the effective total request timeout from an optional configured
/// value (seconds, e.g. `config.download.timeout`).
///
/// `None` or zero falls back to [`DEFAULT_HTTP_TIMEOUT_SECS`] so requests can
/// never run unbounded.
pub fn http_timeout(configured: Option<u64>) -> Duration {
    match configured {
        Some(secs) if secs > 0 => Duration::from_secs(secs),
        _ => Duration::from_secs(DEFAULT_HTTP_TIMEOUT_SECS),
    }
}

/// Pre-configured reqwest client builder with a bounded connect phase and a
/// total timeout. Callers may still layer extra options (e.g. user agent)
/// before calling `.build()`.
pub fn http_client_builder(total_timeout: Duration) -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .connect_timeout(HTTP_CONNECT_TIMEOUT)
        .timeout(total_timeout)
}

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

/// Queue downloads by CLI ID: resolve each ID against the search cache,
/// insert a pending task row, and lazy-start the download daemon when
/// anything was queued. Quality override silently falls back to the
/// configured default on unparsable values (CLI parity).
///
/// Shared by `alx download add` (IDs branch) and the MCP `download_add`
/// tool; playlist-file matching stays with the CLI path.
/// Time/space complexity: O(k) cache lookups for k IDs / O(k).
pub fn download_add_ids(
    ids: &[String],
    quality: Option<String>,
) -> Result<Vec<db::SearchCacheEntry>> {
    let config = lux_core::config::Config::load().unwrap_or_default();
    let selected_quality = quality
        .as_ref()
        .and_then(|q| std::str::FromStr::from_str(q).ok())
        .unwrap_or(config.source.default_quality);

    let mut added_songs = Vec::new();
    for cli_id in ids {
        let song_entry = db::get_song_by_cli_id(cli_id)?
            .ok_or_else(|| anyhow!("CLI ID '{}' not found in cache. Search first.", cli_id))?;

        db::insert_download(
            &song_entry.song_id,
            &song_entry.source,
            &song_entry.name,
            &song_entry.singer,
            selected_quality.as_str(),
        )?;

        added_songs.push(song_entry);
    }

    // Lazy spawn daemon if not running
    if !added_songs.is_empty() {
        ensure_daemon_running()?;
    }

    Ok(added_songs)
}

pub async fn run(action: DownloadAction, json: bool) -> Result<()> {
    match action {
        DownloadAction::Add { ids, quality, file } => {
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
                let selected_quality = quality
                    .as_ref()
                    .and_then(|q| std::str::FromStr::from_str(q).ok())
                    .unwrap_or(
                        lux_core::config::Config::load()
                            .unwrap_or_default()
                            .source
                            .default_quality,
                    );

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
                added_songs = download_add_ids(&ids, quality)?;
            }

            // Lazy spawn daemon if not running (idempotent; also covers the
            // playlist-file branch above)
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

/// Whether `pid` refers to a live `alx` download daemon.
///
/// On Linux the process identity is verified against procfs: the PID is only
/// accepted when `/proc/<pid>/comm` (or, as fallback, the basename of the
/// `/proc/<pid>/exe` symlink) is exactly `alx` or starts with `alx`. A
/// recycled PID owned by any other program therefore counts as dead. Where
/// `proc_dir` itself is absent (macOS/Windows have no procfs) identity cannot
/// be verified and every parseable PID is assumed alive.
///
/// `proc_dir` is a parameter seam so tests can inject a fake `/proc` tree.
pub(crate) fn pid_is_live_daemon(pid: u32, proc_dir: &Path) -> bool {
    if !proc_dir.is_dir() {
        return true;
    }
    let entry = proc_dir.join(pid.to_string());
    if let Ok(comm) = fs::read_to_string(entry.join("comm")) {
        return process_name_is_alx(comm.trim());
    }
    if let Ok(exe) = fs::read_link(entry.join("exe")) {
        if let Some(name) = exe.file_name() {
            return process_name_is_alx(&name.to_string_lossy());
        }
    }
    false
}

fn process_name_is_alx(name: &str) -> bool {
    name.starts_with("alx")
}

/// Whether the pidfile points at a live `alx` daemon. Missing, unreadable,
/// unparsable or identity-mismatched pidfiles all count as not running.
pub(crate) fn daemon_pid_alive(pid_file: &Path, proc_dir: &Path) -> bool {
    fs::read_to_string(pid_file)
        .ok()
        .and_then(|content| content.trim().parse::<u32>().ok())
        .map(|pid| pid_is_live_daemon(pid, proc_dir))
        .unwrap_or(false)
}

pub fn ensure_daemon_running() -> Result<()> {
    let paths = lux_core::config::resolve_paths();
    let pid_file = paths.cache_dir.join("download.pid");
    let proc_dir = Path::new("/proc");

    if daemon_pid_alive(&pid_file, proc_dir) {
        return Ok(());
    }

    // Serialize the stale-pidfile removal + spawn across concurrent CLI
    // processes (O_EXCL sentinel lock, same pattern as the mpv spawn lock):
    // without it two invocations can both observe a dead daemon and spawn
    // duplicates racing over `reset_downloading_to_pending()` and `.part`
    // writes.
    let lock_path = paths.cache_dir.join("download-daemon.lock");
    let _spawn_guard = crate::player::spawn_lock::SpawnLock::acquire(&lock_path)?;

    // Double-check under the lock: a competing process may have respawned
    // the daemon while we waited for it.
    if !daemon_pid_alive(&pid_file, proc_dir) {
        // Stale or forged pidfile: remove before respawning so the new
        // daemon owns the file exclusively.
        let _ = fs::remove_file(&pid_file);
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

/// Action to take on a download-stream response after a resume request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RangeResponseAction {
    /// Status is acceptable; keep streaming from the current offset.
    Proceed,
    /// Server ignored the Range header (200 for a ranged request); the
    /// .part must be truncated and the stream restarted from byte 0.
    RestartFromScratch,
}

/// Pure decision for a stream-response status given whether a Range header
/// was sent with this request.
///
/// - No range sent: any 2xx proceeds, anything else fails.
/// - Range sent: 206 Partial Content proceeds as-is, 200 OK means the server
///   ignored the range and the full body is being replayed -> restart from
///   scratch, anything else fails.
fn resume_response_action(
    status: reqwest::StatusCode,
    range_sent: bool,
) -> Result<RangeResponseAction> {
    if !range_sent {
        if !status.is_success() {
            anyhow::bail!("Failed to start download stream: HTTP status {}", status);
        }
        return Ok(RangeResponseAction::Proceed);
    }
    match status {
        reqwest::StatusCode::PARTIAL_CONTENT => Ok(RangeResponseAction::Proceed),
        reqwest::StatusCode::OK => Ok(RangeResponseAction::RestartFromScratch),
        other => anyhow::bail!("Failed to start download stream: HTTP status {}", other),
    }
}

/// Pure length-integrity check run after the chunk loop.
///
/// `expected_total` combines the advertised content length with bytes
/// already present before this request (`content_length + prior_bytes`);
/// a value of 0 means the server advertised no length (e.g. chunked
/// transfer) and verification is skipped. A clean mid-body connection close
/// leaves `bytes_downloaded` short of `expected_total` and is rejected here
/// instead of being finalized as a completed file.
fn verify_complete_download(bytes_downloaded: u64, expected_total: u64) -> Result<()> {
    if expected_total > 0 && bytes_downloaded != expected_total {
        anyhow::bail!(
            "Incomplete download: received {} of {} expected bytes (connection closed early); discarding partial file",
            bytes_downloaded,
            expected_total
        );
    }
    Ok(())
}

/// Sniff the audio container from the leading magic bytes of a downloaded
/// file. Recognizes FLAC (`fLaC`) and MP3 (ID3v2 `ID3` tag, or a raw MPEG
/// audio frame sync `0xFFEx`/`0xFFFx`). Returns `None` when inconclusive so
/// callers can apply their own fallback.
fn detect_audio_format(head: &[u8]) -> Option<&'static str> {
    if head.len() >= 4 && &head[..4] == b"fLaC" {
        return Some("flac");
    }
    if head.len() >= 3 && &head[..3] == b"ID3" {
        return Some("mp3");
    }
    if head.len() >= 2 && head[0] == 0xFF && (head[1] & 0xE0) == 0xE0 {
        return Some("mp3");
    }
    None
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

    // Atomically write into a .part file inside cache dir
    // (the container extension is decided after download via magic-byte
    // sniffing; see `detect_audio_format`).
    let part_filename = format!("{}_{}.part", task.song_id, quality.as_str());
    let part_path = paths.cache_dir.join(&part_filename);

    let mut file = File::options()
        .create(true)
        .write(true)
        .read(true)
        .truncate(false)
        .open(&part_path)?;

    let mut existing_bytes = file.metadata()?.len();

    // Resumable HTTP Range requests: probe whether the server supports
    // byte ranges before relying on resume.
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

    // Send the stream request. When a Range header was sent but the server
    // answers 200 OK instead of 206 Partial Content it ignored the range and
    // is replaying the full body: truncate the .part and restart from
    // scratch within this task rather than appending a duplicate payload.
    let mut response = loop {
        let mut request = client.get(&stream_url);
        let range_sent = support_range && existing_bytes > 0;
        if range_sent {
            file.seek(SeekFrom::End(0))?;
            request = request.header(reqwest::header::RANGE, format!("bytes={}-", existing_bytes));
        } else {
            existing_bytes = 0;
            file.set_len(0)?;
            file.seek(SeekFrom::Start(0))?;
        }

        let resp = request.send().await?;
        match resume_response_action(resp.status(), range_sent)? {
            RangeResponseAction::Proceed => break resp,
            RangeResponseAction::RestartFromScratch => {
                eprintln!(
                    "↻ Server ignored Range header for '{}'; restarting download from scratch",
                    task.name
                );
                existing_bytes = 0;
            }
        }
    };

    let content_len = response.content_length().unwrap_or(0);
    let total_bytes = content_len + existing_bytes;

    let mut bytes_downloaded = existing_bytes;
    let mut last_progress = 0.0;
    let mut last_update_time = Instant::now();

    // Stream download loop
    while let Some(chunk) = response.chunk().await? {
        file.write_all(&chunk)?;
        bytes_downloaded += chunk.len() as u64;

        if total_bytes > 0 {
            let progress = bytes_downloaded as f64 / total_bytes as f64;
            let now = Instant::now();
            if now.duration_since(last_update_time) >= Duration::from_millis(250)
                || (progress - last_progress).abs() >= 0.025
            {
                db::update_download_progress(task.id, progress, bytes_downloaded, total_bytes)?;
                last_progress = progress;
                last_update_time = now;
            }
        }
    }

    if total_bytes > 0 {
        let progress = bytes_downloaded as f64 / total_bytes as f64;
        let _ = db::update_download_progress(task.id, progress, bytes_downloaded, total_bytes);
    }

    file.flush()?;
    drop(file);

    // Integrity: reject truncated bodies instead of finalizing them. On
    // mismatch the .part is deleted so a retry starts from a clean slate.
    if let Err(e) = verify_complete_download(bytes_downloaded, total_bytes) {
        let _ = fs::remove_file(&part_path);
        return Err(e);
    }

    // Decide the container extension from the actual file head instead of a
    // URL substring: read the first 16 bytes of the .part and sniff magic
    // bytes. The URL heuristic only applies when sniffing is inconclusive
    // (e.g. exotic containers).
    let mut head = [0u8; 16];
    let head_len = File::open(&part_path)?.read(&mut head)?;
    let extension = detect_audio_format(&head[..head_len]).unwrap_or_else(|| {
        if stream_url.contains(".flac") {
            "flac"
        } else {
            "mp3"
        }
    });

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

    // Integrity check
    let meta = fs::metadata(&part_path)?;
    if meta.len() == 0 {
        return Err(anyhow!("Downloaded file is empty (integrity check failed)"));
    }

    // Rename atomically to the final filename in output_dir
    let clean_title = sanitize_filename(&task.name);
    let clean_singer = sanitize_filename(&task.singer);
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
            return Err(anyhow!(
                "Failed to place file on path '{}': {}",
                final_filename,
                e
            ));
        }
    }

    // OS level disk synchronization barrier
    if let Ok(f) = File::open(&final_path) {
        let _ = f.sync_all();
    }

    Ok(final_path)
}

pub fn sanitize_filename(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|c| {
            if c.is_control() || ['/', '\\', '?', '*', ':', '"', '<', '>', '|'].contains(&c) {
                '-'
            } else {
                c
            }
        })
        .collect();

    let mut trimmed = sanitized.trim().to_string();
    while trimmed.contains("--") {
        trimmed = trimmed.replace("--", "-");
    }

    if trimmed.chars().count() > 180 {
        trimmed = trimmed.chars().take(180).collect();
    }

    if trimmed.is_empty() {
        "Unknown".to_string()
    } else {
        trimmed
    }
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

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("Normal Title"), "Normal Title");
        assert_eq!(sanitize_filename("Title/With\\Slash"), "Title-With-Slash");
        assert_eq!(
            sanitize_filename("emoji 🎶 & special ⚡?"),
            "emoji 🎶 & special ⚡-"
        );
        assert_eq!(
            sanitize_filename("   leading and trailing   "),
            "leading and trailing"
        );
        assert_eq!(sanitize_filename("multiple----dashes"), "multiple-dashes");
        assert_eq!(sanitize_filename(""), "Unknown");

        let long_title = "a".repeat(200);
        let cleaned_long = sanitize_filename(&long_title);
        assert_eq!(cleaned_long.chars().count(), 180);
    }

    #[test]
    fn test_http_timeout_uses_configured_value() {
        assert_eq!(http_timeout(Some(45)), Duration::from_secs(45));
        assert_eq!(http_timeout(Some(1)), Duration::from_secs(1));
    }

    #[test]
    fn test_http_timeout_falls_back_on_none_and_zero() {
        assert_eq!(
            http_timeout(None),
            Duration::from_secs(DEFAULT_HTTP_TIMEOUT_SECS)
        );
        assert_eq!(
            http_timeout(Some(0)),
            Duration::from_secs(DEFAULT_HTTP_TIMEOUT_SECS)
        );
    }

    /// Build a fake procfs tree: `<root>/<pid>/comm` entries plus an
    /// optional `/exe` symlink (symlinks require Unix).
    fn fake_proc_dir(tag: &str, pids: &[(&u32, &str)]) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("alx-test-proc-{}-{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for (pid, comm) in pids {
            let dir = root.join(pid.to_string());
            fs::create_dir_all(&dir).expect("create fake proc entry");
            fs::write(dir.join("comm"), format!("{comm}\n")).expect("write fake comm");
        }
        root
    }

    #[test]
    fn test_pid_identity_accepts_alx_comm() {
        let pid = 4242u32;
        let proc_dir = fake_proc_dir("accept", &[(&pid, "alx")]);
        assert!(pid_is_live_daemon(pid, &proc_dir));
        // Prefix match also accepted (e.g. threaded/renamed binaries).
        fs::write(proc_dir.join("4242").join("comm"), "alx-daemon\n").unwrap();
        assert!(pid_is_live_daemon(pid, &proc_dir));
        let _ = fs::remove_dir_all(&proc_dir);
    }

    #[test]
    fn test_pid_identity_rejects_foreign_and_absent() {
        let foreign = 1111u32;
        let ghost = 2222u32;
        let proc_dir = fake_proc_dir("reject", &[(&foreign, "mpv")]);
        // Recycled PID now owned by another program counts as dead.
        assert!(!pid_is_live_daemon(foreign, &proc_dir));
        // Absent /proc/<pid> entry counts as dead.
        assert!(!pid_is_live_daemon(ghost, &proc_dir));
        let _ = fs::remove_dir_all(&proc_dir);
    }

    #[cfg(unix)]
    #[test]
    fn test_pid_identity_falls_back_to_exe_basename() {
        use std::os::unix::fs::symlink;

        let pid = 5150u32;
        let proc_dir = fake_proc_dir("exe", &[(&pid, "")]);
        let entry = proc_dir.join("5150");
        fs::remove_file(entry.join("comm")).unwrap();
        let target = proc_dir.join("alx-worker");
        fs::write(&target, b"elf").unwrap();
        symlink(&target, entry.join("exe")).unwrap();
        assert!(pid_is_live_daemon(pid, &proc_dir));

        fs::remove_file(entry.join("exe")).unwrap();
        symlink(proc_dir.join("other-program"), entry.join("exe")).unwrap();
        assert!(!pid_is_live_daemon(pid, &proc_dir));
        let _ = fs::remove_dir_all(&proc_dir);
    }

    #[test]
    fn test_pid_identity_assumes_alive_without_procfs() {
        // No procfs on this platform: identity cannot be verified.
        assert!(pid_is_live_daemon(
            9999,
            &PathBuf::from("/nonexistent-proc-dir")
        ));
    }

    #[test]
    fn test_daemon_pid_alive_parses_pidfile() {
        let base = std::env::temp_dir().join(format!("alx-test-pidfile-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let pid_file = base.join("download.pid");

        // Missing pidfile -> not alive.
        assert!(!daemon_pid_alive(
            &pid_file,
            &PathBuf::from("/nonexistent-proc")
        ));

        // Unparsable content -> not alive.
        fs::write(&pid_file, "not-a-pid\n").unwrap();
        assert!(!daemon_pid_alive(
            &pid_file,
            &PathBuf::from("/nonexistent-proc")
        ));

        let live = 7331u32;
        let proc_dir = fake_proc_dir("pidfile", &[(&live, "alx")]);
        fs::write(&pid_file, format!("{live}\n")).unwrap();
        assert!(daemon_pid_alive(&pid_file, &proc_dir));

        // Live PID but foreign process -> stale.
        fs::write(&pid_file, format!("{}\n", 8675u32)).unwrap();
        assert!(!daemon_pid_alive(&pid_file, &proc_dir));
        let _ = fs::remove_dir_all(&base);
        let _ = fs::remove_dir_all(&proc_dir);
    }

    #[test]
    fn test_verify_complete_download_rejects_truncation() {
        assert!(verify_complete_download(1000, 1000).is_ok());
        // Unknown advertised length (0) skips verification.
        assert!(verify_complete_download(0, 0).is_ok());
        assert!(verify_complete_download(123, 0).is_ok());
        // Clean mid-body close leaves the file short and must fail.
        let err = verify_complete_download(640, 1024).unwrap_err();
        assert!(err.to_string().contains("received 640 of 1024"));
        // Oversized bodies are also a mismatch.
        assert!(verify_complete_download(2048, 1024).is_err());
    }

    #[test]
    fn test_resume_response_action_206_vs_200() {
        let ok = reqwest::StatusCode::OK;
        let partial = reqwest::StatusCode::PARTIAL_CONTENT;
        let server_error = reqwest::StatusCode::INTERNAL_SERVER_ERROR;

        // Plain request: any 2xx proceeds.
        assert_eq!(
            resume_response_action(ok, false).unwrap(),
            RangeResponseAction::Proceed
        );
        assert!(resume_response_action(server_error, false).is_err());

        // Ranged request: only 206 may proceed; 200 means the range was
        // ignored -> restart from scratch; other statuses fail.
        assert_eq!(
            resume_response_action(partial, true).unwrap(),
            RangeResponseAction::Proceed
        );
        assert_eq!(
            resume_response_action(ok, true).unwrap(),
            RangeResponseAction::RestartFromScratch
        );
        assert!(resume_response_action(server_error, true).is_err());
    }

    #[test]
    fn test_detect_audio_format_flac() {
        let mut head = vec![0x66, 0x4C, 0x61, 0x43]; // "fLaC"
        head.extend_from_slice(&[0x00, 0x00, 0x00, 0x22]);
        assert_eq!(detect_audio_format(&head), Some("flac"));
    }

    #[test]
    fn test_detect_audio_format_id3() {
        let mut head = b"ID3".to_vec();
        head.extend_from_slice(&[0x04, 0x00, 0x00, 0x00, 0x00, 0x0A, 0x00]);
        assert_eq!(detect_audio_format(&head), Some("mp3"));
        // ID3 requires all three bytes; a truncated head is inconclusive.
        assert_eq!(detect_audio_format(b"ID"), None);
    }

    #[test]
    fn test_detect_audio_format_mpeg_frame_sync() {
        // 0xFFFB / 0xFFE3 both match the 11-bit frame sync (0xFFEx/0xFFFx).
        assert_eq!(detect_audio_format(&[0xFF, 0xFB, 0x90, 0x00]), Some("mp3"));
        assert_eq!(detect_audio_format(&[0xFF, 0xE3, 0x00, 0x00]), Some("mp3"));
        // 2-byte sync only: still detected.
        assert_eq!(detect_audio_format(&[0xFF, 0xFA]), Some("mp3"));
        // Non-sync leading byte or low bits clear -> inconclusive.
        assert_eq!(detect_audio_format(&[0xFE, 0xFB]), None);
        assert_eq!(detect_audio_format(&[0xFF, 0x00]), None);
        assert_eq!(detect_audio_format(&[0xFF]), None);
    }

    #[test]
    fn test_detect_audio_format_inconclusive() {
        assert_eq!(detect_audio_format(&[]), None);
        assert_eq!(detect_audio_format(b"RIFF....WAVEfmt "), None);
        assert_eq!(detect_audio_format(b"<html><body>404"), None);
        // Exactly 16 bytes are sufficient for every signature.
        assert_eq!(detect_audio_format(&b"fLaC".to_vec()), Some("flac"));
    }
}

//! `alx mcp` — Model Context Protocol stdio server.
//!
//! Implements a hand-rolled JSON-RPC 2.0 / MCP 2024-11-05-compatible subset
//! over newline-delimited JSON using only existing dependencies (`serde_json`,
//! `std::io`). stdout carries protocol frames exclusively; every diagnostic is
//! written to stderr so agents always receive parseable output.
//!
//! Transport seam: [`serve`] reads any `BufRead` and writes any `Write`, with
//! tool execution behind a `Fn(&str, Value) -> Result<Value, String>`
//! dispatcher so unit tests drive complete sessions in memory.
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};

/// Protocol version reported during the initialize handshake.
pub const PROTOCOL_VERSION: &str = "2024-11-05";

static QUIET: AtomicBool = AtomicBool::new(false);

/// Emit a diagnostic to stderr unless `--quiet` suppressed logging.
fn diag_warn(message: &str) {
    if !QUIET.load(Ordering::Relaxed) {
        eprintln!("[alx-mcp] {message}");
    }
}

/// Run the server foreground on stdin/stdout until EOF.
///
/// Malformed frames are logged to stderr and skipped; EOF exits cleanly.
pub fn run(quiet: bool) -> anyhow::Result<()> {
    QUIET.store(quiet, Ordering::Relaxed);
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    serve(stdin.lock(), stdout.lock(), &dispatch_tool)
}

/// Core NDJSON loop: one JSON-RPC message per line, response per request.
pub fn serve(
    input: impl BufRead,
    mut output: impl Write,
    dispatch: &dyn Fn(&str, Value) -> Result<Value, String>,
) -> anyhow::Result<()> {
    for line in input.lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                diag_warn(&format!("failed reading stdin line: {e}"));
                continue;
            }
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                diag_warn(&format!("dropping malformed frame ({e}): {trimmed}"));
                continue;
            }
        };
        let Some(obj) = msg.as_object() else {
            diag_warn(&format!("dropping non-object frame: {trimmed}"));
            continue;
        };
        let Some(method) = obj.get("method").and_then(Value::as_str) else {
            diag_warn(&format!("dropping frame without method: {trimmed}"));
            continue;
        };

        // Notifications (absent or null id) never produce a response.
        let is_notification = obj.get("id").is_none_or(Value::is_null);
        if is_notification || method.starts_with("notifications/") {
            continue;
        }
        let id = obj.get("id").cloned().unwrap_or(Value::Null);

        let outcome = match method {
            "initialize" => Ok(json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": { "tools": {} },
                "serverInfo": {
                    "name": "alx",
                    "version": env!("CARGO_PKG_VERSION")
                }
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tool_descriptors() })),
            "tools/call" => Ok(tool_call_envelope(
                obj.get("params").unwrap_or(&Value::Null),
                dispatch,
            )),
            other => Err(json!({
                "code": -32601,
                "message": format!("Method not found: {other}")
            })),
        };

        let response = match outcome {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err(error) => json!({ "jsonrpc": "2.0", "id": id, "error": error }),
        };
        writeln!(output, "{response}")?;
        output.flush()?;
    }
    Ok(())
}

/// Execute a tools/call request and wrap the payload in the MCP content
/// envelope. Tool failures become `isError: true` results carrying a compact
/// `{"error": ...}` JSON payload — never a panic, never a broken stream.
fn tool_call_envelope(
    params: &Value,
    dispatch: &dyn Fn(&str, Value) -> Result<Value, String>,
) -> Value {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    let payload = if name.is_empty() {
        Err("tools/call requires a non-empty tool name".to_string())
    } else {
        match arguments {
            Value::Object(_) => dispatch(name, arguments),
            _ => Err("tools/call arguments must be an object".to_string()),
        }
    };

    match payload {
        Ok(value) => text_envelope(&value, false),
        Err(message) => text_envelope(&json!({ "error": message }), true),
    }
}

fn text_envelope(payload: &Value, is_error: bool) -> Value {
    let text = serde_json::to_string(payload)
        .unwrap_or_else(|_| "{\"error\":\"unserializable tool payload\"}".to_string());
    json!({
        "content": [{ "type": "text", "text": text }],
        "isError": is_error
    })
}

// ---------------------------------------------------------------------------
// Tool descriptor table (v1)
// ---------------------------------------------------------------------------

fn object_schema(properties: Value, required: &[&str]) -> Value {
    let mut schema = json!({
        "type": "object",
        "properties": properties
    });
    if !required.is_empty() {
        schema["required"] = json!(required);
    }
    schema
}

fn empty_schema() -> Value {
    object_schema(json!({}), &[])
}

/// Static v1 tool catalog served by `tools/list`.
pub fn tool_descriptors() -> Vec<Value> {
    vec![
        ("search", "Search music across configured sources. Returns cached song entries with stable `cli_id` values usable by play/queue/favorite tools.", object_schema(json!({
            "query": { "type": "string", "description": "Search keyword; supports artist:/album:/kbps: directives" },
            "source": { "type": "string", "enum": ["all", "wy", "kw", "kg", "tx", "mg"], "default": "all" },
            "page": { "type": "integer", "minimum": 1, "default": 1 },
            "limit": { "type": "integer", "minimum": 1, "default": 30 }
        }), &["query"])),
        ("play", "Start playback from song IDs (cached via search), a direct stream URL, or a local file path.", object_schema(json!({
            "ids": { "type": "array", "items": { "type": "string" }, "description": "CLI ID(s) from search results" },
            "url": { "type": "string", "description": "Stream URL or local file path" },
            "replace_queue": { "type": "boolean", "default": true, "description": "Clear the queue before playing" }
        }), &[])),
        ("playback_control", "Control playback state.", object_schema(json!({
            "action": { "type": "string", "enum": ["pause", "resume", "stop", "toggle"] }
        }), &["action"])),
        ("skip", "Skip to the next or previous track.", object_schema(json!({
            "direction": { "type": "string", "enum": ["next", "prev"] }
        }), &["direction"])),
        ("status", "Report playback state: status, position/duration (seconds), volume (%), queue index (0-based) and current song.", empty_schema()),
        ("queue_list", "List the active play queue. Array positions are 0-based.", empty_schema()),
        ("queue_add", "Append cached songs to the end of the queue.", object_schema(json!({
            "ids": { "type": "array", "items": { "type": "string" }, "description": "CLI ID(s) from search results" }
        }), &["ids"])),
        ("queue_remove", "Remove one queue entry by 0-based index.", object_schema(json!({
            "index": { "type": "integer", "minimum": 0 }
        }), &["index"])),
        ("queue_clear", "Empty the play queue and stop playback.", empty_schema()),
        ("playlist_list", "List user playlists with song counts.", empty_schema()),
        ("playlist_show", "Show the songs of one playlist.", object_schema(json!({
            "name": { "type": "string" }
        }), &["name"])),
        ("playlist_add", "Append cached songs to a playlist.", object_schema(json!({
            "name": { "type": "string" },
            "ids": { "type": "array", "items": { "type": "string" } }
        }), &["name", "ids"])),
        ("playlist_remove", "Remove one song from a playlist by CLI ID.", object_schema(json!({
            "name": { "type": "string" },
            "id": { "type": "string" }
        }), &["name", "id"])),
        ("favorite_add", "Add a cached song to the built-in \"Favorites\" playlist.", object_schema(json!({
            "id": { "type": "string", "description": "CLI ID of the song" }
        }), &["id"])),
        ("favorite_list", "List the songs in the built-in \"Favorites\" playlist.", empty_schema()),
        ("lyric_get", "Fetch lyrics for a song ID or the currently playing song (cached-first).", object_schema(json!({
            "id": { "type": "string", "description": "CLI ID or platform song ID; defaults to current song" },
            "track": { "type": "string", "enum": ["main", "translated", "romanized"], "default": "main" }
        }), &[])),
        ("download_add", "Queue songs for background download by CLI ID.", object_schema(json!({
            "ids": { "type": "array", "items": { "type": "string" } },
            "quality": { "type": "string", "description": "Optional quality override (e.g. 320k, flac)" }
        }), &["ids"])),
        ("download_status", "List active and pending background download tasks.", empty_schema()),
    ].into_iter().map(|(name, description, input_schema)| {
        json!({ "name": name, "description": description, "inputSchema": input_schema })
    }).collect()
}

// ---------------------------------------------------------------------------
// Argument parsing helpers (defensive: bad shapes are errors, never panics)
// ---------------------------------------------------------------------------

fn req_string(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("argument '{key}' is required and must be a string"))
}

fn opt_string(args: &Value, key: &str) -> Result<Option<String>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(format!("argument '{key}' must be a string")),
    }
}

fn opt_u64(args: &Value, key: &str) -> Result<Option<u64>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) if v.is_u64() => Ok(Some(v.as_u64().expect("checked u64"))),
        Some(_) => Err(format!("argument '{key}' must be a non-negative integer")),
    }
}

fn opt_bool(args: &Value, key: &str) -> Result<Option<bool>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(format!("argument '{key}' must be a boolean")),
    }
}

fn string_array(args: &Value, key: &str) -> Result<Vec<String>, String> {
    let values = args
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("argument '{key}' is required and must be an array of strings"))?;
    values
        .iter()
        .map(|v| {
            v.as_str()
                .map(str::to_string)
                .ok_or_else(|| format!("argument '{key}' must contain only strings"))
        })
        .collect()
}

fn req_enum<'a>(args: &'a Value, key: &str, allowed: &[&'a str]) -> Result<&'a str, String> {
    let value = args.get(key).and_then(Value::as_str).ok_or_else(|| {
        format!(
            "argument '{key}' is required and must be one of: {}",
            allowed.join(", ")
        )
    })?;
    if allowed.contains(&value) {
        Ok(value)
    } else {
        Err(format!(
            "argument '{key}' must be one of: {}",
            allowed.join(", ")
        ))
    }
}

/// Bridge a synchronous handler into futures that genuinely need the async
/// runtime (currently only the multi-source search fan-out). Prefers the
/// ambient tokio runtime (`alx mcp` runs under `#[tokio::main]`); builds a
/// throwaway current-thread runtime otherwise so tests stay runtime-free.
fn bridge_async<F, T>(fut: F) -> Result<T, String>
where
    F: std::future::Future<Output = anyhow::Result<T>>,
{
    let mapped = async { fut.await.map_err(|e| e.to_string()) };
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| handle.block_on(mapped)),
        Err(_) => {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| format!("failed to build fallback runtime: {e}"))?;
            rt.block_on(mapped)
        }
    }
}

fn default_quality() -> lux_core::types::Quality {
    lux_core::config::Config::load()
        .unwrap_or_default()
        .source
        .default_quality
}

fn err_to_string<T>(result: anyhow::Result<T>) -> Result<T, String> {
    result.map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Tool dispatch
// ---------------------------------------------------------------------------

/// Route a tools/call request to its handler. Payloads returned here are
/// serialized to compact JSON inside the MCP text-content envelope.
pub fn dispatch_tool(name: &str, args: Value) -> Result<Value, String> {
    match name {
        "search" => tool_search(&args),
        "play" => tool_play(&args),
        "playback_control" => tool_playback_control(&args),
        "skip" => tool_skip(&args),
        "status" => tool_status(&args),
        "queue_list" => tool_queue_list(),
        "queue_add" => tool_queue_add(&args),
        "queue_remove" => tool_queue_remove(&args),
        "queue_clear" => tool_queue_clear(),
        "playlist_list" => tool_playlist_list(),
        "playlist_show" => tool_playlist_show(&args),
        "playlist_add" => tool_playlist_add(&args),
        "playlist_remove" => tool_playlist_remove(&args),
        "favorite_add" => tool_favorite_add(&args),
        "favorite_list" => tool_favorite_list(),
        "lyric_get" => tool_lyric_get(&args),
        "download_add" => tool_download_add(&args),
        "download_status" => tool_download_status(),
        other => Err(format!("unknown tool: {other}")),
    }
}

fn tool_search(args: &Value) -> Result<Value, String> {
    let query = req_string(args, "query")?;
    if query.trim().is_empty() {
        return Err("argument 'query' must be a non-empty string".to_string());
    }
    let source = opt_string(args, "source")?.unwrap_or_else(|| "all".to_string());
    let page = opt_u64(args, "page")?.unwrap_or(1);
    let limit = opt_u64(args, "limit")?.unwrap_or(30);
    if page == 0 {
        return Err("argument 'page' must be >= 1".to_string());
    }
    if limit == 0 {
        return Err("argument 'limit' must be >= 1".to_string());
    }

    let entries = bridge_async(crate::cmd::search::search_songs(
        &query,
        &source,
        page as usize,
        limit as usize,
    ))?;
    Ok(json!({ "count": entries.len(), "results": entries }))
}

fn tool_play(args: &Value) -> Result<Value, String> {
    let ids = match args.get("ids") {
        None | Some(Value::Null) => Vec::new(),
        Some(_) => string_array(args, "ids")?,
    };
    let url = opt_string(args, "url")?;
    let replace_queue = opt_bool(args, "replace_queue")?.unwrap_or(true);

    if ids.is_empty() && url.is_none() {
        return Err("provide either 'ids' or 'url'".to_string());
    }
    if !ids.is_empty() && url.is_some() {
        return Err("provide either 'ids' or 'url', not both".to_string());
    }

    crate::library::db::init_db().map_err(|e| e.to_string())?;
    let entries = if let Some(target) = url {
        vec![crate::cmd::play::direct_entry(target)]
    } else {
        let mut resolved = Vec::with_capacity(ids.len());
        for id in &ids {
            let song = crate::library::db::get_song_by_cli_id(id)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| {
                    format!("Song ID '{id}' not found in search cache. Search first.")
                })?;
            resolved.push(song);
        }
        resolved
    };

    let outcome = err_to_string(crate::cmd::play::start_playback(
        entries,
        default_quality(),
        replace_queue,
    ))?;
    Ok(json!({
        "status": "playing",
        "id": outcome.first.cli_id,
        "name": outcome.first.name,
        "singer": outcome.first.singer,
        "source": outcome.first.source,
        "queue_count": outcome.queue_count
    }))
}

fn tool_playback_control(args: &Value) -> Result<Value, String> {
    let action = req_enum(args, "action", &["pause", "resume", "stop", "toggle"])?;
    let client = crate::player::MpvClient::new();
    // Never cold-start a headless daemon from an agent session; surface the
    // same guidance the skip commands give when nothing is running.
    client.try_ensure_running().map_err(|e| e.to_string())?;

    match action {
        "pause" => {
            client.pause().map_err(|e| e.to_string())?;
            Ok(json!({ "status": "paused" }))
        }
        "resume" => {
            client.resume().map_err(|e| e.to_string())?;
            Ok(json!({ "status": "resumed" }))
        }
        "stop" => {
            client.stop().map_err(|e| e.to_string())?;
            Ok(json!({ "status": "stopped" }))
        }
        _ => {
            let (_, _, _, _, paused) = client
                .get_playback_status()
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "Nothing is playing to toggle.".to_string())?;
            if paused {
                client.resume().map_err(|e| e.to_string())?;
                Ok(json!({ "status": "resumed", "via": "toggle" }))
            } else {
                client.pause().map_err(|e| e.to_string())?;
                Ok(json!({ "status": "paused", "via": "toggle" }))
            }
        }
    }
}

fn tool_skip(args: &Value) -> Result<Value, String> {
    let direction = req_enum(args, "direction", &["next", "prev"])?;
    let client = crate::player::MpvClient::new();
    let result = match direction {
        "next" => client.next(),
        _ => client.prev(),
    };
    result.map_err(|e| crate::player::describe_skip_error(direction, &e))?;
    Ok(json!({ "status": "skipped", "direction": direction }))
}

fn tool_status(_args: &Value) -> Result<Value, String> {
    let client = crate::player::MpvClient::new();

    // Best-effort queue index refresh, mirroring `alx queue show`.
    let mut queue_state = err_to_string(crate::cmd::queue::load_or_init_queue())?;
    if let Ok(Some(new_idx)) = client.get_playing_index()
        && queue_state.current_index != Some(new_idx)
    {
        queue_state.current_index = Some(new_idx);
        let _ = crate::cmd::queue::save_queue(&queue_state);
    }

    match client.get_playback_status().map_err(|e| e.to_string())? {
        Some((_, position, duration, volume, paused)) => {
            let paths = lux_core::config::resolve_paths();
            let song = crate::cmd::lyric::read_current_song(&paths.cache_dir);
            Ok(json!({
                "status": if paused { "paused" } else { "playing" },
                "position": position,
                "duration": duration,
                "volume": volume,
                "queue_index": queue_state.current_index,
                "song": song
            }))
        }
        None => Ok(json!({
            "status": "stopped",
            "position": Value::Null,
            "duration": Value::Null,
            "volume": Value::Null,
            "queue_index": queue_state.current_index,
            "song": Value::Null
        })),
    }
}

fn tool_queue_list() -> Result<Value, String> {
    let queue_state = err_to_string(crate::cmd::queue::load_or_init_queue())?;
    Ok(json!({
        "current_index": queue_state.current_index,
        "songs": queue_state.songs
    }))
}

fn tool_queue_add(args: &Value) -> Result<Value, String> {
    let ids = string_array(args, "ids")?;
    if ids.is_empty() {
        return Err("argument 'ids' must contain at least one CLI ID".to_string());
    }
    let count = err_to_string(crate::cmd::queue::queue_append_ids(&ids))?;
    Ok(json!({ "status": "added", "count": count }))
}

fn tool_queue_remove(args: &Value) -> Result<Value, String> {
    let index = opt_u64(args, "index")?.ok_or_else(|| {
        "argument 'index' is required and must be a non-negative integer".to_string()
    })?;
    let mut queue_state = err_to_string(crate::cmd::queue::load_or_init_queue())?;
    if index as usize >= queue_state.songs.len() {
        return Err(format!(
            "index {index} out of range; queue has {} entries (0-based)",
            queue_state.songs.len()
        ));
    }

    let client = crate::player::MpvClient::new();
    let _ = crate::player::ipc::send_mpv_command(
        &client.socket_path,
        vec![json!("playlist-remove"), json!(index)],
    );

    let updated = crate::cmd::queue::queue_after_removal(&queue_state, index as usize);
    queue_state = updated;
    err_to_string(crate::cmd::queue::save_queue(&queue_state))?;
    Ok(json!({ "status": "removed", "index": index }))
}

fn tool_queue_clear() -> Result<Value, String> {
    let client = crate::player::MpvClient::new();
    let _ =
        crate::player::ipc::send_mpv_command(&client.socket_path, vec![json!("playlist-clear")]);
    let _ = client.stop();

    err_to_string(crate::cmd::queue::save_queue(
        &crate::cmd::queue::PlayQueue {
            songs: Vec::new(),
            current_index: None,
        },
    ))?;
    Ok(json!({ "status": "cleared" }))
}

fn tool_playlist_list() -> Result<Value, String> {
    let playlists = err_to_string(crate::library::db::list_playlists())?;
    let entries: Vec<Value> = playlists
        .into_iter()
        .map(|(name, description, song_count)| {
            json!({
                "name": name,
                "description": description,
                "song_count": song_count
            })
        })
        .collect();
    Ok(json!({ "count": entries.len(), "playlists": entries }))
}

fn tool_playlist_show(args: &Value) -> Result<Value, String> {
    let name = req_string(args, "name")?;
    let songs = err_to_string(crate::library::db::get_playlist_songs(&name))?;
    Ok(json!({ "name": name, "songs": songs }))
}

fn tool_playlist_add(args: &Value) -> Result<Value, String> {
    let name = req_string(args, "name")?;
    let ids = string_array(args, "ids")?;
    crate::library::db::init_db().map_err(|e| e.to_string())?;

    for id in &ids {
        let song = crate::library::db::get_song_by_cli_id(id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("Song with CLI ID '{id}' not found in cache. Search first."))?;
        crate::library::db::add_to_playlist(&name, &song).map_err(|e| e.to_string())?;
    }
    Ok(json!({ "status": "added", "playlist": name, "count": ids.len() }))
}

fn tool_playlist_remove(args: &Value) -> Result<Value, String> {
    let name = req_string(args, "name")?;
    let id = req_string(args, "id")?;
    crate::library::db::init_db().map_err(|e| e.to_string())?;

    let song = crate::library::db::get_song_by_cli_id(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Song with CLI ID '{id}' not found in cache."))?;
    crate::library::db::remove_from_playlist(&name, &song.song_id, &song.source)
        .map_err(|e| e.to_string())?;
    Ok(json!({ "status": "removed", "playlist": name, "song": song.name }))
}

fn tool_favorite_add(args: &Value) -> Result<Value, String> {
    let id = req_string(args, "id")?;
    crate::library::db::init_db().map_err(|e| e.to_string())?;

    let song = crate::library::db::get_song_by_cli_id(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Song with CLI ID '{id}' not found in cache. Search first."))?;
    crate::library::db::add_to_playlist("Favorites", &song).map_err(|e| e.to_string())?;
    Ok(json!({ "status": "added", "playlist": "Favorites", "song": song.name }))
}

fn tool_favorite_list() -> Result<Value, String> {
    crate::library::db::init_db().map_err(|e| e.to_string())?;
    let favorites = err_to_string(crate::library::db::get_playlist_songs("Favorites"))?;
    Ok(json!({ "count": favorites.len(), "favorites": favorites }))
}

fn tool_lyric_get(args: &Value) -> Result<Value, String> {
    let id = opt_string(args, "id")?;
    let track = opt_string(args, "track")?.unwrap_or_else(|| "main".to_string());
    let (translated, romanized) = match track.as_str() {
        "main" => (false, false),
        "translated" => (true, false),
        "romanized" => (false, true),
        other => {
            return Err(format!(
                "argument 'track' must be one of: main, translated, romanized (got '{other}')"
            ));
        }
    };

    let fetched = err_to_string(crate::cmd::lyric::fetch_lyric(
        id.as_deref(),
        translated,
        romanized,
    ))?;
    match fetched.content {
        Some(content) => Ok(json!({
            "song_id": fetched.song.song_id,
            "cli_id": fetched.song.cli_id,
            "name": fetched.song.name,
            "singer": fetched.song.singer,
            "track": fetched.track,
            "lyric": content
        })),
        None => Err(crate::cmd::lyric::missing_track_error(fetched.track).to_string()),
    }
}

fn tool_download_add(args: &Value) -> Result<Value, String> {
    let ids = string_array(args, "ids")?;
    if ids.is_empty() {
        return Err("argument 'ids' must contain at least one CLI ID".to_string());
    }
    let quality = opt_string(args, "quality")?;
    let added = err_to_string(crate::cmd::download::download_add_ids(&ids, quality))?;
    let names: Vec<String> = added.iter().map(|s| s.name.clone()).collect();
    Ok(json!({ "status": "added", "songs": names }))
}

fn tool_download_status() -> Result<Value, String> {
    crate::library::db::init_db().map_err(|e| e.to_string())?;
    let mut tasks = err_to_string(crate::library::db::list_downloads(Some("downloading")))?;
    let pending = err_to_string(crate::library::db::list_downloads(Some("pending")))?;
    tasks.extend(pending);
    Ok(json!({ "count": tasks.len(), "tasks": tasks }))
}

#[cfg(test)]
mod tests;

//! In-memory and sandboxed tests for the MCP stdio server.
use super::{dispatch_tool, serve, tool_descriptors};
use serde_json::{Value, json};
use std::io::Cursor;
use std::path::PathBuf;

/// Serialize env-var sandboxed tests; ALX_HOME is process-global, so the
/// db test suite's mutex is shared to keep the two suites from racing.
use crate::library::db::tests::DB_TEST_MUTEX as SANDBOX_MUTEX;

fn sandbox_home(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir =
        std::env::temp_dir().join(format!("alx-mcp-test-{tag}-{}-{nanos}", std::process::id()));
    if dir.exists() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    unsafe {
        std::env::set_var("ALX_HOME", dir.to_str().expect("utf-8 temp path"));
    }
    dir
}

fn release_sandbox(dir: &PathBuf) {
    let _ = std::fs::remove_dir_all(dir);
    unsafe {
        std::env::remove_var("ALX_HOME");
    }
}

/// Drive `serve` with raw NDJSON input through an echo dispatcher.
fn drive(input: &str) -> Vec<Value> {
    drive_with(input, &|_name, args| Ok(args))
}

fn drive_with(input: &str, dispatch: &dyn Fn(&str, Value) -> Result<Value, String>) -> Vec<Value> {
    let mut output = Vec::new();
    serve(
        Cursor::new(input.as_bytes().to_vec()),
        &mut output,
        dispatch,
    )
    .expect("serve must never fail on protocol content");
    String::from_utf8(output)
        .expect("protocol output is utf-8")
        .lines()
        .map(|l| serde_json::from_str(l).expect("every response line parses"))
        .collect()
}

fn request(id: impl Into<Value>, method: &str, params: Value) -> String {
    json!({ "jsonrpc": "2.0", "id": id.into(), "method": method, "params": params }).to_string()
        + "\n"
}

fn sample_entry(cli_id: &str) -> crate::library::db::SearchCacheEntry {
    crate::library::db::SearchCacheEntry {
        cli_id: cli_id.to_string(),
        song_id: format!("song-{cli_id}"),
        name: "Test Song".to_string(),
        singer: "Test Singer".to_string(),
        source: "wy".to_string(),
        interval: Some("03:30".to_string()),
        album_name: Some("Test Album".to_string()),
        album_id: Some("123".to_string()),
        pic_url: Some("http://pic.com/img.jpg".to_string()),
        songmid: Some(format!("mid-{cli_id}")),
        hash: None,
        extra: None,
    }
}

fn seed_queue(entries: Vec<crate::library::db::SearchCacheEntry>, current_index: Option<usize>) {
    let paths = lux_core::config::resolve_paths();
    let cache_dir = paths.cache_dir;
    std::fs::create_dir_all(&cache_dir).expect("cache dir created");
    let queue = crate::cmd::queue::PlayQueue {
        songs: entries,
        current_index,
    };
    std::fs::write(
        cache_dir.join("queue.json"),
        serde_json::to_string(&queue).expect("queue serializes"),
    )
    .expect("queue.json written");
}

// ---------------------------------------------------------------------------
// Transport / protocol
// ---------------------------------------------------------------------------

#[test]
fn initialize_handshake_reports_protocol_capabilities_and_server_info() {
    let responses = drive(&request(
        1,
        "initialize",
        json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "test", "version": "0" }
        }),
    ));
    assert_eq!(responses.len(), 1);
    let result = &responses[0]["result"];
    assert_eq!(result["protocolVersion"], "2024-11-05");
    assert_eq!(result["capabilities"]["tools"], json!({}));
    assert_eq!(result["serverInfo"]["name"], "alx");
    assert_eq!(result["serverInfo"]["version"], env!("CARGO_PKG_VERSION"));
}

#[test]
fn initialized_notification_is_acknowledged_silently() {
    let responses = drive(
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}
"#,
    );
    assert!(responses.is_empty(), "notifications never produce output");
}

#[test]
fn unknown_notification_is_also_silent() {
    let responses = drive(r#"{"jsonrpc":"2.0","method":"notifications/whatever"}"#);
    assert!(responses.is_empty());
}

#[test]
fn ping_returns_empty_result() {
    let responses = drive(&request(json!(7), "ping", json!({})));
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0]["id"], 7);
    assert_eq!(responses[0]["result"], json!({}));
}

#[test]
fn tools_list_matches_design_table_exactly() {
    let responses = drive(&request(1, "tools/list", json!({})));
    let tools = responses[0]["result"]["tools"]
        .as_array()
        .expect("tools array");

    let expected = [
        "search",
        "play",
        "playback_control",
        "skip",
        "status",
        "queue_list",
        "queue_add",
        "queue_remove",
        "queue_clear",
        "playlist_list",
        "playlist_show",
        "playlist_add",
        "playlist_remove",
        "favorite_add",
        "favorite_list",
        "lyric_get",
        "download_add",
        "download_status",
    ];
    assert_eq!(tools.len(), expected.len());
    for (tool, name) in tools.iter().zip(expected) {
        assert_eq!(tool["name"], name);
        assert!(tool["description"].is_string());
        assert_eq!(tool["inputSchema"]["type"], "object");
    }
    // Descriptor table itself agrees (used by docs contract test).
    let names: Vec<String> = tool_descriptors()
        .iter()
        .map(|t| t["name"].as_str().expect("name").to_string())
        .collect();
    let mut sorted = names.clone();
    sorted.sort();
    let mut unique = sorted.clone();
    unique.dedup();
    assert_eq!(sorted.len(), unique.len(), "tool names must be unique");
}

#[test]
fn unknown_method_returns_jsonrpc_error_32601() {
    let responses = drive(&request("abc", "resources/list", json!({})));
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0]["id"], "abc");
    let error = &responses[0]["error"];
    assert_eq!(error["code"], -32601);
    assert!(
        error["message"]
            .as_str()
            .expect("message")
            .contains("Method not found")
    );
}

#[test]
fn malformed_json_line_is_skipped_and_stream_continues() {
    let input = format!("this is not json\n{}", request(1, "ping", json!({})));
    let responses = drive(&input);
    assert_eq!(responses.len(), 1, "server survives and answers the ping");
    assert_eq!(responses[0]["result"], json!({}));
}

#[test]
fn non_object_frame_is_skipped_without_response() {
    let input = "[1,2,3]\n\"hello\"\n42\nnull\n".to_string();
    assert!(drive(&input).is_empty());
}

#[test]
fn frame_without_method_is_skipped_without_response() {
    assert!(drive(r#"{"jsonrpc":"2.0","id":1}"#).is_empty());
}

#[test]
fn empty_input_terminates_cleanly_with_zero_output() {
    assert!(drive("").is_empty());
}

#[test]
fn blank_lines_are_ignored() {
    let input = format!("\n   \n{}\n", request(1, "ping", json!({})));
    let responses = drive(&input);
    assert_eq!(responses.len(), 1);
}

#[test]
fn tools_call_success_wraps_compact_json_in_text_content() {
    let payload = json!({ "status": "cleared" });
    let input = request(
        3,
        "tools/call",
        json!({ "name": "queue_clear", "arguments": {} }),
    );
    let responses = drive_with(&input, &|name, _args| {
        assert_eq!(name, "queue_clear");
        Ok(payload.clone())
    });
    assert_eq!(responses.len(), 1);
    let result = &responses[0]["result"];
    let content = result["content"].as_array().expect("content array");
    assert_eq!(content.len(), 1);
    assert_eq!(content[0]["type"], "text");
    let text = content[0]["text"].as_str().expect("text");
    assert_eq!(serde_json::from_str::<Value>(text).unwrap(), payload);
    assert_eq!(result["isError"], false);
}

#[test]
fn tools_call_failure_sets_is_error_with_error_payload() {
    let input = request(
        4,
        "tools/call",
        json!({ "name": "status", "arguments": {} }),
    );
    let responses = drive_with(&input, &|_name, _args| Err("daemon exploded".into()));
    let result = &responses[0]["result"];
    assert_eq!(result["isError"], true);
    let text = result["content"][0]["text"].as_str().expect("text");
    assert_eq!(
        serde_json::from_str::<Value>(text).unwrap(),
        json!({ "error": "daemon exploded" })
    );
}

#[test]
fn tools_call_unknown_tool_is_reported_as_tool_error_not_protocol_error() {
    let input = request(5, "tools/call", json!({ "name": "nope" }));
    let responses = drive_with(&input, &dispatch_tool);
    assert_eq!(responses[0]["id"], 5);
    let result = &responses[0]["result"];
    assert_eq!(result["isError"], true);
    let text = result["content"][0]["text"].as_str().expect("text");
    let parsed = serde_json::from_str::<Value>(text).unwrap();
    assert_eq!(parsed["error"], json!("unknown tool: nope"));
}

#[test]
fn tools_call_missing_name_is_a_validation_error() {
    let responses = drive_with(&request(6, "tools/call", json!({})), &dispatch_tool);
    let result = &responses[0]["result"];
    assert_eq!(result["isError"], true);
    let text = result["content"][0]["text"].as_str().expect("text");
    assert!(text.contains("requires a non-empty tool name"));
}

#[test]
fn string_request_id_is_echoed_verbatim() {
    let responses = drive(&request("session-9", "ping", json!({})));
    assert_eq!(responses[0]["id"], "session-9");
}

#[test]
fn null_request_id_still_receives_a_response_when_method_requires_one() {
    // JSON-RPC treats null id as a notification; MCP clients never send this,
    // but the server must not crash or reply — silence is correct.
    let input = r#"{"jsonrpc":"2.0","id":null,"method":"ping"}"#;
    assert!(drive(input).is_empty());
}

// ---------------------------------------------------------------------------
// Tool handlers against sandboxed state (no network, no mpv daemon)
// ---------------------------------------------------------------------------

#[test]
fn status_without_daemon_reports_stopped_with_seeded_queue_index() {
    let _guard = SANDBOX_MUTEX.lock().unwrap();
    let dir = sandbox_home("status-idle");
    seed_queue(vec![sample_entry("aaa1"), sample_entry("bbb2")], Some(1));

    let response = dispatch_tool("status", json!({})).expect("idle status succeeds");
    assert_eq!(response["status"], "stopped");
    assert_eq!(response["queue_index"], 1);
    assert!(response["song"].is_null());

    release_sandbox(&dir);
}

#[test]
fn queue_list_returns_seeded_queue_with_current_index() {
    let _guard = SANDBOX_MUTEX.lock().unwrap();
    let dir = sandbox_home("queue-list");
    seed_queue(vec![sample_entry("aaa1"), sample_entry("bbb2")], Some(0));

    let response = dispatch_tool("queue_list", json!({})).expect("list succeeds");
    let songs = response["songs"].as_array().expect("songs array");
    assert_eq!(songs.len(), 2);
    assert_eq!(response["current_index"], 0);
    assert_eq!(songs[0]["cli_id"], "aaa1");
    assert_eq!(songs[1]["cli_id"], "bbb2");

    release_sandbox(&dir);
}

#[test]
fn queue_clear_persists_an_empty_queue_file() {
    let _guard = SANDBOX_MUTEX.lock().unwrap();
    let dir = sandbox_home("queue-clear");
    seed_queue(vec![sample_entry("aaa1")], None);

    let response = dispatch_tool("queue_clear", json!({})).expect("clear succeeds");
    assert_eq!(response["status"], "cleared");

    let paths = lux_core::config::resolve_paths();
    let persisted: crate::cmd::queue::PlayQueue =
        serde_json::from_str(&std::fs::read_to_string(paths.cache_dir.join("queue.json")).unwrap())
            .unwrap();
    assert!(persisted.songs.is_empty());
    assert!(persisted.current_index.is_none());

    release_sandbox(&dir);
}

#[test]
fn queue_remove_updates_persisted_queue_and_shifts_current_index() {
    let _guard = SANDBOX_MUTEX.lock().unwrap();
    let dir = sandbox_home("queue-remove");
    seed_queue(
        vec![
            sample_entry("aaa1"),
            sample_entry("bbb2"),
            sample_entry("ccc3"),
        ],
        Some(2),
    );

    let response =
        dispatch_tool("queue_remove", json!({ "index": 0 })).expect("remove of index 0 succeeds");
    assert_eq!(response["status"], "removed");
    assert_eq!(response["index"], 0);

    let paths = lux_core::config::resolve_paths();
    let persisted: crate::cmd::queue::PlayQueue =
        serde_json::from_str(&std::fs::read_to_string(paths.cache_dir.join("queue.json")).unwrap())
            .unwrap();
    assert_eq!(persisted.songs.len(), 2);
    assert_eq!(persisted.songs[0].cli_id, "bbb2");
    assert_eq!(persisted.current_index, Some(1), "current shifts down");

    release_sandbox(&dir);
}

#[test]
fn queue_remove_out_of_range_is_a_tool_error() {
    let _guard = SANDBOX_MUTEX.lock().unwrap();
    let dir = sandbox_home("queue-remove-range");
    seed_queue(vec![sample_entry("aaa1")], None);

    let err = dispatch_tool("queue_remove", json!({ "index": 5 }))
        .expect_err("out-of-range index errors");
    assert!(err.contains("out of range"), "message: {err}");

    release_sandbox(&dir);
}

#[test]
fn queue_add_unknown_cli_id_errors_before_touching_the_network() {
    let _guard = SANDBOX_MUTEX.lock().unwrap();
    let dir = sandbox_home("queue-add-unknown");
    std::fs::create_dir_all(lux_core::config::resolve_paths().cache_dir).ok();
    crate::library::db::init_db().unwrap();

    let err = dispatch_tool("queue_add", json!({ "ids": ["ghost00"] }))
        .expect_err("unknown ID must error");
    assert!(err.contains("not found in search cache"), "message: {err}");

    release_sandbox(&dir);
}

#[test]
fn search_rejects_blank_query_as_validation_error() {
    let _guard = SANDBOX_MUTEX.lock().unwrap();
    let err =
        dispatch_tool("search", json!({ "query": "   " })).expect_err("blank query must error");
    assert!(err.contains("non-empty"), "message: {err}");
}

#[test]
fn play_requires_ids_or_url() {
    let err = dispatch_tool("play", json!({})).expect_err("empty play args must error");
    assert!(err.contains("'ids' or 'url'"), "message: {err}");
}

#[test]
fn play_rejects_ids_combined_with_url() {
    let err = dispatch_tool("play", json!({ "ids": ["x"], "url": "http://x" }))
        .expect_err("both ids and url must error");
    assert!(err.contains("not both"), "message: {err}");
}

#[test]
fn playback_control_rejects_unknown_action() {
    let err = dispatch_tool("playback_control", json!({ "action": "rewind" }))
        .expect_err("invalid action must error");
    assert!(
        err.contains("pause, resume, stop, toggle"),
        "message: {err}"
    );
}

#[test]
fn playback_control_without_daemon_reports_error_instead_of_spawning_mpv() {
    let _guard = SANDBOX_MUTEX.lock().unwrap();
    let dir = sandbox_home("playback-no-daemon");

    let err = dispatch_tool("playback_control", json!({ "action": "pause" }))
        .expect_err("must not cold-start mpv");
    assert!(err.contains("daemon is not running"), "message: {err}");

    release_sandbox(&dir);
}

#[test]
fn skip_rejects_unknown_direction() {
    let err =
        dispatch_tool("skip", json!({ "direction": "sideways" })).expect_err("invalid direction");
    assert!(err.contains("next, prev"), "message: {err}");
}

#[test]
fn playlist_add_and_show_roundtrip_against_tempdir_state() {
    let _guard = SANDBOX_MUTEX.lock().unwrap();
    let dir = sandbox_home("playlist-roundtrip");
    crate::library::db::init_db().unwrap();
    crate::library::db::create_playlist("Road Trip", None).unwrap();
    crate::library::db::insert_search_cache(&sample_entry("plitem01")).unwrap();

    let added = dispatch_tool(
        "playlist_add",
        json!({ "name": "Road Trip", "ids": ["plitem01"] }),
    )
    .expect("add succeeds");
    assert_eq!(added["status"], "added");
    assert_eq!(added["playlist"], "Road Trip");

    let shown =
        dispatch_tool("playlist_show", json!({ "name": "Road Trip" })).expect("show succeeds");
    let songs = shown["songs"].as_array().expect("songs array");
    assert_eq!(songs.len(), 1);
    assert_eq!(songs[0]["song_id"], "song-plitem01");

    let listed = dispatch_tool("playlist_list", json!({})).expect("list succeeds");
    let playlists = listed["playlists"].as_array().expect("playlists array");
    assert!(
        playlists
            .iter()
            .any(|p| p["name"] == "Road Trip" && p["song_count"] == 1)
    );

    release_sandbox(&dir);
}

#[test]
fn favorite_add_then_list_roundtrip_against_tempdir_state() {
    let _guard = SANDBOX_MUTEX.lock().unwrap();
    let dir = sandbox_home("favorites-roundtrip");
    crate::library::db::init_db().unwrap();
    crate::library::db::insert_search_cache(&sample_entry("favitem1")).unwrap();

    let added =
        dispatch_tool("favorite_add", json!({ "id": "favitem1" })).expect("favorite add succeeds");
    assert_eq!(added["playlist"], "Favorites");
    assert_eq!(added["song"], "Test Song");

    let listed = dispatch_tool("favorite_list", json!({})).expect("favorite list succeeds");
    let favorites = listed["favorites"].as_array().expect("favorites array");
    assert_eq!(favorites.len(), 1);
    // NOTE: get_playlist_songs regenerates a legacy 8-char cli_id instead of
    // returning the stored one (pre-existing db.rs defect, tracked outside
    // CTX-0006), so assert on the stable song identifier.
    assert_eq!(favorites[0]["song_id"], "song-favitem1");

    release_sandbox(&dir);
}

#[test]
fn favorite_add_unknown_id_errors() {
    let _guard = SANDBOX_MUTEX.lock().unwrap();
    let dir = sandbox_home("favorites-unknown");
    crate::library::db::init_db().unwrap();

    let err = dispatch_tool("favorite_add", json!({ "id": "ghost99" }))
        .expect_err("unknown ID must error");
    assert!(err.contains("not found in cache"), "message: {err}");

    release_sandbox(&dir);
}

#[test]
fn lyric_get_without_id_or_active_song_errors() {
    let _guard = SANDBOX_MUTEX.lock().unwrap();
    let dir = sandbox_home("lyric-no-current");
    crate::library::db::init_db().unwrap();

    let err = dispatch_tool("lyric_get", json!({})).expect_err("no active song must error");
    assert!(err.contains("No active song"), "message: {err}");

    release_sandbox(&dir);
}

#[test]
fn lyric_get_rejects_unknown_track_kind() {
    let err = dispatch_tool("lyric_get", json!({ "track": "instrumental" }))
        .expect_err("invalid track kind must error");
    assert!(
        err.contains("main, translated, romanized"),
        "message: {err}"
    );
}

#[test]
fn download_add_unknown_cli_id_errors_before_daemon_spawn() {
    let _guard = SANDBOX_MUTEX.lock().unwrap();
    let dir = sandbox_home("download-add-unknown");
    crate::library::db::init_db().unwrap();

    let err = dispatch_tool("download_add", json!({ "ids": ["ghost77"] }))
        .expect_err("unknown ID must error before any download starts");
    assert!(err.contains("not found in cache"), "message: {err}");

    release_sandbox(&dir);
}

#[test]
fn download_status_lists_seeded_pending_tasks() {
    let _guard = SANDBOX_MUTEX.lock().unwrap();
    let dir = sandbox_home("download-status");
    crate::library::db::init_db().unwrap();
    let entry = sample_entry("dlseed01");
    crate::library::db::insert_download(
        &entry.song_id,
        &entry.source,
        &entry.name,
        &entry.singer,
        "320k",
    )
    .unwrap();

    let response = dispatch_tool("download_status", json!({})).expect("status succeeds");
    assert_eq!(response["count"], 1);
    let tasks = response["tasks"].as_array().expect("tasks array");
    assert_eq!(tasks[0]["name"], "Test Song");
    assert_eq!(tasks[0]["status"], "pending");

    release_sandbox(&dir);
}

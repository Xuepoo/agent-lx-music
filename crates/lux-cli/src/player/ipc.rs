use anyhow::{Result, anyhow};
use serde_json::json;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

pub fn send_mpv_command(
    socket_path: &Path,
    args: Vec<serde_json::Value>,
) -> Result<serde_json::Value> {
    let mut stream = UnixStream::connect(socket_path)
        .map_err(|e| anyhow!("Failed to connect to mpv socket: {}", e))?;

    stream.set_read_timeout(Some(Duration::from_millis(500)))?;
    stream.set_write_timeout(Some(Duration::from_millis(500)))?;

    let cmd_payload = json!({
        "command": args
    });

    let payload_str = format!("{}\n", cmd_payload);
    stream.write_all(payload_str.as_bytes())?;

    let reader = BufReader::new(stream);
    for line in reader.lines() {
        let line = line?;
        if let Ok(response) = serde_json::from_str::<serde_json::Value>(&line) {
            // Ignore events, we want the response to our command
            if response.get("event").is_some() {
                continue;
            }
            if let Some(err_val) = response.get("error") {
                if err_val.as_str() == Some("success") {
                    return Ok(response
                        .get("data")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null));
                } else {
                    return Err(anyhow!("mpv error: {}", err_val));
                }
            }
        }
    }

    Err(anyhow!("No response from mpv"))
}

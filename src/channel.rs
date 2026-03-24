use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;

use serde_json::{Value, json};

const INSTRUCTIONS: &str = "\
Events from the mandelbot terminal host arrive as <channel source=\"mandelbot\">.";

fn main() {
    let socket_path = std::env::var("MANDELBOT_SOCKET").unwrap_or_else(|_| {
        eprintln!("mandelbot-channel: MANDELBOT_SOCKET not set");
        std::process::exit(1);
    });

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut reader = BufReader::new(stdin.lock());
    let mut writer = stdout.lock();

    // Wait for the initialize request and respond.
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            return;
        }

        let msg: Value = match serde_json::from_str(line.trim()) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if msg.get("method").and_then(|m| m.as_str()) == Some("initialize") {
            let id = msg.get("id").cloned().unwrap_or(Value::Null);
            let protocol_version = msg
                .pointer("/params/protocolVersion")
                .and_then(|v| v.as_str())
                .unwrap_or("2025-03-26");

            let response = json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": protocol_version,
                    "capabilities": {
                        "experimental": {
                            "claude/channel": {}
                        }
                    },
                    "serverInfo": {
                        "name": "mandelbot",
                        "version": "0.0.1"
                    },
                    "instructions": INSTRUCTIONS
                }
            });

            writeln!(writer, "{}", response).unwrap();
            writer.flush().unwrap();
            break;
        }
    }

    // Clean up any stale socket, then listen.
    let _ = std::fs::remove_file(&socket_path);
    let listener = UnixListener::bind(&socket_path).unwrap_or_else(|e| {
        eprintln!("mandelbot-channel: failed to bind {socket_path}: {e}");
        std::process::exit(1);
    });

    // Read events from the Unix socket and forward as MCP notifications.
    for stream in listener.incoming() {
        let stream = match stream {
            Ok(s) => s,
            Err(_) => continue,
        };

        let stream_reader = BufReader::new(stream);
        for line in stream_reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => break,
            };
            if line.trim().is_empty() {
                continue;
            }

            let event: Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(_) => continue,
            };

            let notification = json!({
                "jsonrpc": "2.0",
                "method": "notifications/claude/channel",
                "params": {
                    "content": event.get("content").and_then(|v| v.as_str()).unwrap_or("ping"),
                }
            });

            if writeln!(writer, "{}", notification).is_err() {
                return;
            }
            let _ = writer.flush();
        }
    }
}

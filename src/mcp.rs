use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

#[derive(Deserialize)]
struct Request {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    params: Option<Value>,
}

#[derive(Serialize)]
struct Response {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError>,
}

#[derive(Serialize)]
struct RpcError {
    code: i64,
    message: String,
}

impl Response {
    fn ok(id: Value, result: Value) -> Self {
        Self { jsonrpc: "2.0", id, result: Some(result), error: None }
    }

    fn err(id: Value, code: i64, message: String) -> Self {
        Self { jsonrpc: "2.0", id, result: None, error: Some(RpcError { code, message }) }
    }
}

fn handle_initialize(id: Value) -> Response {
    Response::ok(
        id,
        serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "mandelbot", "version": "0.1.0" },
        }),
    )
}

fn handle_tools_list(id: Value) -> Response {
    Response::ok(
        id,
        serde_json::json!({
            "tools": [{
                "name": "send_message",
                "description": "Send a message to the parent session",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "text": {
                            "type": "string",
                            "description": "The message text to send",
                        },
                    },
                    "required": ["text"],
                },
            }],
        }),
    )
}

async fn handle_tools_call(
    id: Value,
    params: Option<Value>,
    session_id: &str,
    parent: &mut tokio::io::WriteHalf<UnixStream>,
) -> Response {
    let Some(params) = params else {
        return Response::err(id, -32602, "Missing params".into());
    };

    let tool_name = params
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    match tool_name {
        "send_message" => {
            let text = params
                .get("arguments")
                .and_then(|a| a.get("text"))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let msg = serde_json::json!({
                "type": "message",
                "session_id": session_id,
                "text": text,
            });
            let mut msg_str = serde_json::to_string(&msg).unwrap();
            msg_str.push('\n');

            if let Err(e) = parent.write_all(msg_str.as_bytes()).await {
                return Response::err(id, -32000, format!("Failed to send: {e}"));
            }
            let _ = parent.flush().await;

            Response::ok(
                id,
                serde_json::json!({
                    "content": [{ "type": "text", "text": "Message sent" }],
                }),
            )
        }
        _ => Response::err(id, -32601, format!("Unknown tool: {tool_name}")),
    }
}

/// Run the MCP server over stdin/stdout (for Claude Code) with a parent socket
/// for relaying messages back to the mandelbot application.
pub async fn run(
    session_id: &str,
    parent_socket: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let parent = UnixStream::connect(parent_socket).await?;
    let (_, mut parent_writer) = tokio::io::split(parent);

    let stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let mut reader = BufReader::new(stdin);
    let mut line = String::new();

    loop {
        line.clear();
        if reader.read_line(&mut line).await? == 0 {
            break;
        }

        let request: Request = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let resp = Response::err(
                    Value::Null,
                    -32700,
                    format!("Parse error: {e}"),
                );
                let mut out = serde_json::to_string(&resp)?;
                out.push('\n');
                stdout.write_all(out.as_bytes()).await?;
                stdout.flush().await?;
                continue;
            }
        };

        // Notifications (no id) get no response.
        let Some(id) = request.id else {
            continue;
        };

        let response = match request.method.as_str() {
            "initialize" => handle_initialize(id),
            "tools/list" => handle_tools_list(id),
            "tools/call" => {
                handle_tools_call(id, request.params, session_id, &mut parent_writer).await
            }
            _ => Response::err(id, -32601, format!("Method not found: {}", request.method)),
        };

        let mut out = serde_json::to_string(&response)?;
        out.push('\n');
        stdout.write_all(out.as_bytes()).await?;
        stdout.flush().await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, Write};
    use std::os::unix::net as unix;

    #[test]
    fn test_send_message_flow() {
        let dir = std::env::temp_dir().join(format!("mandelbot-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let parent_sock = dir.join("parent.sock");

        let parent_listener = unix::UnixListener::bind(&parent_sock).unwrap();

        // current_exe() in tests points to the test runner binary. The main
        // binary lives in the same directory under the crate name.
        let exe = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("mandelbot");
        let mut child = std::process::Command::new(&exe)
            .arg("--mcp-server")
            .arg("--session-id")
            .arg("tab-42")
            .arg("--parent-socket")
            .arg(&parent_sock)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .unwrap();

        let mut child_stdin = child.stdin.take().unwrap();
        let child_stdout = child.stdout.take().unwrap();
        let mut child_reader = std::io::BufReader::new(child_stdout);

        // Accept the MCP server's connection to parent.
        let (parent_stream, _) = parent_listener.accept().unwrap();
        let mut parent_reader = std::io::BufReader::new(parent_stream);

        // Helper to send a request and read the response.
        let mut resp_line = String::new();
        let mut send = |json: &str, stdin: &mut dyn Write, reader: &mut dyn BufRead| -> String {
            stdin.write_all(json.as_bytes()).unwrap();
            stdin.write_all(b"\n").unwrap();
            stdin.flush().unwrap();
            resp_line.clear();
            reader.read_line(&mut resp_line).unwrap();
            resp_line.clone()
        };

        // -- initialize --
        let init = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"0.1.0"}}}"#;
        let resp = send(init, &mut child_stdin, &mut child_reader);
        let resp: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(resp["result"]["serverInfo"]["name"], "mandelbot");

        // -- initialized notification (no response expected) --
        child_stdin
            .write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n")
            .unwrap();
        child_stdin.flush().unwrap();

        // -- tools/list --
        let list = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#;
        let resp = send(list, &mut child_stdin, &mut child_reader);
        let resp: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(resp["result"]["tools"][0]["name"], "send_message");

        // -- tools/call send_message --
        let call = r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"send_message","arguments":{"text":"hello from agent"}}}"#;
        let resp = send(call, &mut child_stdin, &mut child_reader);
        let resp: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(resp["result"]["content"][0]["text"], "Message sent");

        // Verify parent received the message.
        let mut parent_line = String::new();
        parent_reader.read_line(&mut parent_line).unwrap();
        let parent_msg: serde_json::Value = serde_json::from_str(&parent_line).unwrap();
        assert_eq!(parent_msg["type"], "message");
        assert_eq!(parent_msg["session_id"], "tab-42");
        assert_eq!(parent_msg["text"], "hello from agent");

        // Close stdin to shut down the server.
        drop(child_stdin);
        child.wait().unwrap();

        let _ = std::fs::remove_dir_all(&dir);
    }
}

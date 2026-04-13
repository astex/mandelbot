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
            "tools": [
                {
                    "name": "set_title",
                    "description": "Set the title of this tab in the parent application",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "title": {
                                "type": "string",
                                "description": "The title to display on this tab",
                            },
                        },
                        "required": ["title"],
                    },
                },
                {
                    "name": "spawn_tab",
                    "description": "Spawn a new agent tab. From the home agent: pass working_directory to create a project agent, or pass project_tab_id to create a task agent under an existing project. From a project agent: creates a task agent (no arguments needed). From a task agent: creates a child task agent nested under this task (no arguments needed).",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "working_directory": {
                                "type": "string",
                                "description": "Absolute path to the project directory. Used from the home agent to spawn a project agent.",
                            },
                            "project_tab_id": {
                                "type": "integer",
                                "description": "Tab ID of an existing project agent. Used from the home agent to spawn a task agent under that project.",
                            },
                            "prompt": {
                                "type": "string",
                                "description": "Initial prompt to send to the spawned agent.",
                            },
                            "branch": {
                                "type": "string",
                                "description": "Git branch name for the task agent's worktree. Used as the worktree directory name and the branch to create.",
                            },
                            "model": {
                                "type": "string",
                                "description": "Model override for the spawned agent (e.g. 'sonnet', 'opus', 'haiku'). Defaults to the model configured for the agent's rank.",
                            },
                            "base": {
                                "type": "string",
                                "description": "Base commit, branch, or ref for the new worktree's branch to start from. Defaults to HEAD of the project.",
                            },
                        },
                    },
                },
                {
                    "name": "set_status",
                    "description": "Set the status indicator for this tab. Use this to communicate your current state to the user.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "status": {
                                "type": "string",
                                "enum": ["idle", "working", "compacting", "blocked", "needs_review", "error"],
                                "description": "idle = waiting for user input, working = actively processing, compacting = context is being compressed, blocked = waiting for permission, needs_review = presenting plan/output for review, error = something went wrong",
                            },
                        },
                        "required": ["status"],
                    },
                },
                {
                    "name": "close_tab",
                    "description": "Close a tab by ID. You can close yourself or any of your descendant tabs. Closing a tab also closes all of its descendants.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "tab_id": {
                                "type": "integer",
                                "description": "The tab ID to close. Must be your own tab or a descendant.",
                            },
                        },
                        "required": ["tab_id"],
                    },
                },
            ],
        }),
    )
}

async fn send_to_parent(
    parent_writer: &mut tokio::io::WriteHalf<UnixStream>,
    msg: Value,
) -> Result<(), String> {
    let mut msg_str = serde_json::to_string(&msg).unwrap();
    msg_str.push('\n');
    parent_writer
        .write_all(msg_str.as_bytes())
        .await
        .map_err(|e| format!("Failed to send: {e}"))?;
    parent_writer
        .flush()
        .await
        .map_err(|e| format!("Failed to flush: {e}"))?;
    Ok(())
}

async fn read_from_parent(
    parent_reader: &mut BufReader<tokio::io::ReadHalf<UnixStream>>,
) -> Result<Value, String> {
    let mut line = String::new();
    parent_reader
        .read_line(&mut line)
        .await
        .map_err(|e| format!("Failed to read response: {e}"))?;
    serde_json::from_str(&line).map_err(|e| format!("Failed to parse response: {e}"))
}

async fn handle_tools_call(
    id: Value,
    params: Option<Value>,
    tab_id: &str,
    parent_writer: &mut tokio::io::WriteHalf<UnixStream>,
    parent_reader: &mut BufReader<tokio::io::ReadHalf<UnixStream>>,
) -> Response {
    let Some(params) = params else {
        return Response::err(id, -32602, "Missing params".into());
    };

    let tool_name = params
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    match tool_name {
        "set_title" => {
            let title = params
                .get("arguments")
                .and_then(|a| a.get("title"))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let msg = serde_json::json!({
                "type": "set_title",
                "tab_id": tab_id,
                "title": title,
            });

            if let Err(e) = send_to_parent(parent_writer, msg).await {
                return Response::err(id, -32000, e);
            }

            Response::ok(
                id,
                serde_json::json!({
                    "content": [{ "type": "text", "text": "Title set" }],
                }),
            )
        }
        "spawn_tab" => {
            let args = params.get("arguments");
            let working_directory = args
                .and_then(|a| a.get("working_directory"))
                .and_then(|v| v.as_str());
            let project_tab_id = args
                .and_then(|a| a.get("project_tab_id"))
                .and_then(|v| v.as_u64());
            let prompt = args
                .and_then(|a| a.get("prompt"))
                .and_then(|v| v.as_str());
            let branch = args
                .and_then(|a| a.get("branch"))
                .and_then(|v| v.as_str());
            let model = args
                .and_then(|a| a.get("model"))
                .and_then(|v| v.as_str());
            let base = args
                .and_then(|a| a.get("base"))
                .and_then(|v| v.as_str());

            let mut msg = serde_json::json!({
                "type": "spawn_tab",
                "tab_id": tab_id,
            });
            if let Some(wd) = working_directory {
                msg["working_directory"] = Value::String(wd.to_string());
            }
            if let Some(ptid) = project_tab_id {
                msg["project_tab_id"] = Value::Number(ptid.into());
            }
            if let Some(p) = prompt {
                msg["prompt"] = Value::String(p.to_string());
            }
            if let Some(b) = branch {
                msg["branch"] = Value::String(b.to_string());
            }
            if let Some(m) = model {
                msg["model"] = Value::String(m.to_string());
            }
            if let Some(b) = base {
                msg["base"] = Value::String(b.to_string());
            }

            if let Err(e) = send_to_parent(parent_writer, msg).await {
                return Response::err(id, -32000, e);
            }

            match read_from_parent(parent_reader).await {
                Ok(resp) => {
                    let new_tab_id = resp
                        .get("tab_id")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    Response::ok(
                        id,
                        serde_json::json!({
                            "content": [{ "type": "text", "text": format!("Agent spawned with tab ID {new_tab_id}") }],
                        }),
                    )
                }
                Err(e) => Response::err(id, -32000, e),
            }
        }
        "set_status" => {
            let status = params
                .get("arguments")
                .and_then(|a| a.get("status"))
                .and_then(|v| v.as_str())
                .unwrap_or("idle");

            let msg = serde_json::json!({
                "type": "set_status",
                "tab_id": tab_id,
                "status": status,
            });

            if let Err(e) = send_to_parent(parent_writer, msg).await {
                return Response::err(id, -32000, e);
            }

            Response::ok(
                id,
                serde_json::json!({
                    "content": [{ "type": "text", "text": "Status set" }],
                }),
            )
        }
        "close_tab" => {
            let target = params
                .get("arguments")
                .and_then(|a| a.get("tab_id"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);

            let msg = serde_json::json!({
                "type": "close_tab",
                "tab_id": tab_id,
                "target_tab_id": target,
            });

            if let Err(e) = send_to_parent(parent_writer, msg).await {
                return Response::err(id, -32000, e);
            }

            match read_from_parent(parent_reader).await {
                Ok(resp) => {
                    let text = resp
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Tab closed");
                    Response::ok(
                        id,
                        serde_json::json!({
                            "content": [{ "type": "text", "text": text }],
                        }),
                    )
                }
                Err(e) => Response::err(id, -32000, e),
            }
        }
        _ => Response::err(id, -32601, format!("Unknown tool: {tool_name}")),
    }
}

/// Run the MCP server over stdin/stdout (for Claude Code) with a parent socket
/// for relaying messages back to the mandelbot application.
pub async fn run(
    tab_id: &str,
    parent_socket: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let parent = UnixStream::connect(parent_socket).await?;
    let (parent_read, mut parent_writer) = tokio::io::split(parent);
    let mut parent_reader = BufReader::new(parent_read);

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
                handle_tools_call(
                    id, request.params, tab_id,
                    &mut parent_writer, &mut parent_reader,
                ).await
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
    fn test_mcp_flow() {
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
            .env("MANDELBOT_TAB_ID", "tab-42")
            .env("MANDELBOT_PARENT_SOCKET", &parent_sock)
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
        assert_eq!(resp["result"]["tools"][0]["name"], "set_title");
        assert_eq!(resp["result"]["tools"][1]["name"], "spawn_tab");
        assert_eq!(resp["result"]["tools"][2]["name"], "set_status");
        assert_eq!(resp["result"]["tools"][3]["name"], "close_tab");

        // -- tools/call set_title --
        let call = r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"set_title","arguments":{"title":"my cool tab"}}}"#;
        let resp = send(call, &mut child_stdin, &mut child_reader);
        let resp: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(resp["result"]["content"][0]["text"], "Title set");

        // Verify parent received the set_title message.
        let mut parent_line = String::new();
        parent_reader.read_line(&mut parent_line).unwrap();
        let parent_msg: serde_json::Value = serde_json::from_str(&parent_line).unwrap();
        assert_eq!(parent_msg["type"], "set_title");
        assert_eq!(parent_msg["tab_id"], "tab-42");
        assert_eq!(parent_msg["title"], "my cool tab");

        // -- tools/call spawn_tab --
        // Get a writer to the parent stream so we can send a response back.
        let parent_writer = parent_reader.get_ref().try_clone().unwrap();

        let call = r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"spawn_tab","arguments":{"working_directory":"/tmp/test-project","model":"sonnet"}}}"#;
        child_stdin.write_all(call.as_bytes()).unwrap();
        child_stdin.write_all(b"\n").unwrap();
        child_stdin.flush().unwrap();

        // Parent receives the spawn_tab message.
        parent_line.clear();
        parent_reader.read_line(&mut parent_line).unwrap();
        let parent_msg: serde_json::Value = serde_json::from_str(&parent_line).unwrap();
        assert_eq!(parent_msg["type"], "spawn_tab");
        assert_eq!(parent_msg["tab_id"], "tab-42");
        assert_eq!(parent_msg["working_directory"], "/tmp/test-project");
        assert_eq!(parent_msg["model"], "sonnet");

        // Parent writes back the new tab ID.
        let mut parent_writer = parent_writer;
        parent_writer.write_all(b"{\"tab_id\":7}\n").unwrap();
        parent_writer.flush().unwrap();

        // MCP server returns the new tab ID to Claude.
        resp_line.clear();
        child_reader.read_line(&mut resp_line).unwrap();
        let resp: serde_json::Value = serde_json::from_str(&resp_line).unwrap();
        assert_eq!(resp["result"]["content"][0]["text"], "Agent spawned with tab ID 7");

        // -- tools/call set_status --
        let call = r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"set_status","arguments":{"status":"working"}}}"#;
        child_stdin.write_all(call.as_bytes()).unwrap();
        child_stdin.write_all(b"\n").unwrap();
        child_stdin.flush().unwrap();

        resp_line.clear();
        child_reader.read_line(&mut resp_line).unwrap();
        let resp: serde_json::Value = serde_json::from_str(&resp_line).unwrap();
        assert_eq!(resp["result"]["content"][0]["text"], "Status set");

        // Verify parent received the set_status message.
        parent_line.clear();
        parent_reader.read_line(&mut parent_line).unwrap();
        let parent_msg: serde_json::Value = serde_json::from_str(&parent_line).unwrap();
        assert_eq!(parent_msg["type"], "set_status");
        assert_eq!(parent_msg["tab_id"], "tab-42");
        assert_eq!(parent_msg["status"], "working");

        // -- tools/call close_tab --
        let call = r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"close_tab","arguments":{"tab_id":7}}}"#;
        child_stdin.write_all(call.as_bytes()).unwrap();
        child_stdin.write_all(b"\n").unwrap();
        child_stdin.flush().unwrap();

        // Parent receives the close_tab message.
        parent_line.clear();
        parent_reader.read_line(&mut parent_line).unwrap();
        let parent_msg: serde_json::Value = serde_json::from_str(&parent_line).unwrap();
        assert_eq!(parent_msg["type"], "close_tab");
        assert_eq!(parent_msg["tab_id"], "tab-42");
        assert_eq!(parent_msg["target_tab_id"], 7);

        // Parent writes back a success response.
        parent_writer.write_all(b"{\"message\":\"Closed 1 tab(s)\"}\n").unwrap();
        parent_writer.flush().unwrap();

        // MCP server returns the response to Claude.
        resp_line.clear();
        child_reader.read_line(&mut resp_line).unwrap();
        let resp: serde_json::Value = serde_json::from_str(&resp_line).unwrap();
        assert_eq!(resp["result"]["content"][0]["text"], "Closed 1 tab(s)");

        // Close stdin to shut down the server.
        drop(child_stdin);
        child.wait().unwrap();

        let _ = std::fs::remove_dir_all(&dir);
    }
}

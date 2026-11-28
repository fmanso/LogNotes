//! LogNotes' MCP adapter.
//!
//! The agent client launches this binary over stdio. Every tool call is forwarded to the running
//! LogNotes app over its loopback control channel; this process never opens the database. If the app is
//! not running, the tools say so instead of inventing an answer.

use std::env;
use std::io::{self, BufRead, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};

const CONTROL_FILE: &str = "lognotes-agent.json";
const PROTOCOL_VERSION: &str = "2024-11-05";

struct Server {
    data_dir: PathBuf,
}

fn main() {
    let data_dir = match data_dir_from_args() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("lognotes-mcp: {error}");
            std::process::exit(2);
        }
    };
    let server = Server { data_dir };
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else {
            break;
        };
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Value>(&line) {
            Ok(message) => handle_message(&server, message),
            Err(_) => Some(error_response(Value::Null, -32700, "invalid JSON")),
        };
        if let Some(response) = response {
            if writeln!(stdout, "{response}").is_err() {
                break;
            }
            let _ = stdout.flush();
        }
    }
}

fn data_dir_from_args() -> Result<PathBuf, String> {
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--data-dir" {
            return args
                .next()
                .map(PathBuf::from)
                .ok_or_else(|| "--data-dir needs a path".to_string());
        }
        if let Some(path) = arg.strip_prefix("--data-dir=") {
            return Ok(PathBuf::from(path));
        }
    }
    default_data_dir()
}

fn default_data_dir() -> Result<PathBuf, String> {
    if let Ok(path) = env::var("LOGNOTES_DATA_DIR") {
        if !path.trim().is_empty() {
            return Ok(PathBuf::from(path));
        }
    }
    #[cfg(windows)]
    {
        let base = env::var_os("LOCALAPPDATA")
            .or_else(|| env::var_os("APPDATA"))
            .ok_or_else(|| "neither LOCALAPPDATA nor APPDATA is set".to_string())?;
        return Ok(PathBuf::from(base).join("LogNotes"));
    }
    #[cfg(not(windows))]
    {
        if let Some(base) = env::var_os("XDG_DATA_HOME") {
            return Ok(PathBuf::from(base).join("lognotes"));
        }
        let home = env::var_os("HOME").ok_or_else(|| "HOME is not set".to_string())?;
        Ok(PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("lognotes"))
    }
}

fn handle_message(server: &Server, message: Value) -> Option<Value> {
    let id = message.get("id").cloned();
    let method = message.get("method").and_then(Value::as_str)?;

    match method {
        "initialize" => Some(result_response(
            id?,
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {"tools": {}},
                "serverInfo": {
                    "name": "lognotes",
                    "version": env!("CARGO_PKG_VERSION"),
                },
                "instructions": "LogNotes must be running. Read project context, then apply one consolidation. The app remains the only writer."
            }),
        )),
        "notifications/initialized" => None,
        "ping" => Some(result_response(id?, json!({}))),
        "tools/list" => Some(result_response(id?, json!({"tools": tools()}))),
        "tools/call" => {
            let id = id?;
            let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let arguments = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            match call_tool(server, name, arguments) {
                Ok(text) => Some(result_response(
                    id,
                    json!({
                        "content": [{"type": "text", "text": text}],
                        "isError": false,
                    }),
                )),
                Err(error) => Some(result_response(
                    id,
                    json!({
                        "content": [{"type": "text", "text": error}],
                        "isError": true,
                    }),
                )),
            }
        }
        _ => id.map(|id| error_response(id, -32601, "method not found")),
    }
}

fn result_response(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn error_response(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

fn call_tool(server: &Server, name: &str, arguments: Value) -> Result<String, String> {
    let (method, params) = translate(name, arguments)?;
    post(&server.data_dir, method, params)
}

fn translate(name: &str, arguments: Value) -> Result<(&'static str, Value), String> {
    let pair = match name {
        "get_project_context" => (
            "project_context",
            json!({
                "page": required(&arguments, "page_name")?,
                "since": arguments.get("since").cloned().unwrap_or(Value::Null),
                "full": arguments.get("full").and_then(Value::as_bool).unwrap_or(false),
            }),
        ),
        "search_notes" => (
            "search_notes",
            json!({
                "query": required(&arguments, "query")?,
                "limit": arguments.get("limit").cloned().unwrap_or(Value::Null),
            }),
        ),
        "get_page" => ("get_page", json!({"name": required(&arguments, "name")?})),
        "get_day" => ("get_day", json!({"date": required(&arguments, "date")?})),
        "get_agenda" => ("get_agenda", json!({})),
        "get_backlinks" => (
            "get_backlinks",
            json!({"page": required(&arguments, "page")?}),
        ),
        "apply_project_consolidation" => ("project_consolidation", arguments),
        other => return Err(format!("unknown tool: {other}")),
    };
    Ok(pair)
}

fn required(arguments: &Value, key: &str) -> Result<Value, String> {
    arguments
        .get(key)
        .cloned()
        .ok_or_else(|| format!("{key} is required"))
}

fn post(data_dir: &Path, method: &str, params: Value) -> Result<String, String> {
    let control = read_control(data_dir)?;
    let mut stream = TcpStream::connect(("127.0.0.1", control.port)).map_err(|_| {
        "LogNotes is not running. Start the app and retry; the bridge is local-only.".to_string()
    })?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(20)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(20)));

    let body = json!({"method": method, "params": params}).to_string();
    let request = format!(
        "POST /rpc HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        control.token,
        body.len(),
        body
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|error| format!("could not send the request to LogNotes: {error}"))?;

    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .map_err(|error| format!("could not read LogNotes' answer: {error}"))?;
    parse_http(&raw)
}

fn read_control(data_dir: &Path) -> Result<Control, String> {
    let path = data_dir.join(CONTROL_FILE);
    let text = std::fs::read_to_string(&path).map_err(|_| {
        format!(
            "LogNotes is not running (no control file at {}). Start the app and retry.",
            path.display()
        )
    })?;
    let value: Value = serde_json::from_str(&text)
        .map_err(|_| format!("the LogNotes control file at {} is invalid", path.display()))?;
    let port = value
        .get("port")
        .and_then(Value::as_u64)
        .ok_or_else(|| "the LogNotes control file has no port".to_string())? as u16;
    let token = value
        .get("token")
        .and_then(Value::as_str)
        .ok_or_else(|| "the LogNotes control file has no token".to_string())?
        .to_string();
    Ok(Control { port, token })
}

struct Control {
    port: u16,
    token: String,
}

fn parse_http(raw: &[u8]) -> Result<String, String> {
    let split = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| "LogNotes sent an incomplete HTTP response".to_string())?;
    let head = String::from_utf8_lossy(&raw[..split]);
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| "LogNotes sent an HTTP response without a status".to_string())?;
    let body = String::from_utf8_lossy(&raw[split + 4..]).to_string();
    if status == 200 {
        Ok(body)
    } else {
        let message = serde_json::from_str::<Value>(&body)
            .ok()
            .and_then(|value| {
                value
                    .get("error")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or(body);
        Err(message)
    }
}

fn tools() -> Value {
    json!([
        {
            "name": "get_project_context",
            "description": "Read a named project page, its agent digest, its human notes, and every explicit journal source root with its children and ancestor framing. The app must be running.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "page_name": {"type": "string", "description": "The exact project page name."},
                    "since": {"type": "string", "description": "Only sources after this YYYY-MM-DD journal day."},
                    "full": {"type": "boolean", "description": "Read all sources, ignoring the last-consolidated date."}
                },
                "required": ["page_name"]
            }
        },
        {
            "name": "apply_project_consolidation",
            "description": "Write one project digest, timeline, and agent tasks as a single undoable action. Only the agent-owned digest section and the marked agent task block are touched. The app must be running.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "page": {"type": "string"},
                    "through": {"type": "string", "description": "The newest source day, YYYY-MM-DD."},
                    "brief": {
                        "type": "object",
                        "properties": {
                            "current_state": {"$ref": "#/definitions/brief_items"},
                            "decisions": {"$ref": "#/definitions/brief_items"},
                            "open_questions": {"$ref": "#/definitions/brief_items"}
                        }
                    },
                    "timeline_entries": {"$ref": "#/definitions/timeline_entries"},
                    "tasks_to_create": {"$ref": "#/definitions/tasks"},
                    "task_updates": {"$ref": "#/definitions/task_updates"}
                },
                "required": ["page", "through"],
                "definitions": {
                    "brief_item": {
                        "type": "object",
                        "properties": {
                            "text": {"type": "string"},
                            "sources": {"type": "array", "items": {"type": "string"}}
                        },
                        "required": ["text"]
                    },
                    "brief_items": {
                        "type": "array",
                        "items": {"$ref": "#/definitions/brief_item"}
                    },
                    "timeline_entry": {
                        "type": "object",
                        "properties": {
                            "date": {"type": "string"},
                            "text": {"type": "string"}
                        },
                        "required": ["date", "text"]
                    },
                    "timeline_entries": {
                        "type": "array",
                        "items": {"$ref": "#/definitions/timeline_entry"}
                    },
                    "task": {
                        "type": "object",
                        "properties": {
                            "text": {"type": "string"},
                            "source_day": {"type": "string"}
                        },
                        "required": ["text", "source_day"]
                    },
                    "tasks": {
                        "type": "array",
                        "items": {"$ref": "#/definitions/task"}
                    },
                    "task_update": {
                        "type": "object",
                        "properties": {
                            "block_id": {"type": "integer"},
                            "marker": {"type": "string", "enum": ["DONE"]}
                        },
                        "required": ["block_id", "marker"]
                    },
                    "task_updates": {
                        "type": "array",
                        "items": {"$ref": "#/definitions/task_update"}
                    }
                }
            }
        },
        {
            "name": "search_notes",
            "description": "Search page names and block text in LogNotes. The app must be running.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {"type": "string"},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 100}
                },
                "required": ["query"]
            }
        },
        {
            "name": "get_page",
            "description": "Read a page's blocks and references. The app must be running.",
            "inputSchema": {
                "type": "object",
                "properties": {"name": {"type": "string"}},
                "required": ["name"]
            }
        },
        {
            "name": "get_day",
            "description": "Read a journal day's blocks. The app must be running.",
            "inputSchema": {
                "type": "object",
                "properties": {"date": {"type": "string"}},
                "required": ["date"]
            }
        },
        {
            "name": "get_agenda",
            "description": "Read the scheduled task agenda for the next seven days, including overdue items. The app must be running.",
            "inputSchema": {"type": "object", "properties": {}}
        },
        {
            "name": "get_backlinks",
            "description": "Read the linked references and plain mentions for a page. The app must be running.",
            "inputSchema": {
                "type": "object",
                "properties": {"page": {"type": "string"}},
                "required": ["page"]
            }
        }
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpStream;

    fn read_full_request(stream: &mut TcpStream) -> String {
        let mut raw = Vec::new();
        let mut chunk = [0u8; 1024];
        let header_end = loop {
            let read = stream.read(&mut chunk).unwrap();
            assert!(read > 0, "the client closed before its headers");
            raw.extend_from_slice(&chunk[..read]);
            if let Some(at) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
                break at;
            }
        };
        let head = String::from_utf8_lossy(&raw[..header_end]).to_string();
        let length = head
            .lines()
            .find_map(|line| line.strip_prefix("Content-Length: "))
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(0);
        while raw.len() < header_end + 4 + length {
            let read = stream.read(&mut chunk).unwrap();
            assert!(read > 0, "the client closed before its body");
            raw.extend_from_slice(&chunk[..read]);
        }
        String::from_utf8_lossy(&raw).to_string()
    }

    #[test]
    fn every_tool_maps_to_its_bridge_method() {
        let cases = [
            (
                "get_project_context",
                json!({"page_name": "Alpha"}),
                "project_context",
            ),
            ("search_notes", json!({"query": "alpha"}), "search_notes"),
            ("get_page", json!({"name": "Alpha"}), "get_page"),
            ("get_day", json!({"date": "2026-11-22"}), "get_day"),
            ("get_agenda", json!({}), "get_agenda"),
            ("get_backlinks", json!({"page": "Alpha"}), "get_backlinks"),
            (
                "apply_project_consolidation",
                json!({"page": "Alpha", "through": "2026-11-22"}),
                "project_consolidation",
            ),
        ];
        for (tool, arguments, method) in cases {
            let (found, _) = translate(tool, arguments).expect("a mapping");
            assert_eq!(found, method, "{tool}");
        }
        assert!(translate("run_sql", json!({})).is_err());
    }

    #[test]
    fn a_stale_control_file_reports_the_app_as_not_running() {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let dir = std::env::temp_dir().join(format!("lognotes-mcp-stale-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(CONTROL_FILE),
            json!({"port": port, "token": "secret", "pid": 1}).to_string(),
        )
        .unwrap();
        let error = post(&dir, "get_agenda", json!({})).unwrap_err();
        assert!(error.contains("not running"), "{error}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn lists_the_documented_tools() {
        let names: Vec<String> = tools()
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            names,
            [
                "get_project_context",
                "apply_project_consolidation",
                "search_notes",
                "get_page",
                "get_day",
                "get_agenda",
                "get_backlinks"
            ]
        );
    }

    #[test]
    fn initializes_with_the_local_tools_capability() {
        let server = Server {
            data_dir: PathBuf::from("/nonexistent"),
        };
        let response = handle_message(
            &server,
            json!({"jsonrpc":"2.0","id":1,"method":"initialize"}),
        )
        .unwrap();
        assert_eq!(response["result"]["protocolVersion"], PROTOCOL_VERSION);
        assert!(response["result"]["capabilities"]["tools"].is_object());
    }

    #[test]
    fn tools_list_is_a_result() {
        let server = Server {
            data_dir: PathBuf::from("/nonexistent"),
        };
        let response = handle_message(
            &server,
            json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        )
        .unwrap();
        assert!(response["result"]["tools"].as_array().unwrap().len() >= 7);
    }

    #[test]
    fn a_missing_control_file_says_the_app_is_not_running() {
        let dir = std::env::temp_dir().join(format!("lognotes-mcp-missing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let server = Server {
            data_dir: dir.clone(),
        };
        let response = handle_message(
            &server,
            json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_agenda","arguments":{}}}),
        )
        .unwrap();
        assert_eq!(response["result"]["isError"], true);
        assert!(
            response["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("not running")
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_fake_control_server_answers_a_tool() {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let dir = std::env::temp_dir().join(format!("lognotes-mcp-fake-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(CONTROL_FILE),
            json!({"port": port, "token": "secret", "pid": 1}).to_string(),
        )
        .unwrap();

        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let text = read_full_request(&mut stream);
            assert!(text.contains("Authorization: Bearer secret"));
            assert!(text.contains("\"method\":\"get_agenda\""));
            let body = json!({"today":"2026-11-22","rows":[]}).to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).unwrap();
        });

        let answer = post(&dir, "get_agenda", json!({})).unwrap();
        assert!(answer.contains("\"rows\":[]"));
        handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_bridge_error_is_returned_as_the_tool_error() {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let dir = std::env::temp_dir().join(format!("lognotes-mcp-error-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(CONTROL_FILE),
            json!({"port": port, "token": "secret", "pid": 1}).to_string(),
        )
        .unwrap();

        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = read_full_request(&mut stream);
            let body = json!({"error":"editor_busy"}).to_string();
            let response = format!(
                "HTTP/1.1 409 Conflict\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).unwrap();
        });

        assert_eq!(
            post(&dir, "project_consolidation", json!({})).unwrap_err(),
            "editor_busy"
        );
        handle.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }
}

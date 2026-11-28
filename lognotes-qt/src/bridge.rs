//! The local control channel the MCP adapter uses.
//!
//! Loopback only. One `POST /rpc` call per connection, a per-launch bearer token in a user-only file
//! beside the database, and a short timeout. The listener never touches the store itself: it hands the
//! request to the worker and waits for the worker's answer.

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::Duration;

use cxx_qt::CxxQtThread;
use rand::Rng;
use serde_json::{Value, json};

use crate::journal::{Job, qobject::Journal};

/// The file beside the database that publishes the loopback endpoint.
pub(crate) const CONTROL_FILE: &str = "lognotes-agent.json";
/// Request bodies larger than this are refused before parsing.
const MAX_REQUEST: usize = 1_048_576;
/// How long a request waits for the worker before the bridge gives up.
const WORKER_TIMEOUT: Duration = Duration::from_secs(15);

static BUSY: AtomicBool = AtomicBool::new(false);

pub(crate) fn set_busy(busy: bool) {
    BUSY.store(busy, Ordering::Relaxed);
}

pub(crate) fn is_busy() -> bool {
    BUSY.load(Ordering::Relaxed)
}

/// Start the loopback listener and publish its port and token. Returns false when the listener cannot
/// bind or the control file cannot be written; that must not prevent the app from opening.
pub(crate) fn start(database: &Path, worker: Sender<Job>, queue: CxxQtThread<Journal>) -> bool {
    let listener = match TcpListener::bind(("127.0.0.1", 0)) {
        Ok(listener) => listener,
        Err(error) => {
            crate::diagnostics::warn(&format!("agent bridge could not listen: {error}"));
            return false;
        }
    };
    let port = match listener.local_addr() {
        Ok(address) => address.port(),
        Err(error) => {
            crate::diagnostics::warn(&format!("agent bridge has no local address: {error}"));
            return false;
        }
    };
    let token = new_token();
    let control = database.with_file_name(CONTROL_FILE);
    if let Err(error) = write_control(&control, port, &token) {
        crate::diagnostics::warn(&format!(
            "agent bridge could not publish its control file: {error}"
        ));
        return false;
    }

    crate::diagnostics::info(&format!("agent bridge listening on 127.0.0.1:{port}"));

    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else {
                continue;
            };
            let worker = worker.clone();
            let token = token.clone();
            let queue = queue.clone();
            thread::spawn(move || handle_connection(stream, worker, &token, queue));
        }
    });
    true
}

fn new_token() -> String {
    let mut rng = rand::rng();
    (0..32)
        .map(|_| format!("{:02x}", rng.random::<u8>()))
        .collect()
}

fn write_control(path: &Path, port: u16, token: &str) -> std::io::Result<()> {
    let body = json!({
        "port": port,
        "token": token,
        "pid": std::process::id(),
    })
    .to_string();
    fs::write(path, body)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn handle_connection(
    mut stream: TcpStream,
    worker: Sender<Job>,
    token: &str,
    queue: CxxQtThread<Journal>,
) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));

    let (method, path, auth, body) = match read_request(&mut stream) {
        Ok(parts) => parts,
        Err((status, message)) => {
            write_response(&mut stream, status, &json!({"error": message}).to_string());
            return;
        }
    };
    if body.len() > MAX_REQUEST {
        write_response(
            &mut stream,
            413,
            &json!({"error": "request body is too large"}).to_string(),
        );
        return;
    }

    let handler = |method: String, params: Value| -> Result<String, String> {
        let (reply, wait) = mpsc::channel();
        if worker
            .send(Job::Agent {
                method,
                params,
                queue: queue.clone(),
                reply,
            })
            .is_err()
        {
            return Err("the LogNotes worker is not available".to_string());
        }
        match wait.recv_timeout(WORKER_TIMEOUT) {
            Ok(Ok(answer)) => Ok(answer),
            Ok(Err(error)) => Err(error),
            Err(_) => Err("LogNotes did not answer in time".to_string()),
        }
    };

    let (status, body) = process_request(
        &method,
        &path,
        auth.as_deref(),
        token,
        &body,
        is_busy(),
        &handler,
    );
    write_response(&mut stream, status, &body);
}

fn process_request(
    method: &str,
    path: &str,
    auth: Option<&str>,
    token: &str,
    body: &[u8],
    busy: bool,
    handler: &dyn Fn(String, Value) -> Result<String, String>,
) -> (u16, String) {
    let offered = auth.and_then(|value| value.strip_prefix("Bearer "));
    if !offered.is_some_and(|offered| constant_time_eq(offered.as_bytes(), token.as_bytes())) {
        return (
            401,
            json!({"error": "missing or stale credential"}).to_string(),
        );
    }

    if method == "GET" && path == "/health" {
        return (
            200,
            json!({
                "app": "LogNotes",
                "version": env!("CARGO_PKG_VERSION"),
                "busy": busy,
            })
            .to_string(),
        );
    }
    if method != "POST" || path != "/rpc" {
        return (404, json!({"error": "not found"}).to_string());
    }
    if body.len() > MAX_REQUEST {
        return (
            413,
            json!({"error": "request body is too large"}).to_string(),
        );
    }

    let request: Value = match serde_json::from_slice(body) {
        Ok(request) => request,
        Err(_) => return (400, json!({"error": "invalid JSON"}).to_string()),
    };
    let Some(call) = request.get("method").and_then(Value::as_str) else {
        return (400, json!({"error": "method is required"}).to_string());
    };
    let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
    if call == "project_consolidation" && busy {
        return (
            409,
            json!({"error": "editor_busy", "retry": true}).to_string(),
        );
    }
    match handler(call.to_string(), params) {
        Ok(answer) => (200, answer),
        Err(error) => (400, json!({"error": error}).to_string()),
    }
}

fn read_request(
    stream: &mut TcpStream,
) -> Result<(String, String, Option<String>, Vec<u8>), (u16, String)> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 1024];
    let header_end = loop {
        let read = stream
            .read(&mut chunk)
            .map_err(|_| (400, "could not read the request".to_string()))?;
        if read == 0 {
            return Err((400, "the request ended before its headers".to_string()));
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(at) = find_headers_end(&buffer) {
            break at;
        }
        if buffer.len() > 16 * 1024 {
            return Err((431, "request headers are too large".to_string()));
        }
    };

    let header_text = String::from_utf8_lossy(&buffer[..header_end]);
    let mut lines = header_text.lines();
    let request_line = lines
        .next()
        .ok_or_else(|| (400, "missing request line".to_string()))?;
    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| (400, "missing request method".to_string()))?
        .to_string();
    let path = parts
        .next()
        .ok_or_else(|| (400, "missing request path".to_string()))?
        .to_string();

    let mut auth = None;
    let mut content_length = 0usize;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim();
        if name == "authorization" {
            auth = Some(value.to_string());
        } else if name == "content-length" {
            content_length = value
                .parse()
                .map_err(|_| (400, "content-length is not a number".to_string()))?;
        }
    }
    if content_length > MAX_REQUEST {
        return Err((413, "request body is too large".to_string()));
    }

    let mut body = buffer[header_end + 4..].to_vec();
    while body.len() < content_length {
        let read = stream
            .read(&mut chunk)
            .map_err(|_| (400, "could not read the request body".to_string()))?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read]);
    }
    body.truncate(content_length);
    Ok((method, path, auth, body))
}

fn find_headers_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

fn write_response(stream: &mut TcpStream, status: u16, body: &str) {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        409 => "Conflict",
        413 => "Payload Too Large",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "Error",
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0u8;
    for (a, b) in left.iter().zip(right) {
        difference |= a ^ b;
    }
    difference == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(_method: String, params: Value) -> Result<String, String> {
        Ok(json!({"got": params}).to_string())
    }

    #[test]
    fn a_missing_or_stale_token_is_refused() {
        let (status, _) = process_request("GET", "/health", None, "secret", b"", false, &answer);
        assert_eq!(status, 401);
        let (status, _) = process_request(
            "GET",
            "/health",
            Some("Bearer wrong"),
            "secret",
            b"",
            false,
            &answer,
        );
        assert_eq!(status, 401);
    }

    #[test]
    fn health_reports_the_app_and_busy_state() {
        let (status, body) = process_request(
            "GET",
            "/health",
            Some("Bearer secret"),
            "secret",
            b"",
            true,
            &answer,
        );
        assert_eq!(status, 200);
        assert!(body.contains("LogNotes"));
        assert!(body.contains("\"busy\":true"));
    }

    #[test]
    fn an_unknown_path_is_not_found() {
        let (status, _) = process_request(
            "GET",
            "/nope",
            Some("Bearer secret"),
            "secret",
            b"",
            false,
            &answer,
        );
        assert_eq!(status, 404);
    }

    #[test]
    fn a_bad_payload_is_refused() {
        let (status, _) = process_request(
            "POST",
            "/rpc",
            Some("Bearer secret"),
            "secret",
            b"{not json",
            false,
            &answer,
        );
        assert_eq!(status, 400);
    }

    #[test]
    fn an_oversized_body_is_refused() {
        let body = vec![b'x'; MAX_REQUEST + 1];
        let (status, _) = process_request(
            "POST",
            "/rpc",
            Some("Bearer secret"),
            "secret",
            &body,
            false,
            &answer,
        );
        assert_eq!(status, 413);
    }

    #[test]
    fn a_consolidation_is_refused_while_the_editor_is_busy() {
        let body = br#"{"method":"project_consolidation","params":{}}"#;
        let (status, body) = process_request(
            "POST",
            "/rpc",
            Some("Bearer secret"),
            "secret",
            body,
            true,
            &answer,
        );
        assert_eq!(status, 409);
        assert!(body.contains("editor_busy"));
    }

    #[test]
    fn a_valid_rpc_reaches_the_handler() {
        let body = br#"{"method":"search_notes","params":{"query":"alpha"}}"#;
        let (status, body) = process_request(
            "POST",
            "/rpc",
            Some("Bearer secret"),
            "secret",
            body,
            false,
            &answer,
        );
        assert_eq!(status, 200);
        assert!(body.contains("alpha"));
    }

    #[test]
    fn an_unknown_method_is_returned_as_a_handler_failure() {
        let body = br#"{"method":"run_sql","params":{}}"#;
        let (status, _) = process_request(
            "POST",
            "/rpc",
            Some("Bearer secret"),
            "secret",
            body,
            false,
            &|_, _| Err("unknown bridge method".to_string()),
        );
        assert_eq!(status, 400);
    }

    #[test]
    fn the_control_file_is_user_only() {
        let path = std::env::temp_dir().join(format!(
            "lognotes-agent-{}-{}.json",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        write_control(&path, 1234, "token").unwrap();
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("\"port\":1234"));
        assert!(body.contains("\"token\":\"token\""));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn the_handler_type_does_not_hide_a_worker_failure() {
        let body = br#"{"method":"project_context","params":{}}"#;
        let (status, body) = process_request(
            "POST",
            "/rpc",
            Some("Bearer secret"),
            "secret",
            body,
            false,
            &|_, _| Err("worker stopped".to_string()),
        );
        assert_eq!(status, 400);
        assert!(body.contains("worker stopped"));
    }
}

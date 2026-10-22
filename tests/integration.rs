//! Integration tests against an in-process mock OneBot HTTP server.
//!
//! Write operations are verified against a simulated service. The
//! mock records every request and replies with canned envelopes, so the tests
//! cover discovery, ordinary calls, failures, unknown fields, stream framing,
//! and the upload/download adapters without touching a live instance.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use flat_snowluma_mcp::catalog::Catalog;
use flat_snowluma_mcp::config::Config;
use flat_snowluma_mcp::onebot::OneBotClient;
use flat_snowluma_mcp::server::SnowLumaServer;
use flat_snowluma_mcp::tools::App;
use rmcp::ServerHandler;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

/// A canned reply: either a single body or a sequence of stream frames.
#[derive(Clone)]
enum Reply {
    Json(Value),
    Frames(Vec<Value>),
    Raw(String),
    DelayedFrames {
        frames: Vec<Value>,
        interval: std::time::Duration,
    },
    UploadEcho,
    UploadDelayed,
    UploadMutate(std::path::PathBuf),
    /// Choose a reply from the request's `params` (for the two upload calls).
    OnComplete {
        chunk_ack: Value,
        complete: Value,
    },
}

/// Records requests and serves canned replies keyed by action name.
struct Mock {
    replies: BTreeMap<String, Reply>,
    seen: Mutex<Vec<Value>>,
    addr: SocketAddr,
}

impl Mock {
    async fn start(replies: BTreeMap<String, Reply>) -> Arc<Mock> {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let mock = Arc::new(Mock {
            replies,
            seen: Mutex::new(Vec::new()),
            addr,
        });
        let server = mock.clone();
        tokio::spawn(async move {
            loop {
                let Ok((socket, _)) = listener.accept().await else {
                    break;
                };
                let mock = server.clone();
                tokio::spawn(async move {
                    let _ = serve_one(socket, mock).await;
                });
            }
        });
        mock
    }

    fn endpoint(&self) -> String {
        format!("http://{}/", self.addr)
    }

    fn requests(&self) -> Vec<Value> {
        self.seen.lock().unwrap().clone()
    }
}

async fn serve_one(mut socket: tokio::net::TcpStream, mock: Arc<Mock>) -> std::io::Result<()> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    // Read headers then the body (Content-Length).
    let (header_end, content_length) = loop {
        let read = socket.read(&mut chunk).await?;
        if read == 0 {
            return Ok(());
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(position) = find_headers_end(&buffer) {
            let headers = String::from_utf8_lossy(&buffer[..position]).to_lowercase();
            let length = headers
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            break (position + 4, length);
        }
    };
    while buffer.len() < header_end + content_length {
        let read = socket.read(&mut chunk).await?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
    let body = &buffer[header_end..(header_end + content_length).min(buffer.len())];
    let request: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let action = request
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    mock.seen.lock().unwrap().push(request.clone());

    let reply = mock.replies.get(&action).cloned().unwrap_or_else(|| {
        Reply::Json(json!({
            "status": "failed",
            "retcode": 1404,
            "data": null,
            "wording": "unknown action",
            "echo": request.get("echo").cloned().unwrap_or(Value::Null),
        }))
    });

    let echo = request.get("echo").cloned().unwrap_or(Value::Null);
    match reply {
        Reply::Json(mut value) => {
            if let Some(object) = value.as_object_mut() {
                object.insert("echo".to_string(), echo);
            }
            // The live instance answers ordinary (non-stream) actions with a
            // single plain JSON object and no CRLFCRLF delimiter.
            let text = serde_json::to_string(&value).unwrap();
            write_response(&mut socket, &text).await?;
        }
        Reply::Frames(frames) => {
            let mut body = String::new();
            for mut frame in frames {
                decorate_stream(&mut frame, echo.clone());
                body.push_str(&serde_json::to_string(&frame).unwrap());
                body.push_str("\r\n\r\n");
            }
            write_response(&mut socket, &body).await?;
        }
        Reply::Raw(body) => write_response(&mut socket, &body).await?,
        Reply::DelayedFrames { frames, interval } => {
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n").await?;
            for mut frame in frames {
                decorate_stream(&mut frame, echo.clone());
                let body = format!("{}\r\n\r\n", frame);
                socket
                    .write_all(format!("{:x}\r\n{}\r\n", body.len(), body).as_bytes())
                    .await?;
                socket.flush().await?;
                tokio::time::sleep(interval).await;
            }
            socket.write_all(b"0\r\n\r\n").await?;
        }
        reply @ (Reply::UploadEcho | Reply::UploadDelayed | Reply::UploadMutate(_)) => {
            let params = &request["params"];
            if params["reset"] != true && params["is_complete"] != true {
                match reply {
                    Reply::UploadDelayed => {
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await
                    }
                    Reply::UploadMutate(path) => std::fs::write(path, b"changed").unwrap(),
                    _ => (),
                }
            }
            let data = if params["reset"] == true {
                json!({"type":"response", "status":"reset"})
            } else if params["is_complete"] == true {
                json!({"type":"response", "status":"file_complete", "file_path":"/tmp/onebot-stream/upload/report.bin", "file_size":params["file_size"], "sha256":params["expected_sha256"]})
            } else {
                json!({"type":"stream", "status":"chunk_received"})
            };
            let mut frame = json!({"status":"ok", "retcode":0, "data":data});
            decorate_stream(&mut frame, echo);
            write_response(&mut socket, &format!("{}\r\n\r\n", frame)).await?;
        }
        Reply::OnComplete {
            chunk_ack,
            complete,
        } => {
            let is_complete = request
                .get("params")
                .and_then(|params| params.get("is_complete"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let mut frame = if is_complete { complete } else { chunk_ack };
            decorate_stream(&mut frame, echo);
            let mut body = serde_json::to_string(&frame).unwrap();
            body.push_str("\r\n\r\n");
            write_response(&mut socket, &body).await?;
        }
    }
    Ok(())
}

/// Add the envelope fields the live instance attaches to every stream frame.
///
/// Verified against the running instance: a stream frame carries `echo`,
/// `stream: "stream-action"`, and (usually empty) `message`/`wording`.
fn decorate_stream(frame: &mut Value, echo: Value) {
    if let Some(object) = frame.as_object_mut() {
        object.insert("echo".to_string(), echo);
        object.insert("stream".to_string(), json!("stream-action"));
        object.entry("message".to_string()).or_insert(json!(""));
        object.entry("wording".to_string()).or_insert(json!(""));
    }
}

fn find_headers_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

async fn write_response(socket: &mut tokio::net::TcpStream, body: &str) -> std::io::Result<()> {
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    socket.write_all(response.as_bytes()).await?;
    socket.flush().await
}

fn app_with(mock: &Mock, stream_dir: std::path::PathBuf) -> App {
    let catalog = Catalog::load().unwrap();
    let client =
        OneBotClient::new(mock.endpoint(), None, std::time::Duration::from_secs(5)).unwrap();
    let config = Config {
        endpoint: Some(mock.endpoint()),
        token: None,
        timeout: std::time::Duration::from_secs(5),
        stream_dir,
        upload_root: None,
        max_stream_bytes: 1024 * 1024,
    };
    App::new(catalog, Some(client), config).unwrap()
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("fsmcp-it-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

async fn call(app: &App, name: &str, args: Value) -> rmcp::model::CallToolResult {
    app.call(name, args.as_object().cloned(), CancellationToken::new())
        .await
}

#[tokio::test]
async fn ordinary_success_keeps_onebot_envelope() {
    let mut replies = BTreeMap::new();
    replies.insert(
        "get_group_msg_history".to_string(),
        Reply::Json(json!({
            "status": "ok",
            "retcode": 0,
            "data": { "messages": [ { "message_id": 1 } ] }
        })),
    );
    let mock = Mock::start(replies).await;
    let app = app_with(&mock, temp_dir("ordinary"));

    let result = call(
        &app,
        "get_group_msg_history",
        json!({ "group_id": 123456, "count": 1 }),
    )
    .await;
    assert_eq!(result.is_error, Some(false));
    let structured = result.structured_content.unwrap();
    assert_eq!(structured["status"], json!("ok"));
    assert_eq!(structured["retcode"], json!(0));
    assert_eq!(structured["data"]["messages"][0]["message_id"], json!(1));

    // The HTTP request matches the official MCP: action + params.
    let seen = mock.requests();
    assert_eq!(seen[0]["action"], json!("get_group_msg_history"));
    assert_eq!(seen[0]["params"]["count"], json!(1));
    assert!(seen[0].get("echo").is_none());
}

#[tokio::test]
async fn logical_failure_preserves_the_official_onebot_envelope() {
    let mut replies = BTreeMap::new();
    replies.insert(
        "get_msg".to_string(),
        Reply::Json(json!({
            "status": "failed",
            "retcode": 1400,
            "data": null,
            "wording": "message_id: is required"
        })),
    );
    let mock = Mock::start(replies).await;
    let app = app_with(&mock, temp_dir("failure"));

    let result = call(&app, "get_msg", json!({ "message_id": 5 })).await;
    assert_eq!(result.is_error, Some(false));
    assert_eq!(
        result.structured_content.as_ref().unwrap()["retcode"],
        json!(1400)
    );
    let text = result.content[0].as_text().unwrap().text.clone();
    assert!(text.contains("message_id: is required"), "{text}");
}

#[tokio::test]
async fn unknown_field_is_rejected_without_a_request() {
    let mock = Mock::start(BTreeMap::new()).await;
    let app = app_with(&mock, temp_dir("unknown"));

    let result = call(
        &app,
        "get_group_msg_history",
        json!({ "group_id": 1, "message_count": 3 }),
    )
    .await;
    assert_eq!(result.is_error, Some(true));
    assert!(mock.requests().is_empty(), "no request should be sent");
}

#[tokio::test]
async fn file_download_commits_and_reports_metadata() {
    // "hello" split into two chunks.
    let mut replies = BTreeMap::new();
    replies.insert(
        "download_file_stream".to_string(),
        Reply::Frames(vec![
            // `chunk_size` is what the live instance advertises (65536) even
            // though it sends much smaller chunks; the reader must count frames,
            // not derive the count from this hint.
            json!({ "status": "ok", "retcode": 0, "data": {
                "type": "stream", "data_type": "file_info",
                "file_name": "greeting.txt", "file_size": 5, "chunk_size": 65536 } }),
            json!({ "status": "ok", "retcode": 0, "data": {
                "type": "stream", "data_type": "file_chunk", "index": 0, "data": "aGVs",
                "size": 3, "progress": 60, "base64_size": 4 } }),
            json!({ "status": "ok", "retcode": 0, "data": {
                "type": "stream", "data_type": "file_chunk", "index": 1, "data": "bG8=",
                "size": 2, "progress": 100, "base64_size": 4 } }),
            json!({ "status": "ok", "retcode": 0, "data": {
                "type": "response", "data_type": "file_complete",
                "file_name": "greeting.txt", "total_bytes": 5, "total_chunks": 2,
                "message": "Download completed" } }),
        ]),
    );
    let mock = Mock::start(replies).await;
    let dir = temp_dir("download");
    let app = app_with(&mock, dir.clone());

    let result = call(&app, "download_file_stream", json!({ "file": "cached-1" })).await;
    assert_eq!(result.is_error, Some(false), "{:?}", result.content);
    let structured = result.structured_content.unwrap();
    assert!(structured.get("kind").is_none());
    assert_eq!(
        structured["terminal"]["data"]["data_type"],
        json!("file_complete")
    );
    assert_eq!(structured["file_size"], json!(5));
    assert_eq!(
        structured["sha256"],
        json!("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824")
    );
    let path = structured["file_path"].as_str().unwrap();
    assert_eq!(std::fs::read(path).unwrap(), b"hello");
    // No leftover .part file.
    let parts: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "part"))
        .collect();
    assert!(parts.is_empty());
}

#[tokio::test]
async fn generic_stream_keeps_all_frames_and_terminal() {
    let mut replies = BTreeMap::new();
    replies.insert(
        "test_download_stream".to_string(),
        Reply::Frames(vec![
            json!({ "status": "ok", "retcode": 0, "data": {
                "type": "stream", "data_type": "data_chunk", "data": "Index-> 1" } }),
            json!({ "status": "ok", "retcode": 0, "data": {
                "type": "stream", "data_type": "data_chunk", "data": "Index-> 2" } }),
            json!({ "status": "ok", "retcode": 0, "data": {
                "type": "response", "data_type": "data_complete", "data": "done" } }),
        ]),
    );
    let mock = Mock::start(replies).await;
    let app = app_with(&mock, temp_dir("generic"));

    let result = call(&app, "test_download_stream", json!({})).await;
    assert_eq!(result.is_error, Some(false), "{:?}", result.content);
    let structured = result.structured_content.unwrap();
    assert!(structured.get("kind").is_none());
    assert_eq!(structured["frame_count"], json!(3));
    assert_eq!(structured["frames"].as_array().unwrap().len(), 2);
    assert_eq!(
        structured["terminal"]["data"]["data_type"],
        json!("data_complete")
    );
}

#[tokio::test]
async fn truncated_file_download_is_an_error_and_leaves_no_part_file() {
    let mut replies = BTreeMap::new();
    replies.insert(
        "download_file_stream".to_string(),
        Reply::Frames(vec![
            json!({ "status": "ok", "retcode": 0, "data": {
                "type": "stream", "data_type": "file_info",
                "file_name": "x.bin", "file_size": 10, "chunk_size": 65536 } }),
            json!({ "status": "ok", "retcode": 0, "data": {
                "type": "stream", "data_type": "file_chunk", "index": 0, "data": "aGVs" } }),
        ]),
    );
    let mock = Mock::start(replies).await;
    let dir = temp_dir("truncated");
    let app = app_with(&mock, dir.clone());

    let result = call(&app, "download_file_stream", json!({ "file": "cached-2" })).await;
    assert_eq!(result.is_error, Some(true));
    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    assert!(leftovers.is_empty(), "partial files must be removed");
}

#[tokio::test]
async fn protocol_error_mid_download_removes_the_part_file() {
    // The first chunk is valid, the second is not valid base64, which is a
    // protocol violation after the writer exists; the writer must not leave a
    // .part file behind.
    let mut replies = BTreeMap::new();
    replies.insert(
        "download_file_stream".to_string(),
        Reply::Frames(vec![
            json!({ "status": "ok", "retcode": 0, "data": {
                "type": "stream", "data_type": "file_info",
                "file_name": "y.bin", "file_size": 100, "chunk_size": 65536 } }),
            json!({ "status": "ok", "retcode": 0, "data": {
                "type": "stream", "data_type": "file_chunk", "index": 0, "data": "aGVs" } }),
            json!({ "status": "ok", "retcode": 0, "data": {
                "type": "stream", "data_type": "file_chunk", "index": 1, "data": "not base64!" } }),
        ]),
    );
    let mock = Mock::start(replies).await;
    let dir = temp_dir("protocol-error");
    let app = app_with(&mock, dir.clone());

    let result = call(&app, "download_file_stream", json!({ "file": "cached-3" })).await;
    assert_eq!(result.is_error, Some(true));
    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    assert!(leftovers.is_empty(), "partial files must be removed");
}

#[tokio::test]
async fn upload_execution_chunks_and_verifies_sha256() {
    let mut replies = BTreeMap::new();
    replies.insert(
        "upload_file_stream".to_string(),
        Reply::OnComplete {
            chunk_ack: json!({
                "status": "ok",
                "retcode": 0,
                "data": {
                    "type": "stream",
                    "stream_id": "x",
                    "status": "chunk_received",
                    "received_chunks": 1,
                    "total_chunks": 1
                }
            }),
            complete: json!({
                "status": "ok",
                "retcode": 0,
                "data": {
                    "type": "response",
                    "stream_id": "x",
                    "status": "file_complete",
                    "received_chunks": 1,
                    "total_chunks": 1,
                    "file_path": "/tmp/onebot-stream/upload/1/x__upload.bin",
                    "file_name": "upload.bin",
                    "file_size": 5,
                    "sha256": "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
                }
            }),
        },
    );
    let mock = Mock::start(replies).await;

    let root = temp_dir("upload-root");
    let file = root.join("upload.bin");
    std::fs::write(&file, b"hello").unwrap();

    let catalog = Catalog::load().unwrap();
    let client =
        OneBotClient::new(mock.endpoint(), None, std::time::Duration::from_secs(5)).unwrap();
    let config = Config {
        endpoint: Some(mock.endpoint()),
        token: None,
        timeout: std::time::Duration::from_secs(5),
        stream_dir: temp_dir("upload-downloads"),
        upload_root: Some(root.canonicalize().unwrap()),
        max_stream_bytes: 1024 * 1024,
    };
    let app = App::new(catalog, Some(client), config).unwrap();

    let result = call(
        &app,
        "upload_file_stream",
        json!({ "execution": { "input_file": file.to_str().unwrap() } }),
    )
    .await;
    assert_eq!(result.is_error, Some(false), "{:?}", result.content);
    let structured = result.structured_content.unwrap();
    assert!(structured.get("kind").is_none());
    assert_eq!(
        structured["terminal"]["data"]["status"],
        json!("file_complete")
    );
    assert_eq!(structured["file_size"], json!(5));
    assert_eq!(
        structured["sha256"],
        json!("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824")
    );

    // Two requests: the chunk then the completion, both carrying MCP-owned fields.
    let seen = mock.requests();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0]["params"]["stream_id"].as_str().unwrap().len(), 36);
    assert_eq!(seen[0]["params"]["chunk_data"], json!("aGVsbG8="));
    assert_eq!(seen[0]["params"]["total_chunks"], json!(1));
    assert_eq!(seen[1]["params"]["is_complete"], json!(true));
    assert_eq!(
        seen[1]["params"]["expected_sha256"],
        json!("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824")
    );
    // The same stream id is used for both calls.
    assert_eq!(
        seen[0]["params"]["stream_id"],
        seen[1]["params"]["stream_id"]
    );
    assert!(structured.get("file_retention_ms").is_none());
    assert!(
        seen.iter()
            .all(|request| request["params"].get("execution").is_none())
    );
}

#[tokio::test]
async fn upload_rejects_paths_outside_the_root() {
    let mock = Mock::start(BTreeMap::new()).await;
    let root = temp_dir("upload-escape-root");
    let catalog = Catalog::load().unwrap();
    let client =
        OneBotClient::new(mock.endpoint(), None, std::time::Duration::from_secs(5)).unwrap();
    let config = Config {
        endpoint: Some(mock.endpoint()),
        token: None,
        timeout: std::time::Duration::from_secs(5),
        stream_dir: temp_dir("upload-escape-downloads"),
        upload_root: Some(root.canonicalize().unwrap()),
        max_stream_bytes: 1024 * 1024,
    };
    let app = App::new(catalog, Some(client), config).unwrap();

    let result = call(
        &app,
        "upload_file_stream",
        json!({ "execution": { "input_file": "/etc/hostname" } }),
    )
    .await;
    assert_eq!(result.is_error, Some(true));
    assert!(mock.requests().is_empty());
}

#[tokio::test]
async fn large_generic_stream_reports_the_official_limit() {
    // Exceed the official frame bound (256): fail instead of inventing a spill format.
    let mut frames: Vec<Value> = Vec::new();
    for index in 0..300 {
        frames.push(json!({ "status": "ok", "retcode": 0, "data": {
            "type": "stream", "data_type": "data_chunk", "data": format!("Index-> {index}") } }));
    }
    frames.push(json!({ "status": "ok", "retcode": 0, "data": {
        "type": "response", "data_type": "data_complete", "data": "done" } }));

    let mut replies = BTreeMap::new();
    replies.insert("test_download_stream".to_string(), Reply::Frames(frames));
    let mock = Mock::start(replies).await;
    let dir = temp_dir("spill");
    let app = app_with(&mock, dir.clone());

    let result = call(&app, "test_download_stream", json!({})).await;
    assert_eq!(result.is_error, Some(true));
    assert!(
        result.content[0]
            .as_text()
            .unwrap()
            .text
            .contains("generic frame summary exceeds limit")
    );
    assert_eq!(std::fs::read_dir(dir).unwrap().count(), 0);
}

#[tokio::test]
async fn generic_stream_error_frame_is_reported() {
    // The live instance's `test_download_stream {error:true}` ends with a
    // `status: "failed"` frame carrying `data.type: "error"`.
    let mut replies = BTreeMap::new();
    replies.insert(
        "test_download_stream".to_string(),
        Reply::Frames(vec![
            json!({ "status": "ok", "retcode": 0, "data": {
                "type": "stream", "data_type": "data_chunk", "data": "Index-> 1" } }),
            json!({ "status": "failed", "retcode": 100, "data": {
                "type": "error", "data_type": "error" },
                "wording": "This is a test error", "message": "This is a test error" }),
        ]),
    );
    let mock = Mock::start(replies).await;
    let app = app_with(&mock, temp_dir("generic-error"));

    let result = call(&app, "test_download_stream", json!({})).await;
    assert_eq!(result.is_error, Some(true));
    let text = result.content[0].as_text().unwrap().text.clone();
    assert!(text.contains("This is a test error"), "{text}");
}

#[tokio::test]
async fn upload_stream_error_frame_is_reported() {
    // A failed upload ack frame (the live shape) must surface as an error and
    // must not be mistaken for a successful chunk acknowledgement.
    let mut replies = BTreeMap::new();
    replies.insert(
        "upload_file_stream".to_string(),
        Reply::Frames(vec![json!({
            "status": "failed",
            "retcode": 100,
            "data": { "type": "error", "data_type": "error" },
            "wording": "incomplete stream: missing 1 chunk(s) [0]",
            "message": "incomplete stream: missing 1 chunk(s) [0]"
        })]),
    );
    let mock = Mock::start(replies).await;
    let root = temp_dir("upload-error-root");
    let file = root.join("upload.bin");
    std::fs::write(&file, b"hello").unwrap();
    let catalog = Catalog::load().unwrap();
    let client =
        OneBotClient::new(mock.endpoint(), None, std::time::Duration::from_secs(5)).unwrap();
    let config = Config {
        endpoint: Some(mock.endpoint()),
        token: None,
        timeout: std::time::Duration::from_secs(5),
        stream_dir: temp_dir("upload-error-downloads"),
        upload_root: Some(root.canonicalize().unwrap()),
        max_stream_bytes: 1024 * 1024,
    };
    let app = App::new(catalog, Some(client), config).unwrap();

    let result = call(
        &app,
        "upload_file_stream",
        json!({ "execution": { "input_file": file.to_str().unwrap() } }),
    )
    .await;
    assert_eq!(result.is_error, Some(true));
    let text = result.content[0].as_text().unwrap().text.clone();
    assert!(text.contains("incomplete stream"), "{text}");
}

#[tokio::test]
async fn server_exposes_every_action_and_the_resource() {
    let mock = Mock::start(BTreeMap::new()).await;
    let catalog = Catalog::load().unwrap();
    let client =
        OneBotClient::new(mock.endpoint(), None, std::time::Duration::from_secs(5)).unwrap();
    let config = Config {
        endpoint: Some(mock.endpoint()),
        token: None,
        timeout: std::time::Duration::from_secs(5),
        stream_dir: temp_dir("server"),
        upload_root: None,
        max_stream_bytes: 1024 * 1024,
    };
    let app = App::new(catalog, Some(client), config).unwrap();
    let server = SnowLumaServer::new(app, catalog);

    let info = server.get_info();
    assert!(info.capabilities.tools.is_some());
    assert!(info.capabilities.resources.is_some());
    assert!(info.instructions.unwrap().contains("SnowLuma QQ API"));

    let tools = flat_snowluma_mcp::server::declared_tools(&server);
    assert_eq!(tools.len(), catalog.len());
}

fn download_frames() -> Vec<Value> {
    vec![
        json!({"status":"ok", "retcode":0, "data":{"type":"stream", "data_type":"file_info", "file_name":"hello.bin", "file_size":5, "chunk_size":65536}}),
        json!({"status":"ok", "retcode":0, "data":{"type":"stream", "data_type":"file_chunk", "index":0, "data":"aGVsbG8=", "size":5}}),
        json!({"status":"ok", "retcode":0, "data":{"type":"response", "data_type":"file_complete", "total_bytes":5, "total_chunks":1}}),
    ]
}

fn configured_app(
    mock: &Mock,
    dir: std::path::PathBuf,
    root: Option<std::path::PathBuf>,
    timeout: std::time::Duration,
    max: u64,
) -> App {
    let client = OneBotClient::new(mock.endpoint(), None, timeout).unwrap();
    App::new(
        Catalog::load().unwrap(),
        Some(client),
        Config {
            endpoint: Some(mock.endpoint()),
            token: None,
            timeout,
            stream_dir: dir,
            upload_root: root,
            max_stream_bytes: max,
        },
    )
    .unwrap()
}

#[tokio::test]
async fn malformed_downloads_are_rejected_before_commit() {
    let mut cases = Vec::new();
    for (field, value) in [
        ("file_size", json!("5")),
        ("file_size", Value::Null),
        ("chunk_size", json!(0)),
    ] {
        let mut frames = download_frames();
        frames[0]["data"][field] = value;
        cases.push(frames);
    }
    for (field, value) in [
        ("index", json!(1)),
        ("index", Value::Null),
        ("size", json!(4)),
        ("data", json!("a===")),
    ] {
        let mut frames = download_frames();
        frames[1]["data"][field] = value;
        cases.push(frames);
    }
    for (field, value) in [
        ("total_bytes", Value::Null),
        ("total_bytes", json!("5")),
        ("total_chunks", json!(2)),
        ("type", json!("stream")),
    ] {
        let mut frames = download_frames();
        frames[2]["data"][field] = value;
        cases.push(frames);
    }
    let mut failed = download_frames();
    failed[2]["status"] = json!("failed");
    failed[2]["retcode"] = json!(100);
    cases.push(failed);
    let mut trailing = download_frames();
    trailing.push(json!({"status":"ok", "retcode":0, "data":{"type":"response"}}));
    cases.push(trailing);
    cases.push(download_frames()[1..].to_vec());
    let mut missing = download_frames();
    missing[2].as_object_mut().unwrap().remove("retcode");
    cases.push(missing);
    for (index, frames) in cases.into_iter().enumerate() {
        let mock = Mock::start(BTreeMap::from([(
            "download_file_stream".into(),
            Reply::Frames(frames),
        )]))
        .await;
        let dir = temp_dir(&format!("malformed-{index}"));
        let app = app_with(&mock, dir.clone());
        let result = call(&app, "download_file_stream", json!({"file":"example"})).await;
        assert_eq!(
            result.is_error,
            Some(true),
            "case {index}: {:?}",
            result.content
        );
        assert_eq!(
            std::fs::read_dir(dir).unwrap().count(),
            0,
            "case {index} left a file"
        );
    }
}

#[tokio::test]
async fn completed_file_with_truncated_trailing_frame_is_not_committed() {
    let mut body = download_frames()
        .iter()
        .map(|frame| format!("{frame}\r\n\r\n"))
        .collect::<String>();
    body.push_str("{\"unfinished\":");
    let mock = Mock::start(BTreeMap::from([(
        "download_file_stream".into(),
        Reply::Raw(body),
    )]))
    .await;
    let dir = temp_dir("trailing-truncated");
    let result = call(
        &app_with(&mock, dir.clone()),
        "download_file_stream",
        json!({"file":"example"}),
    )
    .await;
    assert_eq!(result.is_error, Some(true));
    assert_eq!(std::fs::read_dir(dir).unwrap().count(), 0);
}

#[tokio::test]
async fn directory_quota_counts_committed_files_and_recovers_after_cleanup() {
    let mock = Mock::start(BTreeMap::from([(
        "download_file_stream".into(),
        Reply::Frames(download_frames()),
    )]))
    .await;
    let dir = temp_dir("quota");
    let app = configured_app(
        &mock,
        dir.clone(),
        None,
        std::time::Duration::from_secs(5),
        5,
    );
    for _ in 0..2 {
        assert_eq!(
            call(&app, "download_file_stream", json!({"file":"example"}))
                .await
                .is_error,
            Some(false)
        );
    }
    let other = configured_app(
        &mock,
        dir.clone(),
        None,
        std::time::Duration::from_secs(5),
        5,
    );
    let full = call(&other, "download_file_stream", json!({"file":"example"})).await;
    assert_eq!(full.is_error, Some(true));
    assert!(full.content[0].as_text().unwrap().text.contains("quota"));
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
    for entry in std::fs::read_dir(&dir).unwrap() {
        std::fs::remove_file(entry.unwrap().path()).unwrap();
    }
    assert_eq!(
        call(&other, "download_file_stream", json!({"file":"example"}))
            .await
            .is_error,
        Some(false)
    );
}

#[tokio::test]
async fn healthy_stream_outlives_the_timeout_when_reads_keep_arriving() {
    let mut frames: Vec<_> = (0..5).map(|i| json!({"status":"ok", "retcode":0, "data":{"type":"stream", "data":"part", "index":i}})).collect();
    frames.push(json!({"status":"ok", "retcode":0, "data":{"type":"response"}}));
    let mock = Mock::start(BTreeMap::from([(
        "test_download_stream".into(),
        Reply::DelayedFrames {
            frames,
            interval: std::time::Duration::from_millis(40),
        },
    )]))
    .await;
    let app = configured_app(
        &mock,
        temp_dir("long-stream"),
        None,
        std::time::Duration::from_millis(150),
        1024,
    );
    let start = std::time::Instant::now();
    let result = call(&app, "test_download_stream", json!({})).await;
    assert_eq!(result.is_error, Some(false), "{:?}", result.content);
    assert!(start.elapsed() > std::time::Duration::from_millis(150));
    assert_eq!(result.structured_content.unwrap()["frame_count"], json!(6));
}

#[tokio::test]
async fn idle_timeout_removes_the_partial_file() {
    let mock = Mock::start(BTreeMap::from([(
        "download_file_stream".into(),
        Reply::DelayedFrames {
            frames: download_frames(),
            interval: std::time::Duration::from_millis(250),
        },
    )]))
    .await;
    let dir = temp_dir("idle-timeout");
    let app = configured_app(
        &mock,
        dir.clone(),
        None,
        std::time::Duration::from_millis(80),
        1024,
    );
    let result = call(&app, "download_file_stream", json!({"file":"example"})).await;
    assert_eq!(result.is_error, Some(true));
    assert!(
        result.content[0]
            .as_text()
            .unwrap()
            .text
            .contains("idle timeout")
    );
    assert_eq!(std::fs::read_dir(dir).unwrap().count(), 0);
}

#[tokio::test]
async fn caller_cancellation_removes_partial_files() {
    let mock = Mock::start(BTreeMap::from([(
        "download_file_stream".into(),
        Reply::DelayedFrames {
            frames: download_frames(),
            interval: std::time::Duration::from_secs(1),
        },
    )]))
    .await;
    let dir = temp_dir("cancel-download");
    let app = app_with(&mock, dir.clone());
    let cancel = CancellationToken::new();
    let signal = cancel.clone();
    let task = tokio::spawn(async move {
        app.call(
            "download_file_stream",
            json!({"file":"example"}).as_object().cloned(),
            signal,
        )
        .await
    });
    for _ in 0..100 {
        if std::fs::read_dir(&dir).unwrap().count() > 0 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    cancel.cancel();
    assert_eq!(task.await.unwrap().is_error, Some(true));
    assert_eq!(std::fs::read_dir(dir).unwrap().count(), 0);
}

#[tokio::test]
async fn stream_admission_has_four_slots_and_releases_them_on_cancellation() {
    let frames = vec![json!({"status":"ok", "retcode":0, "data":{"type":"stream"}})];
    let mock = Mock::start(BTreeMap::from([(
        "test_download_stream".into(),
        Reply::DelayedFrames {
            frames,
            interval: std::time::Duration::from_secs(2),
        },
    )]))
    .await;
    let app = app_with(&mock, temp_dir("admission"));
    let mut tasks = Vec::new();
    let mut cancels = Vec::new();
    for _ in 0..4 {
        let app = app.clone();
        let cancel = CancellationToken::new();
        cancels.push(cancel.clone());
        tasks.push(tokio::spawn(async move {
            app.call("test_download_stream", None, cancel).await
        }));
    }
    for _ in 0..100 {
        if mock.requests().len() == 4 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(mock.requests().len(), 4);
    let fifth = call(&app, "test_download_stream", json!({})).await;
    assert_eq!(fifth.is_error, Some(true));
    assert!(
        fifth.content[0]
            .as_text()
            .unwrap()
            .text
            .contains("concurrent")
    );
    assert_eq!(mock.requests().len(), 4);
    for cancel in cancels {
        cancel.cancel();
    }
    for task in tasks {
        assert_eq!(task.await.unwrap().is_error, Some(true));
    }
    let already_cancelled = CancellationToken::new();
    already_cancelled.cancel();
    let result = app
        .call("test_download_stream", None, already_cancelled)
        .await;
    assert!(result.content[0].as_text().unwrap().text.contains("cancel"));
}

#[tokio::test]
async fn automatic_upload_streams_multiple_chunks_and_empty_files() {
    use base64::Engine as _;
    use sha2::{Digest, Sha256};
    let mock = Mock::start(BTreeMap::from([(
        "upload_file_stream".into(),
        Reply::UploadEcho,
    )]))
    .await;
    for (index, bytes) in [vec![b'x'; 512 * 1024 + 13], Vec::new()]
        .into_iter()
        .enumerate()
    {
        let root = temp_dir(&format!("upload-buffer-{index}"));
        let file = root.join("example.bin");
        std::fs::write(&file, &bytes).unwrap();
        let app = configured_app(
            &mock,
            temp_dir(&format!("upload-buffer-downloads-{index}")),
            Some(root),
            std::time::Duration::from_secs(5),
            2 * 1024 * 1024,
        );
        let start = mock.requests().len();
        let result = call(
            &app,
            "upload_file_stream",
            json!({"filename":"sent.bin", "file_retention":0, "execution":{"input_file":file}}),
        )
        .await;
        assert_eq!(result.is_error, Some(false), "{:?}", result.content);
        let result = result.structured_content.unwrap();
        let chunks = if bytes.is_empty() { 1 } else { 2 };
        assert_eq!(result["chunk_count"], json!(chunks));
        let seen = mock.requests();
        let sent = &seen[start..];
        assert_eq!(sent.len(), chunks + 1);
        let reconstructed: Vec<u8> = sent[..chunks]
            .iter()
            .flat_map(|request| {
                base64::engine::general_purpose::STANDARD
                    .decode(request["params"]["chunk_data"].as_str().unwrap())
                    .unwrap()
            })
            .collect();
        assert_eq!(reconstructed, bytes);
        assert!(
            sent.iter()
                .all(|request| request["params"].get("execution").is_none()
                    && request["params"]["file_retention"] == 0)
        );
        let sha = Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert_eq!(result["sha256"], json!(sha));
        assert_eq!(sent[chunks]["params"]["expected_sha256"], json!(sha));
    }
}

#[tokio::test]
async fn execution_options_are_validated_before_sending_http() {
    let mock = Mock::start(BTreeMap::new()).await;
    let app = app_with(&mock, temp_dir("execution-invalid"));
    for execution in [
        Value::Null,
        json!("path"),
        json!({"unknown":1}),
        json!({"input_file":""}),
        json!({"input_file":42}),
    ] {
        assert_eq!(
            call(&app, "upload_file_stream", json!({"execution":execution}))
                .await
                .is_error,
            Some(true)
        );
    }
    assert_eq!(
        call(
            &app,
            "get_msg",
            json!({"message_id":1, "execution":{"input_file":"file"}})
        )
        .await
        .is_error,
        Some(true)
    );
    for field in flat_snowluma_mcp::stream::UPLOAD_OWNED_FIELDS {
        let mut args = json!({"execution":{"input_file":"file"}});
        args[*field] = Value::Null;
        let result = call(&app, "upload_file_stream", args).await;
        assert_eq!(result.is_error, Some(true));
        assert!(
            result.content[0]
                .as_text()
                .unwrap()
                .text
                .contains("MCP-owned")
        );
    }
    assert!(mock.requests().is_empty());
}

#[tokio::test]
async fn raw_upload_ack_and_reset_remain_available() {
    let ack =
        json!({"status":"ok", "retcode":0, "data":{"type":"stream", "status":"chunk_received"}});
    let mock = Mock::start(BTreeMap::from([(
        "upload_file_stream".into(),
        Reply::Frames(vec![ack]),
    )]))
    .await;
    let app = app_with(&mock, temp_dir("raw-upload"));
    let result = call(
        &app,
        "upload_file_stream",
        json!({"stream_id":"manual", "chunk_data":"aGVsbG8="}),
    )
    .await;
    assert_eq!(result.is_error, Some(false));
    assert_eq!(result.structured_content.unwrap()["frames"], json!([]));
    let reset =
        json!({"status":"failed", "retcode":100, "data":null, "wording":"Stream reset completed"});
    let mock = Mock::start(BTreeMap::from([(
        "upload_file_stream".into(),
        Reply::Frames(vec![reset]),
    )]))
    .await;
    let result = call(
        &app_with(&mock, temp_dir("raw-reset")),
        "upload_file_stream",
        json!({"stream_id":"manual", "reset":true}),
    )
    .await;
    assert_eq!(result.is_error, Some(false));
    assert_eq!(
        result.structured_content.unwrap()["terminal"]["retcode"],
        json!(100)
    );
}

#[cfg(unix)]
#[tokio::test]
async fn upload_symlink_escape_is_rejected_before_http() {
    let mock = Mock::start(BTreeMap::new()).await;
    let root = temp_dir("upload-symlink");
    let link = root.join("outside");
    std::os::unix::fs::symlink("/etc/hostname", &link).unwrap();
    let app = configured_app(
        &mock,
        temp_dir("upload-symlink-downloads"),
        Some(root),
        std::time::Duration::from_secs(5),
        1024,
    );
    assert_eq!(
        call(
            &app,
            "upload_file_stream",
            json!({"execution":{"input_file":link}})
        )
        .await
        .is_error,
        Some(true)
    );
    assert!(mock.requests().is_empty());
}

#[tokio::test]
async fn cancelled_automatic_upload_still_resets_remote_state() {
    let mock = Mock::start(BTreeMap::from([(
        "upload_file_stream".into(),
        Reply::UploadDelayed,
    )]))
    .await;
    let root = temp_dir("cancel-upload-root");
    let file = root.join("data.bin");
    std::fs::write(&file, b"hello").unwrap();
    let app = configured_app(
        &mock,
        temp_dir("cancel-upload-downloads"),
        Some(root),
        std::time::Duration::from_secs(5),
        1024,
    );
    let cancel = CancellationToken::new();
    let signal = cancel.clone();
    let args = json!({"execution":{"input_file":file}})
        .as_object()
        .cloned();
    let task = tokio::spawn(async move { app.call("upload_file_stream", args, signal).await });
    for _ in 0..100 {
        if !mock.requests().is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(mock.requests().len(), 1);
    cancel.cancel();
    let result = tokio::time::timeout(std::time::Duration::from_secs(1), task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.is_error, Some(true));
    let seen = mock.requests();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[1]["params"]["reset"], true);
    assert_eq!(
        seen[0]["params"]["stream_id"],
        seen[1]["params"]["stream_id"]
    );
}

#[tokio::test]
async fn input_changes_during_upload_abort_and_reset_without_committing() {
    let root = temp_dir("mutate-upload-root");
    let file = root.join("data.bin");
    std::fs::write(&file, vec![b'x'; 512 * 1024 + 13]).unwrap();
    let mock = Mock::start(BTreeMap::from([(
        "upload_file_stream".into(),
        Reply::UploadMutate(file.clone()),
    )]))
    .await;
    let app = configured_app(
        &mock,
        temp_dir("mutate-upload-downloads"),
        Some(root),
        std::time::Duration::from_secs(5),
        2 * 1024 * 1024,
    );
    let result = call(
        &app,
        "upload_file_stream",
        json!({"execution":{"input_file":file}}),
    )
    .await;
    assert_eq!(result.is_error, Some(true));
    let seen = mock.requests();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[1]["params"]["reset"], true);
    assert!(
        seen.iter()
            .all(|request| request["params"]["is_complete"] != true)
    );
}

#[tokio::test]
async fn generic_terminal_errors_and_text_limits_match_official_behavior() {
    let terminal = json!({"status":"ok", "retcode":0, "data":{"type":"response"}});
    let cases = [
        vec![terminal.clone(), terminal.clone()],
        vec![json!({"status":"failed", "retcode":100, "data":{"type":"response"}})],
        vec![
            json!({"status":"ok", "retcode":0, "data":{"type":"stream", "data":"x".repeat(256 * 1024)}}),
            terminal,
        ],
    ];
    for (index, frames) in cases.into_iter().enumerate() {
        let mock = Mock::start(BTreeMap::from([(
            "test_download_stream".into(),
            Reply::Frames(frames),
        )]))
        .await;
        let dir = temp_dir(&format!("generic-limits-{index}"));
        assert_eq!(
            call(
                &app_with(&mock, dir.clone()),
                "test_download_stream",
                json!({})
            )
            .await
            .is_error,
            Some(true)
        );
        assert_eq!(std::fs::read_dir(dir).unwrap().count(), 0);
    }
}

#[tokio::test]
async fn decimal_json_integers_have_the_same_meaning_as_official_javascript() {
    let body = download_frames()
        .iter()
        .map(|frame| format!("{frame}\r\n\r\n"))
        .collect::<String>()
        .replace("\"retcode\":0", "\"retcode\":0.0")
        .replace("\"file_size\":5", "\"file_size\":5.0")
        .replace("\"size\":5", "\"size\":5.0")
        .replace("\"index\":0", "\"index\":0.0")
        .replace("\"total_bytes\":5", "\"total_bytes\":5.0")
        .replace("\"total_chunks\":1", "\"total_chunks\":1.0");
    let mock = Mock::start(BTreeMap::from([(
        "download_file_stream".into(),
        Reply::Raw(body),
    )]))
    .await;
    let app = app_with(&mock, temp_dir("decimal-integers"));
    assert_eq!(
        call(&app, "download_file_stream", json!({"file":"example"}))
            .await
            .is_error,
        Some(false)
    );
}

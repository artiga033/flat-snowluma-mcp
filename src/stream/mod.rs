//! MCP-host execution of official Stream Actions: bounded frame summaries,
//! atomic file downloads and streaming automatic uploads.

mod frames;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use base64::Engine as _;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::error::{StreamError, stream};
use crate::onebot::OneBotClient;
use frames::{Frame, FrameReader, safe_u64};

const FILE_DOWNLOAD_ACTIONS: &[&str] = &[
    "download_file_stream",
    "download_file_image_stream",
    "download_file_record_stream",
];
const MAX_GENERIC_FRAMES: usize = 256;
const MAX_GENERIC_TEXT_BYTES: usize = 256 * 1024;
const MAX_FILE_CHUNKS: u64 = 200_000;
const UPLOAD_CHUNK_BYTES: usize = 512 * 1024;
pub const MAX_ACTIVE_STREAMS: usize = 4;
pub const UPLOAD_OWNED_FIELDS: &[&str] = &[
    "stream_id",
    "chunk_data",
    "chunk_index",
    "total_chunks",
    "file_size",
    "expected_sha256",
    "is_complete",
    "reset",
    "verify_only",
];

pub struct StreamOutcome(Value);
impl StreamOutcome {
    pub fn to_json(&self) -> Value {
        self.0.clone()
    }
}

#[derive(Clone)]
pub struct StreamLimits {
    pub max_stream_bytes: u64,
    pub stream_dir: PathBuf,
    pub idle: Duration,
}

/// The caller owns one top-level stream slot, including all automatic-upload
/// subrequests. Internal acknowledgements must not acquire additional slots.
pub async fn consume(
    client: &OneBotClient,
    action: &str,
    params: &Map<String, Value>,
    limits: &StreamLimits,
    cancel: &CancellationToken,
) -> Result<StreamOutcome, StreamError> {
    let response = client
        .call_stream(action, params, cancel)
        .await
        .map_err(|source| StreamError::Transport {
            action: action.into(),
            source,
        })?;
    let mut reader = FrameReader::new(response, action.into(), limits.idle, cancel.clone());
    let first = reader.next_frame().await?;
    let file = FILE_DOWNLOAD_ACTIONS.contains(&action)
        || first
            .as_ref()
            .is_some_and(|frame| frame.data_type() == Some("file_info"));
    reader.put_back(first);
    if file {
        read_file_download(&mut reader, action, limits).await
    } else {
        read_generic(&mut reader, action, params).await
    }
}

fn check_frame(frame: &Frame, action: &str) -> Result<(), StreamError> {
    if !frame.is_ok() || frame.kind() == Some("error") {
        return stream::FrameTerminalSnafu {
            action,
            index: frame.index,
            detail: frame.failure_detail(),
        }
        .fail();
    }
    Ok(())
}

async fn read_file_download(
    reader: &mut FrameReader,
    action: &str,
    limits: &StreamLimits,
) -> Result<StreamOutcome, StreamError> {
    let mut writer: Option<FileWriter> = None;
    let mut hasher = Sha256::new();
    let mut received = 0u64;
    let mut chunks = 0u64;
    let mut declared_size = None;
    let mut terminal: Option<Frame> = None;
    while let Some(frame) = reader.next_frame().await? {
        check_frame(&frame, action)?;
        if terminal.is_some() {
            return protocol(action, frame.index, "frame received after terminal");
        }
        if frame.index == 0 && frame.data_type() != Some("file_info") {
            return protocol(action, 0, "first successful frame must be file_info");
        }
        match frame.data_type() {
            Some("file_info") => {
                if frame.index != 0 || writer.is_some() {
                    return protocol(
                        action,
                        frame.index,
                        "file_info must be the first and only header",
                    );
                }
                let size = frame
                    .data_u64("file_size")
                    .ok_or_else(|| protocol_error(action, frame.index, "invalid file_size"))?;
                if size > limits.max_stream_bytes {
                    return limit(action, "file_size exceeds limit");
                }
                if let Some(value) = frame.data.as_ref().and_then(|data| data.get("chunk_size")) {
                    let chunk_size = safe_u64(value)
                        .filter(|size| *size > 0)
                        .ok_or_else(|| protocol_error(action, frame.index, "invalid chunk_size"))?;
                    if size.div_ceil(chunk_size) > MAX_FILE_CHUNKS {
                        return limit(action, "declared file chunks exceed limit");
                    }
                }
                declared_size = (size > 0).then_some(size);
                writer = Some(
                    FileWriter::create(
                        limits,
                        frame.data_str("file_name").unwrap_or("download.bin"),
                        declared_size.unwrap_or(limits.max_stream_bytes),
                    )
                    .await?,
                );
            }
            Some("file_chunk") => {
                let active = writer.as_mut().ok_or_else(|| {
                    protocol_error(action, frame.index, "file_chunk without file_info")
                })?;
                if chunks >= MAX_FILE_CHUNKS {
                    return limit(action, "file chunks exceed limit");
                }
                if frame.data_u64("index") != Some(chunks) {
                    return protocol(action, frame.index, "unexpected chunk index");
                }
                let encoded = frame.data_str("data").ok_or_else(|| {
                    protocol_error(action, frame.index, "file_chunk.data must be base64 text")
                })?;
                let chunk = base64::engine::general_purpose::STANDARD
                    .decode(encoded)
                    .map_err(|error| {
                        protocol_error(
                            action,
                            frame.index,
                            &format!("file_chunk.data is not valid canonical base64: {error}"),
                        )
                    })?;
                let length = chunk.len() as u64;
                if let Some(size) = frame.data.as_ref().and_then(|data| data.get("size"))
                    && safe_u64(size) != Some(length)
                {
                    return protocol(
                        action,
                        frame.index,
                        "file_chunk.size does not match decoded bytes",
                    );
                }
                let next = received
                    .checked_add(length)
                    .ok_or_else(|| protocol_error(action, frame.index, "download size overflow"))?;
                if declared_size.is_some_and(|size| next > size) {
                    return limit(action, "downloaded bytes exceed declared file_size");
                }
                if next > limits.max_stream_bytes {
                    return limit(action, "downloaded bytes exceed limit");
                }
                active
                    .write(&chunk)
                    .await
                    .map_err(|source| download_io("writing download chunk", source))?;
                hasher.update(&chunk);
                received = next;
                chunks += 1;
            }
            Some("file_complete") => {
                if writer.is_none() {
                    return protocol(action, frame.index, "file_complete without file_info");
                }
                if frame.kind() != Some("response") {
                    return protocol(action, frame.index, "file_complete must be a response");
                }
                terminal = Some(frame);
            }
            _ => return protocol(action, frame.index, "unexpected file frame"),
        }
    }
    let terminal = terminal.ok_or_else(|| StreamError::MissingTerminal {
        action: action.into(),
    })?;
    if declared_size.is_some_and(|size| size != received) {
        return protocol(
            action,
            terminal.index,
            "download size differs from declared file_size",
        );
    }
    if terminal.data_u64("total_bytes") != Some(received) {
        return protocol(
            action,
            terminal.index,
            "terminal total_bytes is missing, invalid or mismatched",
        );
    }
    if let Some(total) = terminal
        .data
        .as_ref()
        .and_then(|data| data.get("total_chunks"))
        && safe_u64(total) != Some(chunks)
    {
        return protocol(action, terminal.index, "terminal total_chunks mismatch");
    }
    let sha256 = hex(&hasher.finalize());
    let file_path = writer
        .expect("a terminal requires a file header")
        .commit()
        .await
        .map_err(|source| download_io("committing download", source))?;
    Ok(StreamOutcome(json!({
        "file_path": file_path, "file_size": received, "sha256": sha256,
        "frame_count": terminal.index + 1, "terminal": terminal.value,
    })))
}

async fn read_generic(
    reader: &mut FrameReader,
    action: &str,
    params: &Map<String, Value>,
) -> Result<StreamOutcome, StreamError> {
    let mut frames = Vec::new();
    let mut text_bytes = 0usize;
    let mut terminal_seen = false;
    while let Some(frame) = reader.next_frame().await? {
        let reset_ack = action == "upload_file_stream"
            && params.get("reset") == Some(&json!(true))
            && frame
                .failure_detail()
                .to_lowercase()
                .contains("stream reset completed");
        if !reset_ack {
            check_frame(&frame, action)?;
        }
        if terminal_seen {
            return protocol(
                action,
                frame.index,
                "frame received after response terminal",
            );
        }
        if matches!(
            frame.data_type(),
            Some("file_info" | "file_chunk" | "file_complete")
        ) {
            return protocol(action, frame.index, "file frame without initial file_info");
        }
        if frames.len() >= MAX_GENERIC_FRAMES {
            return limit(action, "generic frame summary exceeds limit (256)");
        }
        text_bytes += frame.wire_bytes;
        if text_bytes > MAX_GENERIC_TEXT_BYTES {
            return limit(action, "generic summary text exceeds limit (262144 bytes)");
        }
        terminal_seen = frame.kind() == Some("response");
        frames.push(frame.value);
    }
    if frames.is_empty()
        || (!terminal_seen && !(action == "upload_file_stream" && frames.len() == 1))
    {
        return stream::MissingTerminalSnafu { action }.fail();
    }
    let count = frames.len();
    let terminal = frames.pop().expect("nonempty frames");
    Ok(StreamOutcome(
        json!({"frame_count": count, "frames": frames, "terminal": terminal}),
    ))
}

/// Read the input incrementally and let the MCP own all transfer bookkeeping.
pub async fn upload_local_file(
    client: &OneBotClient,
    input_file: &Path,
    params: Map<String, Value>,
    limits: &StreamLimits,
    cancel: &CancellationToken,
) -> Result<Value, StreamError> {
    const ACTION: &str = "upload_file_stream";
    if let Some(field) = UPLOAD_OWNED_FIELDS
        .iter()
        .find(|field| params.contains_key(**field))
    {
        return upload_protocol(&format!(
            "params conflict with MCP-owned transfer field: {field}"
        ));
    }
    let stream_id = Uuid::new_v4().to_string();
    let mut remote_touched = false;
    let mut result = async {
        check_cancel(cancel)?;
        let mut options = fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        let mut file = options.open(input_file).await.map_err(|source| upload_io("opening input_file", source))?;
        let initial = file.metadata().await.map_err(|source| upload_io("reading input metadata", source))?;
        if !initial.is_file() { return upload_protocol("input_file is not a regular file"); }
        if initial.len() > limits.max_stream_bytes { return limit(ACTION, "input_file exceeds limit"); }
        let file_size = initial.len();
        let total_chunks = file_size.div_ceil(UPLOAD_CHUNK_BYTES as u64).max(1);
        let filename = params.get("filename").and_then(Value::as_str).filter(|value| !value.is_empty()).map(str::to_string)
            .unwrap_or_else(|| input_file.file_name().unwrap_or_default().to_string_lossy().into_owned());
        let mut hash = Sha256::new();
        let mut read = 0u64;
        for index in 0..total_chunks {
            check_cancel(cancel)?;
            let wanted = (file_size - read).min(UPLOAD_CHUNK_BYTES as u64) as usize;
            let mut chunk = vec![0; wanted];
            tokio::select! {
                _ = cancel.cancelled() => return stream::CancelledSnafu { action: ACTION }.fail(),
                result = file.read_exact(&mut chunk) => result.map_err(|source| upload_io("input_file changed while reading", source))?,
            };
            check_cancel(cancel)?;
            hash.update(&chunk);
            read += chunk.len() as u64;
            let mut request = params.clone();
            request.extend(json!({"stream_id": stream_id, "chunk_data": base64::engine::general_purpose::STANDARD.encode(&chunk),
                "chunk_index": index, "total_chunks": total_chunks, "file_size": file_size, "filename": filename}).as_object().unwrap().clone());
            remote_touched = true;
            let ack = consume(client, ACTION, &request, limits, cancel).await?.to_json();
            check_cancel(cancel)?;
            if ack["terminal"]["data"]["type"] != "stream" || ack["terminal"]["data"]["status"] != "chunk_received" {
                return upload_protocol(&format!("unexpected chunk acknowledgement: {}", ack["terminal"]));
            }
        }
        let after = file.metadata().await.map_err(|source| upload_io("verifying input_file", source))?;
        if after.len() != file_size || after.modified().ok() != initial.modified().ok() || read != file_size { return upload_protocol("input_file changed during upload"); }
        drop(file);
        check_cancel(cancel)?;
        let sha256 = hex(&hash.finalize());
        let mut request = params.clone();
        request.extend(json!({"stream_id": stream_id, "total_chunks": total_chunks, "file_size": file_size,
            "expected_sha256": sha256, "is_complete": true, "filename": filename}).as_object().unwrap().clone());
        let completed = consume(client, ACTION, &request, limits, cancel).await?.to_json();
        check_cancel(cancel)?;
        let data = &completed["terminal"]["data"];
        if data["type"] != "response" || data["status"] != "file_complete" { return upload_protocol("unexpected completion acknowledgement"); }
        if safe_u64(&data["file_size"]) != Some(file_size) { return upload_protocol("remote file_size mismatch"); }
        if data["sha256"].as_str() != Some(sha256.as_str()) { return upload_protocol("remote sha256 mismatch"); }
        let path = data["file_path"].as_str().filter(|path| !path.is_empty()).ok_or_else(|| StreamError::UploadProtocol { action: ACTION.into(), reason: "completion acknowledgement has no file_path".into() })?;
        Ok(json!({"file_path": path, "file_size": file_size, "sha256": sha256, "chunk_count": total_chunks, "stream_id": stream_id, "terminal": completed["terminal"]}))
    }.await;
    if result.is_err() && remote_touched {
        // Reset must still run after caller cancellation. It cannot undo a
        // committed file if the completion acknowledgement was lost.
        let fresh = CancellationToken::new();
        let mut reset_limits = limits.clone();
        reset_limits.idle = Duration::from_secs(5);
        let reset = json!({"stream_id": stream_id, "reset": true})
            .as_object()
            .unwrap()
            .clone();
        let reset = tokio::time::timeout(
            Duration::from_secs(5),
            consume(client, ACTION, &reset, &reset_limits, &fresh),
        )
        .await;
        let reset_error = match reset {
            Ok(Ok(_)) => None,
            Ok(Err(error)) => Some(error.to_string()),
            Err(_) => Some("reset timed out after 5000ms".into()),
        };
        if let Some(detail) = reset_error {
            let original = result.as_ref().expect_err("reset follows a failed upload");
            result = upload_protocol(&format!("{original}; remote reset failed: {detail}"));
        }
    }
    result
}

fn check_cancel(cancel: &CancellationToken) -> Result<(), StreamError> {
    if cancel.is_cancelled() {
        return stream::CancelledSnafu {
            action: "upload_file_stream",
        }
        .fail();
    }
    Ok(())
}
fn upload_protocol<T>(reason: &str) -> Result<T, StreamError> {
    Err(StreamError::UploadProtocol {
        action: "upload_file_stream".into(),
        reason: reason.into(),
    })
}
fn upload_io(reason: &str, source: std::io::Error) -> StreamError {
    StreamError::Upload {
        action: "upload_file_stream".into(),
        reason: reason.into(),
        source,
    }
}
fn download_io(reason: &str, source: std::io::Error) -> StreamError {
    StreamError::Download {
        action: "download".into(),
        reason: reason.into(),
        source,
    }
}
fn limit<T>(action: &str, reason: &str) -> Result<T, StreamError> {
    stream::LimitsSnafu { action, reason }.fail()
}
fn protocol<T>(action: &str, index: usize, reason: &str) -> Result<T, StreamError> {
    Err(protocol_error(action, index, reason))
}
fn protocol_error(action: &str, index: usize, reason: &str) -> StreamError {
    StreamError::Protocol {
        action: action.into(),
        index,
        reason: reason.into(),
    }
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

// Share reservations across App instances targeting the same real directory.
type Reservations = BTreeMap<PathBuf, BTreeMap<PathBuf, (PathBuf, u64)>>;
static RESERVATIONS: OnceLock<Mutex<Reservations>> = OnceLock::new();
struct Reservation {
    dir: PathBuf,
    part: PathBuf,
}
impl Drop for Reservation {
    fn drop(&mut self) {
        let mut all = RESERVATIONS.get_or_init(Mutex::default).lock().unwrap();
        if let Some(active) = all.get_mut(&self.dir) {
            active.remove(&self.part);
            if active.is_empty() {
                all.remove(&self.dir);
            }
        }
    }
}
fn reserve(
    dir: &Path,
    part: &Path,
    final_path: &Path,
    bytes: u64,
    quota: u64,
) -> Result<Reservation, StreamError> {
    let mut all = RESERVATIONS.get_or_init(Mutex::default).lock().unwrap();
    let active = all.entry(dir.to_path_buf()).or_default();
    let reserved: u64 = active.values().map(|(_, size)| *size).sum();
    let mut existing = 0u64;
    for entry in std::fs::read_dir(dir)
        .map_err(|source| download_io("scanning download directory", source))?
    {
        let entry = entry.map_err(|source| download_io("reading download entry", source))?;
        let path = entry.path();
        if active.contains_key(&path) || active.values().any(|(final_path, _)| *final_path == path)
        {
            continue;
        }
        if !entry
            .file_type()
            .map_err(|source| download_io("reading file type", source))?
            .is_file()
        {
            continue;
        }
        match entry.metadata() {
            Ok(metadata) => existing = existing.saturating_add(metadata.len()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(download_io("reading existing download size", error)),
        }
    }
    if existing.saturating_add(reserved).saturating_add(bytes) > quota {
        return limit("download", "download directory quota exceeded");
    }
    active.insert(part.to_path_buf(), (final_path.to_path_buf(), bytes));
    Ok(Reservation {
        dir: dir.into(),
        part: part.into(),
    })
}
struct FileWriter {
    handle: Option<fs::File>,
    part: PathBuf,
    final_path: PathBuf,
    committed: bool,
    _reservation: Reservation,
}
impl FileWriter {
    async fn create(
        limits: &StreamLimits,
        name: &str,
        reserve_bytes: u64,
    ) -> Result<Self, StreamError> {
        let dir = limits
            .stream_dir
            .canonicalize()
            .map_err(|source| download_io("resolving download directory", source))?;
        let final_path = dir.join(format!("{}-{}", Uuid::new_v4(), sanitize_name(name)));
        let part = dir.join(format!(
            "{}.part",
            final_path.file_name().unwrap().to_string_lossy()
        ));
        let reservation = reserve(
            &dir,
            &part,
            &final_path,
            reserve_bytes,
            2 * limits.max_stream_bytes,
        )?;
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        let handle = options
            .open(&part)
            .await
            .map_err(|source| download_io("creating partial file", source))?;
        Ok(Self {
            handle: Some(handle),
            part,
            final_path,
            committed: false,
            _reservation: reservation,
        })
    }
    async fn write(&mut self, chunk: &[u8]) -> std::io::Result<()> {
        self.handle.as_mut().unwrap().write_all(chunk).await
    }
    async fn commit(mut self) -> std::io::Result<String> {
        if let Some(mut handle) = self.handle.take() {
            handle.flush().await?;
            handle.sync_all().await?;
        }
        fs::rename(&self.part, &self.final_path).await?;
        self.committed = true;
        Ok(self.final_path.to_string_lossy().into_owned())
    }
}
impl Drop for FileWriter {
    fn drop(&mut self) {
        if !self.committed {
            self.handle.take();
            let _ = std::fs::remove_file(&self.part);
        }
    }
}
fn sanitize_name(name: &str) -> String {
    let base = Path::new(name)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let clean: String = base
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    let clean: String = clean.trim_start_matches('.').chars().take(180).collect();
    if clean.is_empty() {
        "download.bin".into()
    } else {
        clean
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filenames_and_hashes_are_stable() {
        assert_eq!(sanitize_name("../../a b.png"), "a_b.png");
        assert_eq!(sanitize_name(""), "download.bin");
        assert_eq!(
            hex(&Sha256::digest(b"hello")),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }
}

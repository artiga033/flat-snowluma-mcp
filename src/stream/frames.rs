//! Stream Action frame decoding.
//!
//! SnowLuma's Stream Actions reply with a sequence of JSON objects separated by
//! `\r\n\r\n` (verified against the running instance, and matching the official
//! MCP client). Each frame is a normal OneBot envelope with an extra `stream`
//! marker and a `data.data_type` discriminator.
//!
//! Preserve the full frame sequence and its completion or failure status,
//! never just the last frame.

use bytes::Bytes;
use futures_util::StreamExt;
use futures_util::stream::BoxStream;
use reqwest::Response;
use serde_json::{Map, Value};
use tokio::time::{Instant, timeout_at};
use tokio_util::sync::CancellationToken;

use crate::error::{StreamError, stream};

/// One decoded frame with its position and convenience accessors.
#[derive(Debug, Clone)]
pub struct Frame {
    pub index: usize,
    pub wire_bytes: usize,
    pub value: Value,
    /// `data` as an object, when it is one.
    pub data: Option<Map<String, Value>>,
}

impl Frame {
    /// The `data.data_type` discriminator.
    pub fn data_type(&self) -> Option<&str> {
        self.data
            .as_ref()
            .and_then(|data| data.get("data_type"))
            .and_then(Value::as_str)
    }

    /// The `data.type` field (`stream` | `response` | `error`).
    pub fn kind(&self) -> Option<&str> {
        self.data
            .as_ref()
            .and_then(|data| data.get("type"))
            .and_then(Value::as_str)
    }

    /// Whether the envelope reports success.
    pub fn is_ok(&self) -> bool {
        let status = self.value.get("status").and_then(Value::as_str);
        let retcode = self.value.get("retcode").and_then(Value::as_f64);
        status == Some("ok") && retcode == Some(0.0)
    }

    /// A short failure reason, if the envelope reports one.
    pub fn failure_detail(&self) -> String {
        for key in ["wording", "message"] {
            if let Some(text) = self.value.get(key).and_then(Value::as_str)
                && !text.is_empty()
            {
                return text.to_string();
            }
        }
        match self.value.get("retcode").and_then(Value::as_i64) {
            Some(code) => format!("retcode={code}"),
            None => "stream frame reported failure".to_string(),
        }
    }

    /// Access a numeric field inside `data`.
    pub fn data_u64(&self, key: &str) -> Option<u64> {
        self.data
            .as_ref()
            .and_then(|data| data.get(key))
            .and_then(safe_u64)
    }

    pub fn data_str(&self, key: &str) -> Option<&str> {
        self.data
            .as_ref()
            .and_then(|data| data.get(key))
            .and_then(Value::as_str)
    }
}

/// Reads `\r\n\r\n`-delimited JSON frames from a response body.
pub struct FrameReader {
    pending: Option<Frame>,
    body: BoxStream<'static, reqwest::Result<Bytes>>,
    buffer: Vec<u8>,
    finished: bool,
    action: String,
    idle: std::time::Duration,
    cancel: CancellationToken,
    next_index: usize,
    /// Total wire bytes buffered but not yet framed, for the size guard.
    max_wire_frame: usize,
}

impl FrameReader {
    pub fn new(
        response: Response,
        action: String,
        idle: std::time::Duration,
        cancel: CancellationToken,
    ) -> Self {
        FrameReader {
            pending: None,
            body: response.bytes_stream().boxed(),
            buffer: Vec::new(),
            finished: false,
            action,
            idle,
            cancel,
            next_index: 0,
            max_wire_frame: 24 * 1024 * 1024,
        }
    }

    pub fn put_back(&mut self, frame: Option<Frame>) {
        self.pending = frame;
    }

    /// Read the next frame, or `None` at a clean end of stream.
    pub async fn next_frame(&mut self) -> Result<Option<Frame>, StreamError> {
        if self.cancel.is_cancelled() {
            return stream::CancelledSnafu {
                action: self.action.clone(),
            }
            .fail();
        }
        if let Some(frame) = self.pending.take() {
            return Ok(Some(frame));
        }
        loop {
            if let Some(frame) = self.take_buffered_frame()? {
                return Ok(Some(frame));
            }
            if self.finished {
                if self.buffer.iter().all(|b| b.is_ascii_whitespace()) {
                    return Ok(None);
                }
                return stream::ProtocolSnafu {
                    action: self.action.clone(),
                    index: self.next_index,
                    reason: "stream ended with an unterminated frame".to_string(),
                }
                .fail();
            }
            self.read_more().await?;
        }
    }

    /// Pull one chunk from the body under the idle deadline and cancellation.
    async fn read_more(&mut self) -> Result<(), StreamError> {
        let deadline = Instant::now() + self.idle;
        let index = self.next_index;
        let action = self.action.clone();

        let chunk = tokio::select! {
            biased;
            _ = self.cancel.cancelled() => {
                return stream::CancelledSnafu { action }.fail();
            }
            result = timeout_at(deadline, self.body.next()) => {
                match result {
                    Err(_) => {
                        return stream::IdleTimeoutSnafu {
                            action,
                            index,
                            millis: self.idle.as_millis(),
                        }
                        .fail();
                    }
                    Ok(None) => None,
                    Ok(Some(Err(source))) => {
                        return Err(StreamError::Transport {
                            action: action.clone(),
                            source: crate::error::OneBotError::Request {
                                action: action.clone(),
                                source,
                            },
                        });
                    }
                    Ok(Some(Ok(chunk))) => Some(chunk),
                }
            }
        };

        match chunk {
            None => self.finished = true,
            Some(bytes) => self.push(&bytes)?,
        }
        Ok(())
    }

    fn push(&mut self, bytes: &Bytes) -> Result<(), StreamError> {
        self.buffer.extend_from_slice(bytes);
        if self.buffer.len() > self.max_wire_frame && !self.has_delimiter() {
            return stream::LimitsSnafu {
                action: self.action.clone(),
                reason: format!(
                    "a single frame exceeded the {} byte wire limit",
                    self.max_wire_frame
                ),
            }
            .fail();
        }
        Ok(())
    }

    fn has_delimiter(&self) -> bool {
        find_delimiter(&self.buffer).is_some()
    }

    /// Extract a frame from the buffer if a complete delimiter is present.
    fn take_buffered_frame(&mut self) -> Result<Option<Frame>, StreamError> {
        let Some(position) = find_delimiter(&self.buffer) else {
            return Ok(None);
        };
        if position > self.max_wire_frame {
            return stream::LimitsSnafu {
                action: self.action.clone(),
                reason: "wire frame exceeds limit (25165824 bytes)",
            }
            .fail();
        }
        let raw = self.buffer[..position].to_vec();
        self.buffer.drain(..position + 4);
        let index = self.next_index;
        self.next_index += 1;

        if raw.is_empty() {
            return stream::ProtocolSnafu {
                action: self.action.clone(),
                index,
                reason: "received an empty frame".to_string(),
            }
            .fail();
        }
        let value: Value =
            serde_json::from_slice(&raw).map_err(|source| StreamError::FrameJson {
                action: self.action.clone(),
                index,
                source,
            })?;
        let Value::Object(object) = value else {
            return stream::FrameShapeSnafu {
                action: self.action.clone(),
                index,
            }
            .fail();
        };
        let data = object.get("data").and_then(Value::as_object).cloned();
        Ok(Some(Frame {
            index,
            wire_bytes: raw.len(),
            value: Value::Object(object),
            data,
        }))
    }
}

/// Find the `\r\n\r\n` frame delimiter.
fn find_delimiter(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

/// JSON integers may be written as 5 or 5.0. Match JavaScript's safe-integer
/// semantics rather than serde_json's lexical number representation.
pub(super) fn safe_u64(value: &Value) -> Option<u64> {
    let number = value.as_f64()?;
    ((0.0..=9_007_199_254_740_991.0).contains(&number) && number.fract() == 0.0)
        .then_some(number as u64)
}

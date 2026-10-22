//! OneBot v11 HTTP client.
//!
//! Like the official MCP, POST `{action, params}` directly and preserve the
//! original response envelope. Stream responses use connection and idle deadlines.

use std::sync::Arc;
use std::time::Duration;

use reqwest::Client;
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

use crate::error::{OneBotError, onebot};

/// A parsed OneBot response envelope.
#[derive(Debug, Clone)]
pub struct Envelope {
    pub status: Option<String>,
    pub retcode: Option<i64>,
    pub wording: Option<String>,
    pub message: Option<String>,
    pub data: Option<Value>,
    /// The raw object, preserved for results that keep the original structure.
    pub raw: Value,
}

impl Envelope {
    /// Whether the server reported success.
    pub fn is_ok(&self) -> bool {
        self.status.as_deref() == Some("ok") && self.retcode.unwrap_or(0) == 0
    }

    /// A short human-readable failure reason.
    pub fn failure_detail(&self) -> String {
        if let Some(wording) = self.wording.as_deref().filter(|w| !w.is_empty()) {
            return wording.to_string();
        }
        if let Some(message) = self.message.as_deref().filter(|m| !m.is_empty()) {
            return message.to_string();
        }
        match self.retcode {
            Some(code) => format!("retcode={code}"),
            None => "request failed".to_string(),
        }
    }
}

/// Shared HTTP client plus endpoint configuration.
#[derive(Clone)]
pub struct OneBotClient {
    inner: Arc<Inner>,
}

struct Inner {
    http: Client,
    endpoint: String,
    token: Option<String>,
    timeout: Duration,
}

impl OneBotClient {
    pub fn new(
        endpoint: String,
        token: Option<String>,
        timeout: Duration,
    ) -> Result<OneBotClient, OneBotError> {
        let http = Client::builder()
            .connect_timeout(timeout)
            .build()
            .map_err(|source| OneBotError::Request {
                action: "client".to_string(),
                source,
            })?;
        Ok(OneBotClient {
            inner: Arc::new(Inner {
                http,
                endpoint,
                token,
                timeout,
            }),
        })
    }

    pub fn timeout(&self) -> Duration {
        self.inner.timeout
    }

    /// Call an ordinary (single-response) action.
    pub async fn call(
        &self,
        action: &str,
        params: &Map<String, Value>,
        cancel: &CancellationToken,
    ) -> Result<Envelope, OneBotError> {
        let body = json!({ "action": action, "params": params });
        let body = serde_json::to_vec(&body).map_err(|source| OneBotError::EncodeParams {
            action: action.to_string(),
            source,
        })?;

        let mut request = self
            .inner
            .http
            .post(&self.inner.endpoint)
            .timeout(self.inner.timeout)
            .header("content-type", "application/json")
            .header("accept", "application/json")
            .body(body);
        if let Some(token) = &self.inner.token {
            request = request.bearer_auth(token);
        }

        let response = tokio::select! {
            biased;
            _ = cancel.cancelled() => return onebot::CancelledSnafu { action }.fail(),
            result = request.send() => result.map_err(|source| {
                if source.is_timeout() {
                    OneBotError::Timeout { action: action.to_string() }
                } else {
                    OneBotError::Request { action: action.to_string(), source }
                }
            })?,
        };

        let status = response.status();
        let text = tokio::select! {
            _ = cancel.cancelled() => return onebot::CancelledSnafu { action }.fail(),
            result = response.text() => result.map_err(|source| OneBotError::Request { action: action.to_string(), source })?,
        };
        let value: Value = serde_json::from_str(&text).map_err(|_| OneBotError::NonJson {
            action: action.to_string(),
            status: status.as_u16(),
        })?;
        envelope_from_value(action, value)
    }

    /// Start a Stream Action request and hand back the raw response.
    ///
    /// The connection timeout is the configured timeout; callers apply their own
    /// per-read idle timeout on the body.
    pub async fn call_stream(
        &self,
        action: &str,
        params: &Map<String, Value>,
        cancel: &CancellationToken,
    ) -> Result<reqwest::Response, OneBotError> {
        let body = json!({ "action": action, "params": params });
        let body = serde_json::to_vec(&body).map_err(|source| OneBotError::EncodeParams {
            action: action.to_string(),
            source,
        })?;

        let mut request = self
            .inner
            .http
            .post(&self.inner.endpoint)
            .header("content-type", "application/json")
            .header("accept", "application/json")
            .body(body);
        if let Some(token) = &self.inner.token {
            request = request.bearer_auth(token);
        }

        let response = tokio::select! {
            biased;
            _ = cancel.cancelled() => return onebot::CancelledSnafu { action }.fail(),
            result = tokio::time::timeout(self.inner.timeout, request.send()) => result.map_err(|_| OneBotError::Timeout { action: action.to_string() })?.map_err(|source| {
                if source.is_timeout() {
                    OneBotError::Timeout { action: action.to_string() }
                } else {
                    OneBotError::Request { action: action.to_string(), source }
                }
            })?,
        };

        if !response.status().is_success() {
            return onebot::HttpSnafu {
                action,
                status: response.status().as_u16(),
            }
            .fail();
        }
        Ok(response)
    }
}

fn envelope_from_value(action: &str, value: Value) -> Result<Envelope, OneBotError> {
    let Value::Object(map) = &value else {
        return onebot::EnvelopeShapeSnafu { action }.fail();
    };
    Ok(Envelope {
        status: map
            .get("status")
            .and_then(Value::as_str)
            .map(str::to_string),
        retcode: map.get("retcode").and_then(Value::as_i64),
        wording: map
            .get("wording")
            .and_then(Value::as_str)
            .map(str::to_string),
        message: map
            .get("message")
            .and_then(Value::as_str)
            .map(str::to_string),
        data: map.get("data").cloned(),
        raw: value,
    })
}

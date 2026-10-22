//! Error types for the flat SnowLuma MCP server.
//!
//! Every fallible path returns one of these. `OneBotError` covers the wire
//! layer (transport, HTTP status, envelope decoding) and `StreamError` covers
//! the multi-frame Stream Action protocol. Both convert into MCP tool-level
//! errors at the boundary; see `tools::error_result`.

use snafu::Snafu;

/// Failures that stop the process from starting.
#[derive(Debug, Snafu)]
#[snafu(module(init), visibility(pub))]
pub enum InitError {
    #[snafu(display("invalid {name}: {reason}"))]
    InvalidConfig { name: String, reason: String },

    #[snafu(display("catalog snapshot is not valid JSON: {source}"))]
    CatalogJson { source: serde_json::Error },

    #[snafu(display("catalog snapshot is malformed: {reason}"))]
    CatalogShape { reason: String },

    #[snafu(display("cannot prepare {purpose} directory {path}: {source}"))]
    PrepareDir {
        purpose: &'static str,
        path: String,
        source: std::io::Error,
    },

    #[snafu(display("{purpose} path {path} is not a directory"))]
    NotADirectory { purpose: &'static str, path: String },

    #[snafu(display("failed to serve MCP over stdio: {source}"))]
    Serve {
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

/// Failures while talking to the OneBot HTTP endpoint.
#[derive(Debug, Snafu)]
#[snafu(module(onebot), visibility(pub))]
pub enum OneBotError {
    #[snafu(display("request for `{action}` failed: {source}"))]
    Request {
        action: String,
        source: reqwest::Error,
    },

    #[snafu(display("request for `{action}` timed out"))]
    Timeout { action: String },

    #[snafu(display("request for `{action}` was cancelled"))]
    Cancelled { action: String },

    #[snafu(display("`{action}` returned HTTP {status}"))]
    Http { action: String, status: u16 },

    #[snafu(display("`{action}` returned non-JSON (HTTP {status})"))]
    NonJson { action: String, status: u16 },

    #[snafu(display("`{action}` returned a response that is not a JSON object"))]
    EnvelopeShape { action: String },

    #[snafu(display("`{action}` could not serialize its parameters: {source}"))]
    EncodeParams {
        action: String,
        source: serde_json::Error,
    },
}

/// Failures while validating tool arguments.
#[derive(Debug, Snafu)]
#[snafu(module(validation), visibility(pub))]
pub enum ValidationError {
    #[snafu(display("unknown field(s) {unknown:?} for `{action}`; legal fields: {legal}"))]
    UnknownFields {
        action: String,
        unknown: Vec<String>,
        legal: String,
    },

    #[snafu(display("`{action}`: {reason}"))]
    Invariant { action: String, reason: String },
}

/// Failures in the Stream Action framing protocol.
#[derive(Debug, Snafu)]
#[snafu(module(stream), visibility(pub))]
pub enum StreamError {
    #[snafu(display("{action}: {source}"))]
    Transport { action: String, source: OneBotError },

    #[snafu(display("{action}: stream returned HTTP {status}"))]
    Http { action: String, status: u16 },

    #[snafu(display("{action}: response body is unavailable"))]
    NoBody { action: String },

    #[snafu(display("{action}: frame {index} is not valid JSON: {source}"))]
    FrameJson {
        action: String,
        index: usize,
        source: serde_json::Error,
    },

    #[snafu(display("{action}: frame {index} is not a JSON object"))]
    FrameShape { action: String, index: usize },

    #[snafu(display("{action}: frame {index} reports an error: {detail}"))]
    FrameTerminal {
        action: String,
        index: usize,
        detail: String,
    },

    #[snafu(display("{action}: frame {index}: {reason}"))]
    Protocol {
        action: String,
        index: usize,
        reason: String,
    },

    #[snafu(display("{action}: stream ended before a terminal frame was received"))]
    MissingTerminal { action: String },

    #[snafu(display("{action}: idle timeout after {millis}ms while reading frame {index}"))]
    IdleTimeout {
        action: String,
        index: usize,
        /// `Duration::as_millis()` is `u128` and can exceed `u64` for large
        /// durations, so the value is stored without narrowing.
        millis: u128,
    },

    #[snafu(display("{action}: cancelled"))]
    Cancelled { action: String },

    #[snafu(display("{action}: too many concurrent stream actions (limit {limit})"))]
    TooManyActive { action: String, limit: usize },

    #[snafu(display("{action}: {reason}"))]
    Upload {
        action: String,
        reason: String,
        source: std::io::Error,
    },

    #[snafu(display("{action}: {reason}"))]
    UploadProtocol { action: String, reason: String },

    #[snafu(display("{action}: {reason}"))]
    Download {
        action: String,
        reason: String,
        source: std::io::Error,
    },

    #[snafu(display("{action}: {reason}"))]
    Limits { action: String, reason: String },
}

impl StreamError {
    /// Human-readable detail for the MCP error content.
    pub fn detail(&self) -> String {
        self.to_string()
    }
}

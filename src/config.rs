//! Runtime configuration from the environment.
//!
//! Address and authentication come from the environment. Variable
//! names mirror the official `@snowluma/mcp` server so existing connection
//! settings can be reused; the endpoint is what actually enables execution.

use std::path::PathBuf;
use std::time::Duration;

use crate::error::{InitError, init};

const DEFAULT_TIMEOUT_MS: u64 = 30_000;
const MAX_TIMEOUT_MS: u64 = 2_147_483_647;
const DEFAULT_MAX_STREAM_BYTES: u64 = 4 * 1024 * 1024 * 1024;

/// Where the server finds SnowLuma and what limits it enforces.
#[derive(Debug, Clone)]
pub struct Config {
    /// OneBot HTTP endpoint. `None` means the server has no live connection.
    pub endpoint: Option<String>,
    pub token: Option<String>,
    /// Ordinary-request timeout, and Stream connection / per-read idle timeout.
    pub timeout: Duration,
    /// Directory on the MCP host for completed stream downloads.
    pub stream_dir: PathBuf,
    /// Existing directory on the MCP host uploads may read from.
    pub upload_root: Option<PathBuf>,
    /// Per-file download/upload ceiling.
    pub max_stream_bytes: u64,
}

impl Config {
    /// Read and validate configuration from the process environment.
    pub fn from_env() -> Result<Config, InitError> {
        let endpoint = non_empty("SNOWLUMA_MCP_ENDPOINT");
        let token = non_empty("SNOWLUMA_MCP_TOKEN");
        let timeout_ms = parse_timeout(non_empty("SNOWLUMA_MCP_TIMEOUT_MS"))?;
        let max_stream_bytes = parse_max_stream_bytes(non_empty("SNOWLUMA_MCP_MAX_STREAM_BYTES"))?;

        let stream_dir = match non_empty("SNOWLUMA_MCP_STREAM_DIR") {
            Some(raw) => PathBuf::from(raw),
            None => std::env::temp_dir().join("flat-snowluma-downloads"),
        };
        let upload_root = non_empty("SNOWLUMA_MCP_UPLOAD_ROOT").map(PathBuf::from);

        Ok(Config {
            endpoint,
            token,
            timeout: Duration::from_millis(timeout_ms),
            stream_dir,
            upload_root,
            max_stream_bytes,
        })
    }

    /// Whether execution against a live instance is configured.
    pub fn can_execute(&self) -> bool {
        self.endpoint.is_some()
    }

    /// Milliseconds form of the timeout, for error messages.
    ///
    /// `Duration::as_millis()` is `u128`; returning it unchanged avoids a
    /// narrowing cast that could truncate for very large durations.
    pub fn timeout_millis(&self) -> u128 {
        self.timeout.as_millis()
    }
}

fn non_empty(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn parse_timeout(raw: Option<String>) -> Result<u64, InitError> {
    let Some(raw) = raw else {
        return Ok(DEFAULT_TIMEOUT_MS);
    };
    let parsed: u64 = raw.parse().map_err(|_| InitError::InvalidConfig {
        name: "SNOWLUMA_MCP_TIMEOUT_MS".to_string(),
        reason: format!("`{raw}` is not a positive integer"),
    })?;
    if parsed == 0 || parsed > MAX_TIMEOUT_MS {
        return init::InvalidConfigSnafu {
            name: "SNOWLUMA_MCP_TIMEOUT_MS".to_string(),
            reason: format!("must be between 1 and {MAX_TIMEOUT_MS}, got {parsed}"),
        }
        .fail();
    }
    Ok(parsed)
}

fn parse_max_stream_bytes(raw: Option<String>) -> Result<u64, InitError> {
    let Some(raw) = raw else {
        return Ok(DEFAULT_MAX_STREAM_BYTES);
    };
    let parsed: u64 = raw.parse().map_err(|_| InitError::InvalidConfig {
        name: "SNOWLUMA_MCP_MAX_STREAM_BYTES".to_string(),
        reason: format!("`{raw}` is not a positive integer"),
    })?;
    if parsed == 0 || parsed > DEFAULT_MAX_STREAM_BYTES {
        return init::InvalidConfigSnafu {
            name: "SNOWLUMA_MCP_MAX_STREAM_BYTES".to_string(),
            reason: format!("must be between 1 and {DEFAULT_MAX_STREAM_BYTES}, got {parsed}"),
        }
        .fail();
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_apply_when_unset() {
        // The test environment does not set these; keep the assertions tolerant.
        assert!(parse_timeout(None).unwrap() == DEFAULT_TIMEOUT_MS);
        assert!(parse_max_stream_bytes(None).unwrap() == DEFAULT_MAX_STREAM_BYTES);
    }

    #[test]
    fn rejects_zero_and_non_numeric() {
        assert!(parse_timeout(Some("0".to_string())).is_err());
        assert!(parse_timeout(Some("abc".to_string())).is_err());
        assert!(parse_max_stream_bytes(Some("0".to_string())).is_err());
        assert!(parse_max_stream_bytes(Some("-1".to_string())).is_err());
    }
}

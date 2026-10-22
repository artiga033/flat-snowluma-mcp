//! Entry point for the flat SnowLuma MCP server (stdio transport).
//!
//! stdout is the MCP protocol channel; all diagnostics go to stderr.

use std::process::ExitCode;

use flat_snowluma_mcp::catalog::Catalog;
use flat_snowluma_mcp::config::Config;
use flat_snowluma_mcp::error::InitError;
use flat_snowluma_mcp::onebot::OneBotClient;
use flat_snowluma_mcp::server::SnowLumaServer;
use flat_snowluma_mcp::tools::App;
use rmcp::{ServiceExt, transport::stdio};

#[tokio::main]
async fn main() -> ExitCode {
    init_tracing();
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(%error, "flat-snowluma-mcp exited with an error");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), InitError> {
    let catalog = Catalog::load()?;
    let config = Config::from_env()?;

    prepare_stream_dir(&config)?;

    let client = match &config.endpoint {
        Some(endpoint) => {
            tracing::info!(%endpoint, "execution enabled");
            Some(
                OneBotClient::new(endpoint.clone(), config.token.clone(), config.timeout).map_err(
                    |source| InitError::Serve {
                        source: Box::new(source),
                    },
                )?,
            )
        }
        None => {
            tracing::info!("no SNOWLUMA_MCP_ENDPOINT set; serving declarations only");
            None
        }
    };

    let app = App::new(catalog, client, config)?;
    let server = SnowLumaServer::new(app, catalog);

    tracing::info!(
        actions = catalog.len(),
        tools = server_tool_count(&server),
        version = catalog.version,
        hash = catalog.hash,
        "ready"
    );

    server
        .serve(stdio())
        .await
        .map_err(|source| InitError::Serve {
            source: Box::new(source),
        })?
        .waiting()
        .await
        .map_err(|source| InitError::Serve {
            source: Box::new(source),
        })?;
    Ok(())
}

fn server_tool_count(server: &SnowLumaServer) -> usize {
    flat_snowluma_mcp::server::declared_tools(server).len()
}

/// Create and validate the download directory on the MCP host.
fn prepare_stream_dir(config: &Config) -> Result<(), InitError> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(&config.stream_dir)
        .map_err(|source| InitError::PrepareDir {
            purpose: "download",
            path: config.stream_dir.display().to_string(),
            source,
        })?;
    if !config.stream_dir.is_dir() {
        return Err(InitError::NotADirectory {
            purpose: "download",
            path: config.stream_dir.display().to_string(),
        });
    }
    std::fs::read_dir(&config.stream_dir).map_err(|source| InitError::PrepareDir {
        purpose: "download",
        path: config.stream_dir.display().to_string(),
        source,
    })?;
    let probe = config
        .stream_dir
        .join(format!(".flat-snowluma-probe-{}", uuid::Uuid::new_v4()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let handle = options
        .open(&probe)
        .map_err(|source| InitError::PrepareDir {
            purpose: "download",
            path: config.stream_dir.display().to_string(),
            source,
        })?;
    drop(handle);
    std::fs::remove_file(&probe).map_err(|source| InitError::PrepareDir {
        purpose: "download",
        path: probe.display().to_string(),
        source,
    })?;
    if let Some(root) = &config.upload_root {
        let resolved = root
            .canonicalize()
            .map_err(|source| InitError::PrepareDir {
                purpose: "upload",
                path: root.display().to_string(),
                source,
            })?;
        if !resolved.is_dir() {
            return Err(InitError::NotADirectory {
                purpose: "upload",
                path: resolved.display().to_string(),
            });
        }
    }
    Ok(())
}

fn init_tracing() {
    use tracing_subscriber::{EnvFilter, fmt};
    // Default to this crate's own logs; rmcp logs at info are noisy on stderr.
    let filter = EnvFilter::try_from_env("FLAT_SNOWLUMA_LOG")
        .unwrap_or_else(|_| EnvFilter::new("flat_snowluma_mcp=info"));
    let _ = fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(filter)
        .with_target(false)
        .try_init();
}

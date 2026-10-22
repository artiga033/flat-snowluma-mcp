//! MCP server wiring.
//!
//! Exposes the catalog tools and the whole catalog as a resource, mirroring the
//! official docs server (`snowluma://onebot/actions`).

use std::sync::Arc;

use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler,
    model::{
        CallToolRequestParams, CallToolResponse, Implementation, ListResourcesResult,
        ListToolsResult, PaginatedRequestParams, ReadResourceRequestParams, ReadResourceResponse,
        ReadResourceResult, Resource, ResourceContents, ServerCapabilities, ServerConfig, Tool,
    },
    service::RequestContext,
};

use crate::catalog::Catalog;
use crate::tools::App;

/// The catalog resource URI, matching the official docs server.
pub const RESOURCE_URI: &str = "snowluma://onebot/actions";

/// The server description, plus the caveat that SnowLuma may
/// deviate from its own docs or be limited by the upstream QQ client.
pub const INSTRUCTIONS: &str = "SnowLuma QQ API。先检查 isError，再读 structuredContent；普通响应还需检查 status/retcode。接口行为可能受 QQ 客户端限制；异常请告知用户。";

/// The MCP server over the flat tool surface.
#[derive(Clone)]
pub struct SnowLumaServer {
    app: Arc<App>,
    catalog: &'static Catalog,
}

impl SnowLumaServer {
    pub fn new(app: App, catalog: &'static Catalog) -> SnowLumaServer {
        SnowLumaServer {
            app: Arc::new(app),
            catalog,
        }
    }
}

impl ServerHandler for SnowLumaServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
        .with_server_info(Implementation::new(
            "flat-snowluma-mcp",
            env!("CARGO_PKG_VERSION"),
        ))
        .with_instructions(INSTRUCTIONS)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult::with_all_items(self.app.tools().to_vec()))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        // Protocol errors are reserved for unroutable requests; every tool-level
        // failure is a `CallToolResult` with `isError: true` so the caller reads it.
        let result = self
            .app
            .call(&request.name, request.arguments, context.ct)
            .await;
        Ok(result.into())
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        let resource = Resource::new(RESOURCE_URI, "SnowLuma OneBot action catalog")
            .with_mime_type("application/json")
            .with_description(format!(
                "SnowLuma v{} 的 {} 个 OneBot action 完整目录（文档 + JSON Schema）。",
                self.catalog.version,
                self.catalog.len()
            ));
        Ok(ListResourcesResult::with_all_items(vec![resource]))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, McpError> {
        if request.uri != RESOURCE_URI {
            return Err(McpError::resource_not_found(
                format!("unknown resource: {}", request.uri),
                None,
            ));
        }
        let payload = serde_json::json!({
            "version": self.catalog.version,
            "hash": self.catalog.hash,
            "fetchedAt": self.catalog.fetched_at,
            "categories": self.catalog.categories,
            "actions": self.catalog.actions,
            "execution": self.catalog.execution,
        });
        let text = serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".to_string());
        let contents = ResourceContents::TextResourceContents {
            uri: request.uri,
            mime_type: Some("application/json".to_string()),
            text,
            meta: None,
        };
        Ok(ReadResourceResult::new(vec![contents]).into())
    }
}

/// Convenience for tests: the declared tools.
pub fn declared_tools(server: &SnowLumaServer) -> &[Tool] {
    server.app.tools()
}

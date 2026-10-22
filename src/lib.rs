//! Flat SnowLuma MCP: one callable, typed MCP tool per SnowLuma OneBot
//! action, exposed over standard MCP stdio.
//!
//! Modules cover the pinned catalog and schemas, parameter validation, OneBot
//! HTTP transport, stream transfers, and the MCP server/tool surface.

pub mod catalog;
pub mod config;
pub mod error;
pub mod names;
pub mod onebot;
pub mod patches;
pub mod schema;
pub mod server;
pub mod stream;
pub mod tools;
pub mod validate;

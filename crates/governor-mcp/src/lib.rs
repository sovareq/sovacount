//! `governor-mcp` — Model Context Protocol (MCP) frontend for Token Governor.
//!
//! Exposes a single MCP tool, `governor_classify`, over the rmcp 1.5 `server`
//! API. Stdio transport is the deployment target: an agent host (Claude Code,
//! Claude Desktop, Codex with MCP, Cursor, …) spawns the binary as a child
//! process and exchanges JSON-RPC messages over stdin/stdout.
//!
//! # Architecture
//!
//! * [`server::GovernorServer`] holds the rmcp tool-router and an
//!   [`Arc`][std::sync::Arc]-wrapped classifier behind the
//!   [`state::ClassifierLike`] trait. The trait abstraction lets tests inject
//!   a fake without requiring a working LLM provider.
//! * [`server::ClassifyParams`] is the flat tool-input shape the agent sends.
//!   It mirrors `governor_core::ClassifyRequest` 1:1 but lives here so its
//!   schema can be derived via [`schemars`] without leaking that dependency
//!   into the core crate.
//!
//! # Logging
//!
//! Stdio MCP transport reserves stdout for protocol bytes; any stray
//! `println!` or stdout-write corrupts the wire. The binary configures
//! `tracing-subscriber` to emit on **stderr** only.

#![forbid(unsafe_code)]

pub mod server;
pub mod state;

pub use server::{ClassifyParams, GovernorServer};
pub use state::{ClassifierLike, RealClassifier};

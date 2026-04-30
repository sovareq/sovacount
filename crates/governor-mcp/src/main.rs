//! `governor-mcp` — stdio MCP-server binary for the Token Governor classifier.
//!
//! Spawned by an MCP host (Claude Code, Claude Desktop, Codex with MCP, …)
//! as a child process. JSON-RPC framing is sent over stdin/stdout per the
//! MCP 2025-11-25 spec. Logs are written to **stderr only** so they do not
//! corrupt the protocol stream.

#![forbid(unsafe_code)]

use std::sync::Arc;

use anyhow::{Context, Result};
use governor_core::{Classifier, Config};
use governor_mcp::{GovernorServer, RealClassifier};
use rmcp::{ServiceExt, transport::io::stdio};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    tracing::info!("governor-mcp starting up");

    let cfg = Config::from_env().context("loading governor config from environment")?;
    let classifier = Classifier::new(cfg)
        .await
        .context("initialising classifier")?;
    let state = RealClassifier::new(Arc::new(classifier));
    let server = GovernorServer::new(Arc::new(state));

    tracing::info!("serving MCP over stdio");

    let (read, write) = stdio();
    let service = server
        .serve((read, write))
        .await
        .context("starting MCP service on stdio")?;
    service
        .waiting()
        .await
        .context("MCP service exited with error")?;

    tracing::info!("governor-mcp shutting down");
    Ok(())
}

/// Configure `tracing-subscriber` to write to **stderr only**.
///
/// Stdio MCP transport reserves stdout for protocol bytes — any stdout-write
/// (including a tracing subscriber's default `fmt` writer) corrupts the
/// JSON-RPC stream. Default level is `info`; override with `RUST_LOG`.
fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .try_init();
}

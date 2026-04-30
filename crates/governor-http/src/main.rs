//! `governor-http` binary entry-point.
//!
//! Loads [`Config`] from environment, builds a real
//! [`governor_core::Classifier`], wires the [`router`](governor_http::router)
//! and serves it via `axum`.
//!
//! Configuration precedence for the bind address:
//!     `--bind <ADDR>`  >  env `GOVERNOR_HTTP_BIND`  >  `127.0.0.1:8989`
//!
//! Optional Bearer auth via env `GOVERNOR_HTTP_API_KEY` (non-empty string).

#![forbid(unsafe_code)]

use anyhow::{Context, Result};
use clap::Parser;
use governor_core::{Classifier, Config};
use governor_http::{AppState, router};
use tracing_subscriber::{EnvFilter, fmt};

const DEFAULT_BIND: &str = "127.0.0.1:8989";

/// Command-line arguments for `governor-http`.
#[derive(Debug, Parser)]
#[command(
    name = "governor-http",
    about = "HTTP-server frontend for Token Governor.",
    version
)]
struct Args {
    /// Bind address. Falls back to `GOVERNOR_HTTP_BIND`, then to `127.0.0.1:8989`.
    ///
    /// Default is loopback-only by design — set explicitly to bind a public
    /// interface (e.g. `0.0.0.0:8989`) once you've decided on auth.
    #[arg(long, value_name = "ADDR")]
    bind: Option<String>,
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    fmt().with_env_filter(filter).init();
}

fn resolve_bind(arg: Option<String>) -> String {
    arg.or_else(|| std::env::var("GOVERNOR_HTTP_BIND").ok())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_BIND.to_string())
}

fn resolve_api_key() -> Option<String> {
    std::env::var("GOVERNOR_HTTP_API_KEY")
        .ok()
        .filter(|s| !s.is_empty())
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let args = Args::parse();
    let bind = resolve_bind(args.bind);
    let api_key = resolve_api_key();

    let cfg = Config::from_env().context("failed to load Config from environment")?;
    let provider = format!("{:?}", cfg.provider).to_lowercase();
    let classifier = Classifier::new(cfg)
        .await
        .context("failed to build classifier")?;

    let auth_label = if api_key.is_some() { "on" } else { "off" };
    let state = AppState::new(classifier, api_key);
    let app = router(state);

    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .with_context(|| format!("failed to bind {bind}"))?;
    let local = listener
        .local_addr()
        .with_context(|| format!("failed to read local_addr from listener bound to {bind}"))?;

    tracing::info!(
        "governor-http listening on http://{local} (auth={auth_label}, provider={provider})"
    );

    axum::serve(listener, app)
        .await
        .context("axum::serve failed")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_bind_arg_wins() {
        // `--bind` always takes precedence over env / default.
        let got = resolve_bind(Some("1.2.3.4:9".into()));
        assert_eq!(got, "1.2.3.4:9");
    }

    #[test]
    fn default_bind_is_loopback() {
        // Hard-coded loopback default — surfaces if anyone changes it
        // (binding to `0.0.0.0` would expose unauthenticated APIs by default).
        assert_eq!(DEFAULT_BIND, "127.0.0.1:8989");
    }
}

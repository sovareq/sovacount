//! `tier-classify` — Token Governor CLI frontend.
//!
//! Thin wrapper around [`governor_core::Classifier`]: parses scope from
//! one of `--task` / `--scope <FILE>` / `--stdin`, builds a
//! [`ClassifyRequest`], invokes the classifier, and prints the result in
//! one of four formats (`json` / `yaml` / `oneline` / `pretty`).
//!
//! Designed for shell-pipeline use:
//!
//! ```text
//! tier-classify --task "..." --format oneline   # → @so
//! ```

#![forbid(unsafe_code)]

use std::io::Read;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, ValueEnum};
use governor_core::config::ProviderKind;
use governor_core::{
    AlternativeTier, Classifier, ClassifyRequest, ClassifyResponse, Complexity, Config, Tier,
};

/// `tier-classify` — classify a task into the @op / @so / @hk LLM-cost tier.
#[derive(Debug, Parser)]
#[command(
    name = "tier-classify",
    version,
    about = "Classify a task scope into an @op/@so/@hk LLM-tier.",
    long_about = "tier-classify reads a scope description (inline, from a file, or from stdin), \
                  asks the Token Governor to choose the cheapest model that can do the work, \
                  and prints the result in JSON / YAML / one-line / pretty form."
)]
struct Cli {
    /// Inline scope text. Mutually exclusive with --scope and --stdin.
    #[arg(long, value_name = "TEXT", conflicts_with_all = ["scope", "stdin"])]
    task: Option<String>,

    /// Read scope from a markdown file. Mutually exclusive with --task and --stdin.
    #[arg(long, value_name = "FILE", conflicts_with_all = ["task", "stdin"])]
    scope: Option<PathBuf>,

    /// Read scope from stdin. Mutually exclusive with --task and --scope.
    #[arg(long, conflicts_with_all = ["task", "scope"])]
    stdin: bool,

    /// Comma-separated SSOT references (paths). Whitespace around commas is trimmed.
    #[arg(long, value_name = "COMMA_LIST", default_value = "")]
    ssot: String,

    /// Override task identifier. Defaults to `cli-<unix-timestamp>`.
    #[arg(long, value_name = "STR")]
    task_id: Option<String>,

    /// Caller's file-count estimate.
    #[arg(long, value_name = "N")]
    files_est: Option<u32>,

    /// Caller's LOC estimate.
    #[arg(long, value_name = "N")]
    loc_est: Option<u32>,

    /// Skip cache lookup for this request.
    #[arg(long)]
    no_cache: bool,

    /// Override `GOVERNOR_PROVIDER`.
    #[arg(long, value_name = "KIND", value_enum)]
    provider: Option<ProviderArg>,

    /// Output format.
    #[arg(long, value_name = "FMT", value_enum, default_value_t = Format::Json)]
    format: Format,

    /// Verbosity. `-v` = info, `-vv` = debug.
    #[arg(short = 'v', long = "verbose", action = clap::ArgAction::Count)]
    verbose: u8,
}

/// Provider override choice. Mirrors [`governor_core::config::ProviderKind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "lowercase")]
enum ProviderArg {
    Anthropic,
    Openai,
    Ollama,
    Mock,
    Custom,
}

impl From<ProviderArg> for ProviderKind {
    fn from(v: ProviderArg) -> Self {
        match v {
            ProviderArg::Anthropic => ProviderKind::Anthropic,
            ProviderArg::Openai => ProviderKind::OpenAi,
            ProviderArg::Ollama => ProviderKind::Ollama,
            ProviderArg::Mock => ProviderKind::Mock,
            ProviderArg::Custom => ProviderKind::Custom,
        }
    }
}

/// Output format choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "lowercase")]
enum Format {
    /// Compact JSON (default).
    Json,
    /// Minimal YAML, manually serialized for `ClassifyResponse`.
    Yaml,
    /// Just the tier tag, no surrounding chars (e.g. `@so`).
    Oneline,
    /// Pretty JSON with a one-line human header.
    Pretty,
}

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        // Clean error output on stderr — no panic-style backtrace.
        eprintln!("tier-classify: {e:#}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let cli = Cli::parse();
    init_tracing(cli.verbose);

    if cli.task.is_none() && cli.scope.is_none() && !cli.stdin {
        bail!("exactly one of --task / --scope / --stdin is required");
    }

    let scope_md = read_scope(&cli)?;
    let ssot_refs = parse_ssot(&cli.ssot);
    let task_id = cli.task_id.clone().unwrap_or_else(default_task_id);

    let mut cfg = Config::from_env().context("loading governor configuration from environment")?;
    if let Some(p) = cli.provider {
        cfg.provider = p.into();
    }

    let classifier = Classifier::new(cfg)
        .await
        .context("constructing classifier")?;

    let req = ClassifyRequest {
        task_id,
        scope_md,
        ssot_refs,
        estimated_loc: cli.loc_est,
        estimated_files: cli.files_est,
        no_cache: cli.no_cache,
    };

    let resp = classifier
        .classify(req)
        .await
        .context("classifying request")?;

    let out = format_response(&resp, cli.format)?;
    print!("{out}");
    Ok(())
}

fn init_tracing(verbosity: u8) {
    use tracing_subscriber::{EnvFilter, fmt};

    // RUST_LOG wins; verbosity is the fallback default.
    let default_level = match verbosity {
        0 => "warn",
        1 => "info",
        _ => "debug",
    };
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_level));

    // ignore double-init in tests
    let _ = fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .try_init();
}

fn read_scope(cli: &Cli) -> Result<String> {
    if let Some(t) = &cli.task {
        return Ok(t.clone());
    }
    if let Some(p) = &cli.scope {
        return std::fs::read_to_string(p)
            .with_context(|| format!("reading scope file {}", p.display()));
    }
    if cli.stdin {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("reading scope from stdin")?;
        return Ok(buf);
    }
    Err(anyhow!("no scope source supplied"))
}

fn parse_ssot(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

fn default_task_id() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("cli-{secs}")
}

fn format_response(resp: &ClassifyResponse, fmt: Format) -> Result<String> {
    Ok(match fmt {
        Format::Json => {
            let mut s = serde_json::to_string(resp).context("serialising response to JSON")?;
            s.push('\n');
            s
        }
        Format::Pretty => {
            let body =
                serde_json::to_string_pretty(resp).context("serialising response to JSON")?;
            format!(
                "# tier={tier} confidence={conf}\n{body}\n",
                tier = resp.tier,
                conf = resp.confidence,
            )
        }
        Format::Oneline => format!("{}\n", resp.tier),
        Format::Yaml => format_yaml(resp),
    })
}

/// Minimal hand-rolled YAML emitter for [`ClassifyResponse`].
///
/// Scope is intentionally narrow: handles the exact field types of
/// `ClassifyResponse` (`Tier`, `Complexity`, `String`, numeric primitives,
/// `Option<String>`, and `Vec<AlternativeTier>`). Avoids the `serde_yaml`
/// dependency (which is not in our workspace allowlist).
fn format_yaml(r: &ClassifyResponse) -> String {
    let mut out = String::new();
    out.push_str(&format!("tier: {}\n", yaml_tier(r.tier)));
    match &r.model_hint {
        Some(m) => out.push_str(&format!("model_hint: {}\n", yaml_string(m))),
        None => out.push_str("model_hint: null\n"),
    }
    out.push_str(&format!("complexity: {}\n", yaml_complexity(r.complexity)));
    out.push_str(&format!("rationale: {}\n", yaml_string(&r.rationale)));
    out.push_str(&format!("confidence: {}\n", r.confidence));
    out.push_str(&format!(
        "estimated_input_tokens: {}\n",
        r.estimated_input_tokens
    ));
    out.push_str(&format!(
        "estimated_output_tokens: {}\n",
        r.estimated_output_tokens
    ));
    out.push_str(&format!(
        "estimated_cost_usd: {}\n",
        format_f64(r.estimated_cost_usd)
    ));
    if r.alternative_tiers.is_empty() {
        out.push_str("alternative_tiers: []\n");
    } else {
        out.push_str("alternative_tiers:\n");
        for alt in &r.alternative_tiers {
            out.push_str(&yaml_alt(alt));
        }
    }
    out.push_str(&format!("from_cache: {}\n", r.from_cache));
    out
}

fn yaml_alt(alt: &AlternativeTier) -> String {
    let mut s = String::new();
    s.push_str(&format!("  - tier: {}\n", yaml_tier(alt.tier)));
    s.push_str(&format!("    rationale: {}\n", yaml_string(&alt.rationale)));
    s.push_str(&format!(
        "    extra_cost_usd: {}\n",
        format_f64(alt.extra_cost_usd)
    ));
    s
}

fn yaml_tier(t: Tier) -> &'static str {
    match t {
        Tier::Op => "op",
        Tier::So => "so",
        Tier::Hk => "hk",
    }
}

fn yaml_complexity(c: Complexity) -> &'static str {
    match c {
        Complexity::Trivial => "trivial",
        Complexity::Standard => "standard",
        Complexity::Complex => "complex",
    }
}

/// Always emit YAML scalars as JSON-style double-quoted strings — robust
/// against colons, hashes, leading dashes, and embedded newlines without
/// having to reason about YAML plain-scalar edge cases.
fn yaml_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn format_f64(v: f64) -> String {
    if v.is_nan() {
        ".nan".into()
    } else if v.is_infinite() {
        if v.is_sign_negative() {
            "-.inf".into()
        } else {
            ".inf".into()
        }
    } else {
        // Match serde_json's float rendering reasonably; force a decimal point.
        let s = format!("{v}");
        if s.contains('.') || s.contains('e') || s.contains('E') {
            s
        } else {
            format!("{s}.0")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn fixture() -> ClassifyResponse {
        ClassifyResponse {
            tier: Tier::So,
            model_hint: Some("claude-sonnet-4-6".into()),
            complexity: Complexity::Standard,
            rationale: "Single-file fix on a known pattern.".into(),
            confidence: 87,
            estimated_input_tokens: 1200,
            estimated_output_tokens: 350,
            estimated_cost_usd: 0.0123,
            alternative_tiers: vec![AlternativeTier {
                tier: Tier::Hk,
                rationale: "Borderline triviality.".into(),
                extra_cost_usd: -0.01,
            }],
            from_cache: false,
        }
    }

    // ---- arg parsing ----

    #[test]
    fn requires_a_scope_source_at_runtime() {
        // clap parsing succeeds (none of the three flags is `required`),
        // but `run()` rejects the no-source case. We assert clap parses an
        // empty argv successfully.
        let cli = Cli::try_parse_from(["tier-classify"]).expect("parse");
        assert!(cli.task.is_none());
        assert!(cli.scope.is_none());
        assert!(!cli.stdin);
    }

    #[test]
    fn task_and_scope_are_mutually_exclusive() {
        let err =
            Cli::try_parse_from(["tier-classify", "--task", "x", "--scope", "y.md"]).unwrap_err();
        assert!(
            err.to_string().contains("cannot be used") || err.to_string().contains("conflict"),
            "expected conflict error, got: {err}"
        );
    }

    #[test]
    fn task_and_stdin_are_mutually_exclusive() {
        assert!(Cli::try_parse_from(["tier-classify", "--task", "x", "--stdin"]).is_err());
    }

    #[test]
    fn parses_full_argv() {
        let cli = Cli::try_parse_from([
            "tier-classify",
            "--task",
            "Fix path bug",
            "--ssot",
            "a.md, b.md ,c.md",
            "--task-id",
            "T-X-1",
            "--files-est",
            "5",
            "--loc-est",
            "350",
            "--no-cache",
            "--provider",
            "mock",
            "--format",
            "oneline",
            "-vv",
        ])
        .expect("parse");
        assert_eq!(cli.task.as_deref(), Some("Fix path bug"));
        assert_eq!(cli.ssot, "a.md, b.md ,c.md");
        assert_eq!(cli.task_id.as_deref(), Some("T-X-1"));
        assert_eq!(cli.files_est, Some(5));
        assert_eq!(cli.loc_est, Some(350));
        assert!(cli.no_cache);
        assert_eq!(cli.provider, Some(ProviderArg::Mock));
        assert_eq!(cli.format, Format::Oneline);
        assert_eq!(cli.verbose, 2);
    }

    #[test]
    fn provider_enum_maps_to_core() {
        assert_eq!(
            ProviderKind::from(ProviderArg::Anthropic),
            ProviderKind::Anthropic
        );
        assert_eq!(
            ProviderKind::from(ProviderArg::Openai),
            ProviderKind::OpenAi
        );
        assert_eq!(
            ProviderKind::from(ProviderArg::Ollama),
            ProviderKind::Ollama
        );
        assert_eq!(ProviderKind::from(ProviderArg::Mock), ProviderKind::Mock);
        assert_eq!(
            ProviderKind::from(ProviderArg::Custom),
            ProviderKind::Custom
        );
    }

    // ---- pure helpers ----

    #[test]
    fn ssot_split_trims_and_drops_empties() {
        assert!(parse_ssot("").is_empty());
        assert!(parse_ssot(",,").is_empty());
        assert_eq!(
            parse_ssot("a.md, b.md ,c.md"),
            vec!["a.md".to_string(), "b.md".into(), "c.md".into()]
        );
        assert_eq!(parse_ssot("  one  "), vec!["one".to_string()]);
    }

    #[test]
    fn default_task_id_is_unix_timestamped() {
        let id = default_task_id();
        assert!(id.starts_with("cli-"), "got: {id}");
        let n: u64 = id.trim_start_matches("cli-").parse().expect("numeric");
        // sanity: between 2020-01-01 and year 9999
        assert!(n > 1_577_836_800, "{n}");
    }

    // ---- formatters ----

    #[test]
    fn oneline_emits_only_the_tag() {
        let s = format_response(&fixture(), Format::Oneline).unwrap();
        assert_eq!(s, "@so\n");
    }

    #[test]
    fn json_format_is_compact_and_parses_back() {
        let s = format_response(&fixture(), Format::Json).unwrap();
        assert!(s.ends_with('\n'));
        // Compact: no trailing commas, no two-space indent.
        assert!(!s.contains("  \"tier\""));
        let trimmed = s.trim_end();
        let v: serde_json::Value = serde_json::from_str(trimmed).expect("valid JSON");
        assert_eq!(v["tier"], serde_json::json!("so"));
        assert_eq!(v["confidence"], serde_json::json!(87));
        assert_eq!(v["model_hint"], serde_json::json!("claude-sonnet-4-6"));
    }

    #[test]
    fn pretty_format_has_human_header_and_pretty_body() {
        let s = format_response(&fixture(), Format::Pretty).unwrap();
        assert!(s.starts_with("# tier=@so confidence=87\n"), "got: {s}");
        // Pretty body starts with `{` after the header newline.
        let body_start = s.find('{').expect("has body");
        let body = &s[body_start..];
        let v: serde_json::Value = serde_json::from_str(body.trim_end()).expect("valid JSON");
        assert_eq!(v["tier"], serde_json::json!("so"));
        // Pretty has indented lines.
        assert!(
            body.contains("\n  \""),
            "expected indented body, got: {body}"
        );
    }

    #[test]
    fn yaml_format_renders_known_keys() {
        let s = format_response(&fixture(), Format::Yaml).unwrap();
        assert!(s.contains("tier: so\n"), "got: {s}");
        assert!(s.contains("complexity: standard\n"), "got: {s}");
        assert!(s.contains("confidence: 87\n"), "got: {s}");
        assert!(
            s.contains("model_hint: \"claude-sonnet-4-6\"\n"),
            "got: {s}"
        );
        assert!(
            s.contains("rationale: \"Single-file fix on a known pattern.\"\n"),
            "got: {s}"
        );
        assert!(s.contains("alternative_tiers:\n  - tier: hk\n"), "got: {s}");
        assert!(s.contains("from_cache: false\n"), "got: {s}");
    }

    #[test]
    fn yaml_handles_empty_alternatives_inline() {
        let mut r = fixture();
        r.alternative_tiers.clear();
        let s = format_yaml(&r);
        assert!(s.contains("alternative_tiers: []\n"));
    }

    #[test]
    fn yaml_handles_missing_model_hint() {
        let mut r = fixture();
        r.model_hint = None;
        let s = format_yaml(&r);
        assert!(s.contains("model_hint: null\n"));
    }

    #[test]
    fn yaml_string_escapes_special_chars() {
        assert_eq!(yaml_string("a\"b"), "\"a\\\"b\"");
        assert_eq!(yaml_string("line1\nline2"), "\"line1\\nline2\"");
        assert_eq!(yaml_string("tab\there"), "\"tab\\there\"");
        assert_eq!(yaml_string("back\\slash"), "\"back\\\\slash\"");
    }

    #[test]
    fn format_f64_keeps_decimal_point() {
        assert_eq!(format_f64(1.0), "1.0");
        assert_eq!(format_f64(0.0), "0.0");
        assert_eq!(format_f64(-0.5), "-0.5");
        assert!(format_f64(f64::NAN) == ".nan");
        assert_eq!(format_f64(f64::INFINITY), ".inf");
        assert_eq!(format_f64(f64::NEG_INFINITY), "-.inf");
    }
}

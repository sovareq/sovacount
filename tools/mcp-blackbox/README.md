# MCP black-box test harness

External, language-agnostic sanity check for `governor-mcp`. **Not** part of
CI — that's covered by [`crates/governor-mcp/tests/integration.rs`]. This
Python harness exists so the MCP wire-protocol is validated by something
that does **not** share types or assumptions with the Rust implementation.

## Why a separate harness?

Testing Rust code with Rust runs the risk of confirming your own assumptions
rather than the protocol. A Python client speaking raw JSON-RPC over stdio
catches drift the in-language tests can't see — wire-format quirks, framing
edge cases, and rmcp transport behavior.

## Run

```bash
# Build the binary first
cargo build -p governor-mcp

# Run the harness
python3 tools/mcp-blackbox/blackbox.py

# Override the binary path (e.g. release build)
GOVERNOR_MCP_BIN=target/release/governor-mcp \
  python3 tools/mcp-blackbox/blackbox.py
```

20 tests covering:

1. **Protocol conformance** (7) — initialize, tools/list, tools/call,
   unknown method, missing field, wrong type, unknown tool
2. **Stdio transport** (3) — malformed JSON, concurrent ids, notifications
3. **Tool semantics** (5) — large payload, unicode, prompt injection,
   no_cache, structured-content mirroring
4. **Security / hardening** (5) — env-var exfiltration, oversized payload,
   mid-message disconnect, BOM prefix, sequential robustness

Exit code is `0` if all pass, `1` otherwise. Output is colorized for
human readability.

## Requirements

- Python 3.9+ (uses `dict[str, str]`-style generics)
- No external Python deps — stdlib only

## When to run

- Before cutting a release.
- After bumping the `rmcp` dependency.
- When you change anything in `crates/governor-mcp/src/`.
- As part of any quarterly upstream-protocol audit.

If a new MCP spec version drops, update `PROTOCOL_VERSION` at the top of
`blackbox.py` and re-validate.

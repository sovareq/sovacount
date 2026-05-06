#!/usr/bin/env bash
# sovacount-dual.sh — start twee governor-http instanties naast elkaar.
# Instantie A: Anthropic (voor Claude Code / MCP)   → :8989
# Instantie B: OpenAI-compatible (voor GPT Codex)   → :8990
#
# Vereisten: governor-http binary in PATH of via cargo run.
# API-keys via env: ANTHROPIC_API_KEY, OPENAI_API_KEY
set -euo pipefail

GOVERNOR_BIN="${GOVERNOR_BIN:-governor-http}"

echo "[dual] Starting Anthropic instance on :8989 ..."
GOVERNOR_PROVIDER=anthropic \
GOVERNOR_API_KEY="${ANTHROPIC_API_KEY:?ANTHROPIC_API_KEY not set}" \
GOVERNOR_HTTP_BIND=127.0.0.1:8989 \
GOVERNOR_CACHE_DIR="${XDG_CACHE_HOME:-$HOME/.cache}/token-governor/anthropic" \
"${GOVERNOR_BIN}" &
PID_A=$!

echo "[dual] Starting OpenAI instance on :8990 ..."
GOVERNOR_PROVIDER=openai \
GOVERNOR_API_KEY="${OPENAI_API_KEY:?OPENAI_API_KEY not set}" \
GOVERNOR_HTTP_BIND=127.0.0.1:8990 \
GOVERNOR_CACHE_DIR="${XDG_CACHE_HOME:-$HOME/.cache}/token-governor/openai" \
"${GOVERNOR_BIN}" &
PID_B=$!

echo "[dual] Both instances running. PID_A=$PID_A PID_B=$PID_B"
echo "[dual] Ctrl-C to stop both."

trap "kill $PID_A $PID_B 2>/dev/null; echo '[dual] Stopped.'" EXIT INT TERM
wait

#!/usr/bin/env bash
#
# governor-hook.sh — Claude Code PreToolUse hook for Token Governor.
#
# Reads the tool input from stdin (Claude Code passes a JSON envelope on
# stdin to user-script hooks), extracts the user's task description, asks
# the local governor-http what tier the work should run on, and prints
# the recommendation to stderr so the user sees it without altering the
# tool call.
#
# Exit code 0 = "let the tool run" (light-mode default — log only).
# Set GOVERNOR_HOOK_MODE=strict to instead exit non-zero on @op tier so
# Claude Code will block the call until the user confirms.
#
# Mode summary
# ------------
#   light  (default) — log decision to stderr, never block.
#   strict           — block @op tier (cost-shock guard).
#   silent           — no stderr output, never block (use only in CI).
#
# Wiring
# ------
# 1. Make this script executable: `chmod +x governor-hook.sh`
# 2. Copy or symlink to ~/.claude/hooks/ (or .claude/hooks/ in this project).
# 3. Register in ~/.claude/settings.json (or .claude/settings.json):
#    {
#      "hooks": {
#        "PreToolUse": [
#          {"matcher": "Edit|Write|Bash", "hooks": [{"type": "command", "command": "/path/to/governor-hook.sh"}]}
#        ]
#      }
#    }
# 4. Make sure governor-http is running: `cargo run -p governor-http`
#
# Requires: bash, jq, curl. No further deps.

set -euo pipefail

GOVERNOR_URL="${GOVERNOR_URL:-http://127.0.0.1:8989/classify}"
MODE="${GOVERNOR_HOOK_MODE:-light}"

# ---- Read Claude Code envelope from stdin -------------------------------
# Claude Code hooks receive a JSON object with tool name + input. We only
# need a free-text description of the task — fall back to the raw envelope
# string if no obvious field is present.
ENVELOPE="$(cat)"
TASK="$(echo "$ENVELOPE" | jq -r '
    .tool_input.description
    // .tool_input.command
    // .tool_input.file_path
    // .tool_input.prompt
    // (.tool_input | tostring)
' 2>/dev/null || echo "$ENVELOPE")"

if [[ -z "$TASK" || "$TASK" == "null" ]]; then
  # No task to classify — let the call through silently.
  exit 0
fi

# Truncate to a reasonable length for the classifier — it doesn't need the
# full file content, just a summary of intent.
TASK="${TASK:0:4000}"

# ---- Call governor-http -------------------------------------------------
RESP="$(curl -s --max-time 5 -X POST "$GOVERNOR_URL" \
  -H 'Content-Type: application/json' \
  -d "$(jq -nc --arg task "$TASK" '{task_id:"claude-code-hook", scope_md:$task}')" \
  || echo '')"

if [[ -z "$RESP" ]]; then
  # Governor unreachable — fail open (don't block on infrastructure issues).
  if [[ "$MODE" != "silent" ]]; then
    echo "[governor-hook] WARN: governor-http unreachable at $GOVERNOR_URL — passing through" >&2
  fi
  exit 0
fi

TIER="$(echo "$RESP" | jq -r .tier 2>/dev/null || echo unknown)"
MODEL="$(echo "$RESP" | jq -r '.model_hint // "(unmapped)"')"
COST="$(echo "$RESP" | jq -r '.estimated_cost_usd // 0')"
RATIONALE="$(echo "$RESP" | jq -r '.rationale // ""')"

# ---- Apply mode ---------------------------------------------------------
case "$MODE" in
  silent)
    exit 0
    ;;

  strict)
    if [[ "$TIER" == "op" ]]; then
      printf '\n\033[31m[governor-hook] BLOCK: tier=@op model=%s ~$%s\n  %s\n  set GOVERNOR_HOOK_MODE=light to override\033[0m\n\n' \
        "$MODEL" "$COST" "$RATIONALE" >&2
      exit 1
    fi
    ;& # fall through to logging

  light|*)
    printf '\033[36m[governor-hook] @%s %s ~$%s\033[0m\n' "$TIER" "$MODEL" "$COST" >&2
    ;;
esac

exit 0

#!/usr/bin/env bash
#
# install.sh — install governor-hook.sh into ~/.claude/hooks/ and patch
# ~/.claude/settings.json to register it as a PreToolUse hook.
#
# Idempotent: re-running won't duplicate the hook entry.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
HOOK_SRC="$SCRIPT_DIR/governor-hook.sh"
HOOK_DEST="$HOME/.claude/hooks/governor-hook.sh"
SETTINGS="$HOME/.claude/settings.json"

if [[ ! -x "$HOOK_SRC" ]]; then
  echo "error: $HOOK_SRC not found or not executable" >&2
  exit 1
fi

mkdir -p "$(dirname "$HOOK_DEST")"
cp "$HOOK_SRC" "$HOOK_DEST"
chmod +x "$HOOK_DEST"
echo "✓ copied hook to $HOOK_DEST"

# Patch settings.json — create if absent, merge if present
if [[ ! -f "$SETTINGS" ]]; then
  cat > "$SETTINGS" <<EOF
{
  "hooks": {
    "PreToolUse": [
      {"matcher": "Edit|Write|Bash", "hooks": [{"type": "command", "command": "$HOOK_DEST"}]}
    ]
  }
}
EOF
  echo "✓ created $SETTINGS"
else
  # Use jq to merge — only add if not already present
  if jq -e --arg path "$HOOK_DEST" '
      .hooks.PreToolUse // [] | map(.hooks // []) | flatten | map(.command) | index($path)
    ' "$SETTINGS" >/dev/null 2>&1; then
    echo "✓ hook already registered in $SETTINGS"
  else
    tmp="$(mktemp)"
    jq --arg path "$HOOK_DEST" '
      .hooks.PreToolUse = ((.hooks.PreToolUse // []) + [
        {"matcher": "Edit|Write|Bash", "hooks": [{"type": "command", "command": $path}]}
      ])
    ' "$SETTINGS" > "$tmp" && mv "$tmp" "$SETTINGS"
    echo "✓ added hook to existing $SETTINGS"
  fi
fi

cat <<EOF

Done. Restart Claude Code so it picks up the new hook.

Test:
  GOVERNOR_HOOK_MODE=strict claude  # blocks @op tasks until override
  GOVERNOR_HOOK_MODE=light  claude  # logs decisions, never blocks (default)
  GOVERNOR_HOOK_MODE=silent claude  # no output (CI / unattended)

Make sure governor-http is running:
  cargo run -p governor-http  # listens on 127.0.0.1:8989

Override the URL if needed:
  GOVERNOR_URL=http://localhost:9999/classify claude
EOF

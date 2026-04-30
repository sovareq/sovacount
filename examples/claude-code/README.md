# Claude Code PreToolUse hook

Working hook that calls `governor-http` before every `Edit` / `Write` /
`Bash` tool invocation and logs the recommended tier + cost. Three modes
match the [three-modes.md](../three-modes.md) policy:

| Mode | Behavior |
|---|---|
| `light` (default) | Log decision to stderr, never block. |
| `strict` | Block `@op` tier (cost-shock guard); user can re-issue with override. |
| `silent` | No output, never block. For CI / unattended workers. |

## Install

```bash
# 1. Make sure governor-http is running
cargo run -p governor-http  # listens on 127.0.0.1:8989

# 2. Install the hook
./examples/claude-code/install.sh
```

The installer is idempotent — re-running won't duplicate the entry in
`~/.claude/settings.json`.

## Use

```bash
# Default (light mode)
claude

# Strict mode — blocks @op tasks
GOVERNOR_HOOK_MODE=strict claude

# Silent mode
GOVERNOR_HOOK_MODE=silent claude

# Custom governor-http URL
GOVERNOR_URL=http://localhost:9999/classify claude
```

## What you'll see

In `light` mode, every `Edit`/`Write`/`Bash` call gets a one-liner on stderr:

```
[governor-hook] @hk claude-haiku-4-5 ~$0.0028
```

In `strict` mode, an `@op` recommendation prints in red and the call is
blocked:

```
[governor-hook] BLOCK: tier=@op model=claude-opus-4-7 ~$0.4425
  Routed to Opus tier because scope exceeds 300 LOC and touches more than 5 files…
  set GOVERNOR_HOOK_MODE=light to override
```

## Fail-open by design

If `governor-http` is unreachable, the hook **does not block**. It logs a
warning (or stays silent in `silent` mode) and lets the tool call through.
A misconfigured / down classifier should never break your editor session.

## Requirements

- `bash` 4+
- `jq`
- `curl`
- A running `governor-http` instance (cargo run -p governor-http or
  systemd / launchd unit)

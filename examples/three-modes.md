# Three execution modes

Token Governor is **recommend-only** by design — it tells you which tier
to use, but never executes the next step. That separation is intentional:
it lets you wire the recommendation into your agent at whatever level of
human oversight your team needs.

This page documents the three established patterns and shows how to wire
each one into Claude Code, Codex, or a custom agent. The companion
[`router.py`](router.py) is a reference implementation you can read,
copy, or fork.

| Mode | Speed | Cost-shock risk | Best for |
|---|---|---|---|
| **strict** | slow (per-task prompt) | none (you approve every call) | new codebases, juniors, financial / legal scope |
| **light** | fast (auto-route + log) | low (auditable post-hoc) | senior engineers, normal day-to-day work |
| **auto** | fastest (silent) | medium (cap at host level) | CI batch-jobs, sandboxed agents with budget limits |

## Mode 1 — `strict`: ask before every routing decision

```
$ python3 examples/router.py "Refactor the auth module" --loc 800 --files 12
[strict] → tier=@op model=claude-opus-4-7 complexity=complex confidence=88% ~$0.4425
  Routed to Opus tier because scope exceeds 300 LOC and touches more than 5 files…
proceed? [y/N] y
{"tier": "op", "model": "claude-opus-4-7", "task": "...", "estimated_cost_usd": 0.4425}
```

Wire-up:

* **Claude Code** — call `governor_classify` via the MCP tool, read the
  response, then prompt the user. The user manually executes `/model
  claude-opus-4-7` if they approve. No automation needed; this is the
  default behavior with the MCP integration.
* **Codex** — set the router's `pre-call` hook to require user
  confirmation. TokenMix.ai exposes `--require-approval` for this. Morph
  Router's equivalent is the `human_in_the_loop: true` policy.
* **Custom agent** — `router.py --mode strict` is the canonical pattern.
  Pipe its stdout into your model dispatch.

## Mode 2 — `light`: auto-route, log, allow cancel

```
$ python3 examples/router.py "Add audit endpoint" --loc 150 --files 3 --mode light
[light] → tier=@so model=claude-sonnet-4-6 complexity=standard confidence=85% ~$0.0330
  New endpoint following standard pagination/cursor pattern…
  auto-routing in 2.0s — Ctrl-C to cancel
{"tier": "so", "model": "claude-sonnet-4-6", "task": "...", "estimated_cost_usd": 0.033}
```

Wire-up:

* **Claude Code** — implement a [PreToolUse hook](https://docs.claude.com/en/docs/claude-code/hooks)
  that calls `governor_classify`, prints the decision, and proceeds
  unless the user types `/cancel` within the cancel window. The hook
  has read-only access to tool input but can short-circuit.
* **Codex** — wire the router's logging webhook to your team's Slack so
  the cost-trail is visible. Set `auto_route: true` and a per-day
  budget cap (`max_daily_cost_usd: 25`) as a safety net.
* **Custom agent** — `router.py --mode light --cancel-window 5`. The
  `2-5s` window gives you time to glance and Ctrl-C if the cost looks
  off without slowing routine work.

## Mode 3 — `auto`: silent routing

```
$ python3 examples/router.py "Fix typo" --loc 3 --files 1 --mode auto
[auto] @hk claude-haiku-4-5 ~$0.0028   ← stderr, one line per task
{"tier": "hk", "model": "claude-haiku-4-5", "task": "...", "estimated_cost_usd": 0.0028}
```

Wire-up:

* **Claude Code** — *not recommended* outside trusted sandboxes. Even
  on `--dangerously-skip-permissions`, you typically want at least the
  light-mode log line so you can see what happened.
* **Codex** — fine for batch-job workers where the budget is
  pre-allocated and there's no interactive user. Ensure the worker
  reports its decisions to your audit log.
* **Custom agent** — CI / cron agents: `router.py --mode auto` keeps
  output to one-line stderr per task, perfect for log-harvesting.
  **Always pair with a daily / weekly budget cap.**

## Choosing a default

Sovareq's recommendation: **light** for interactive agents, **auto**
for scheduled / unattended workers. Keep **strict** as an opt-in for
high-risk scope (auth, billing, legal).

If you're unsure, start strict and downgrade once the team has six
weeks of confidence in the tier-recommendations. The cost of one
misrouted Opus call is low; the cost of an unaudited rogue agent is
high.

## Hooking into Claude Code (HITL-light recipe)

```bash
# .claude/hooks/pre-tool.sh
#!/usr/bin/env bash
set -euo pipefail

if [[ "${TOOL_NAME:-}" != "Edit" && "${TOOL_NAME:-}" != "Write" ]]; then
  exit 0
fi

decision=$(curl -s -X POST http://127.0.0.1:8989/classify \
  -H 'Content-Type: application/json' \
  -d "$(jq -nc --arg task "$TOOL_INPUT" '{task_id:"hook", scope_md:$task}')")

tier=$(echo "$decision" | jq -r .tier)
cost=$(echo "$decision" | jq -r .estimated_cost_usd)
echo "[governor] tier=@$tier ~\$$cost" >&2

# Light mode: log only, never block
exit 0
```

Register it in `.claude/settings.json`:

```json
{
  "hooks": {
    "PreToolUse": ".claude/hooks/pre-tool.sh"
  }
}
```

## Hooking into Codex (HITL-strict recipe)

For the OpenAI Codex / openai-router family, drop a `governor.toml`
into your router config:

```toml
[router.governor]
endpoint = "http://127.0.0.1:8989/classify"
require_approval_for = ["op"]  # only Opus needs approval; SO/HK auto-route

[router.governor.budgets]
daily_max_usd = 25
per_task_max_usd = 1.50
```

Implementation details vary per router — check your router's
`pre-call middleware` extension point. The contract is the same:
POST the task to `/classify`, read `tier` + `estimated_cost_usd`,
decide.

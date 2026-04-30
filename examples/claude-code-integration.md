# Token Governor + Claude Code

Use `governor_classify` from inside Claude Code / Claude Desktop to ask the
governor what tier each task should run on, then route or annotate accordingly.

## Setup

1. Install the binary:

   ```bash
   cargo install --path crates/governor-mcp
   # or download a prebuilt binary from GitHub releases
   ```

2. Add the server to your MCP config. On macOS the file is
   `~/Library/Application Support/Claude/claude_desktop_config.json`:

   ```json
   {
     "mcpServers": {
       "token-governor": {
         "command": "governor-mcp",
         "env": {
           "GOVERNOR_PROVIDER": "anthropic",
           "GOVERNOR_API_KEY": "sk-ant-..."
         }
       }
     }
   }
   ```

   For Claude Code, the equivalent file is `~/.config/claude/mcp.json` (or
   whatever your CLI version expects — see `claude --help`).

3. Restart your client. You should see a `governor_classify` tool listed
   alongside the built-ins.

## Use

Ask Claude something like:

> *"Before you start, ask `governor_classify` whether this task should be
> Opus or Sonnet. Tell me what tier it picked and why."*

A typical exchange:

```
Tool call: governor_classify
  task_id: "T-201"
  scope_md: "Add audit-mirror endpoint to the existing API. ~150 LOC, ≤3 files."
  estimated_loc: 150
  estimated_files: 3

Tool result:
{
  "tier": "so",
  "model_hint": "claude-sonnet-4-6",
  "complexity": "standard",
  "rationale": "Standard implementation on a known endpoint pattern. <300 LOC.",
  "confidence": 87,
  ...
}
```

## Tips

- **Short-circuit obvious tasks.** For one-line typo-fixes, the heuristic
  fast-path returns `@hk` without an LLM call — fast and ~free.
- **Pre-classify a queue of tranches.** Before starting a multi-PR session,
  feed the queue through `governor-cli` and stash the tier per task in your
  notes. Saves round-trips.
- **Cache TTL.** By default the same `(task_id, scope_md, …)` returns from
  cache for 30 days. Pass `no_cache: true` to force re-classification.
- **Custom prompt.** If your team has a stricter rubric (e.g. always-Opus for
  anything touching `auth/`), set `GOVERNOR_CLASSIFIER_PROMPT_FILE` to a
  modified copy of `crates/governor-core/src/prompts/classifier.md`.

## Troubleshooting

- **Tool not appearing**: `governor-mcp` writes logs to stderr, not stdout
  (stdout is reserved for MCP protocol). Run it standalone first to confirm
  it starts: `governor-mcp` should print one `info` line on stderr and then
  block waiting for protocol bytes.
- **`provider 'anthropic' requires GOVERNOR_API_KEY`**: the env block in the
  MCP config did not propagate. Some hosts require absolute paths in
  `command` and an explicit `env` map, others read your shell's exported
  vars. Double-check by adding `GOVERNOR_PROVIDER=mock` first to confirm
  wiring before plugging in real keys.

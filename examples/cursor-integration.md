# Token Governor + Cursor

Cursor supports MCP servers natively. Wiring it up is the same shape as
Claude Code — Cursor just looks in a different config file.

## Setup

1. Install the binary:

   ```bash
   cargo install --path crates/governor-mcp
   ```

2. Open Cursor → **Settings → MCP → Edit Config**. Add:

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

3. Hit "Refresh" in the MCP panel. You should see `token-governor` listed
   with a green dot and one tool: `governor_classify`.

## Use

When you're chatting with Cursor's agent and it suggests a non-trivial change,
you can interject:

> *"Before doing that — run governor_classify and tell me which tier this
> falls into. If it's @op, let's split it first."*

Cursor will pick the tool up automatically and surface the JSON result in
the conversation.

## Tip — auto-split on `@op`

Add a custom rule in **Cursor → Rules**:

```
Before any task whose scope description mentions "rewrite", "migrate",
"architecture" or has more than 300 LOC, call governor_classify. If the
returned tier is @op, propose splitting the task into 2-3 smaller tranches
and ask me to confirm before continuing.
```

This effectively gives Cursor a fan-out reflex paid for by a single cheap
classifier call.

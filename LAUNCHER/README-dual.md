# Dual-instance setup (Claude Code + GPT Codex)

## Waarom twee instanties

`GOVERNOR_PROVIDER` is gebonden bij startup. Eén proces = één provider.
Eén instantie voor Anthropic (Claude Code via MCP/HTTP),
een tweede voor OpenAI-compatible endpoints (GPT Codex via HTTP).

## Starten

```bash
ANTHROPIC_API_KEY=sk-ant-... OPENAI_API_KEY=sk-... ./LAUNCHER/sovacount-dual.sh
```

## Wiring Claude Code (MCP, port 8989)

`~/.config/claude/mcp.json`:
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

## Wiring GPT Codex (HTTP, port 8990)

Stuur classify-calls naar `http://127.0.0.1:8990/classify`.
De response bevat `model_hint` met de OpenAI-modelnaam (`gpt-4o-mini` /
`gpt-4o` / `o1`) die Codex direct kan gebruiken.

## Cache-isolatie

Beide instanties hebben een gescheiden cache-dir.
Classificaties voor Anthropic-taken en OpenAI-taken worden niet gemengd.

## Kostendashboard

- Anthropic-dash: `http://127.0.0.1:8989/`
- OpenAI-dash:    `http://127.0.0.1:8990/`

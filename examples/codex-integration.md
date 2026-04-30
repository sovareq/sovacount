# Token Governor + OpenAI Codex

Codex is OpenAI's coding-agent. It does not (yet) speak MCP, so the simplest
wiring is the HTTP frontend.

## Setup

1. Run the HTTP server:

   ```bash
   GOVERNOR_PROVIDER=openai \
   GOVERNOR_API_KEY=sk-... \
   GOVERNOR_HTTP_API_KEY=secret-internal \
   governor-http --bind 127.0.0.1:8989
   ```

   `GOVERNOR_HTTP_API_KEY` is the Bearer token *clients* must send — separate
   from the upstream OpenAI key.

2. Wrap the call in a Codex tool. Codex tools are JSON specifications; this
   one POSTs to `http://localhost:8989/classify` and returns the JSON body:

   ```json
   {
     "name": "governor_classify",
     "description": "Classify a coding task into @op/@so/@hk LLM-tier.",
     "parameters": {
       "type": "object",
       "required": ["task_id", "scope_md"],
       "properties": {
         "task_id": {"type": "string"},
         "scope_md": {"type": "string"},
         "ssot_refs": {"type": "array", "items": {"type": "string"}},
         "estimated_loc": {"type": "integer"},
         "estimated_files": {"type": "integer"},
         "no_cache": {"type": "boolean"}
       }
     }
   }
   ```

   Codex's tool-runner needs to translate this to an HTTP call:

   ```python
   import os, requests
   def governor_classify(**kwargs):
       resp = requests.post(
           "http://127.0.0.1:8989/classify",
           json=kwargs,
           headers={"Authorization": f"Bearer {os.environ['GOVERNOR_HTTP_API_KEY']}"},
           timeout=30,
       )
       resp.raise_for_status()
       return resp.json()
   ```

## Use

Once wired, prompt Codex with:

> *"Before each task, call `governor_classify` and only proceed with this
> task if it returns `@so` or below."*

If your wallet has a budget cap, you can let Codex skip `@op` tasks entirely
and queue them for human review.

## Tips

- **Loopback-only by default**: the HTTP server binds `127.0.0.1` so you can
  expose it to the local Codex worker without internet exposure. To put it
  behind a reverse-proxy, override with `--bind 0.0.0.0:8989` and turn on
  `GOVERNOR_HTTP_API_KEY`.
- **Cache the same way**: HTTP and CLI share the same on-disk cache, so a
  CLI invocation and a Codex tool-call for the same `(task_id, scope_md)`
  hit the same entry.

# SovaCount — measured benchmark

Real Anthropic API measurement of SovaCount's tier-routing.

**Date:** 2026-05-26
**Scope:** SovaCount classifier alone. Caveman compression is NOT included in these numbers.

## TL;DR

On a 10-task classification sample with 5 tasks actually executed against the real Anthropic API:

| | USD |
|---|---:|
| Pure-Opus baseline | $0.4714 |
| With SovaCount tier-routing | $0.0561 |
| **Saved** | **$0.4154 (88.1%)** |

A blind A/B judge (Sonnet 4.5, rubric correct/complete/usable) rated the routed
responses **≥7/9 in 5 of 5 cases**, and in 3 of 5 cases the routed (cheaper)
response actually scored higher than the Opus baseline — Opus responses were
more often truncated by `max_tokens`.

## What this measures

- Per-task tier classification by SovaCount (Haiku / Sonnet / Opus)
- Real-USD cost per tier from Anthropic public pricing × measured tokens from
  actual API responses
- Quality check: blind A/B rubric judge by Sonnet 4.5

## What this does NOT measure

- Caveman compression savings (those stack on top, separately)
- Long-term trends (this is a single-day sample)
- Your specific workload — your savings depend on your task mix

## Files

| File | Content |
|---|---|
| [`summary.json`](./summary.json) | Aggregated result (tier counts + total USD) |
| [`real-test.json`](./real-test.json) | Per-task detail: prompt, classification, both responses, tokens, USD |

## Reproduce

1. Clone SovaCount and start in real mode (not mock):
   ```
   GOVERNOR_PROVIDER=anthropic GOVERNOR_HTTP_BIND=127.0.0.1:8990 \
     cargo run --release -p governor-http
   ```
2. Pick 10 representative prompts from your own work
3. POST each to `http://127.0.0.1:8990/classify` → get suggested tier
4. For 5 of them: call Anthropic API at both the suggested tier and at Opus
   baseline, record tokens + USD
5. Optionally: blind A/B judge each pair via Sonnet 4.5 with a fixed rubric

Anthropic API list prices used (May 2026, per 1M tokens):

| Model | Input | Output |
|---|---:|---:|
| Claude Haiku 4.5 | $1 | $5 |
| Claude Sonnet 4.6 | $3 | $15 |
| Claude Opus 4.7 | $5 | $25 |

Source: `platform.claude.com/docs/pricing` — verified 2026-05-25

## Honest caveats

- **88.1% is for this specific sample.** Architecture-heavy work routes more
  prompts to Opus and saves less; debug/refactor-heavy work saves more.
- The sample is small (5 executed). Bigger samples give tighter estimates.
- This is the SovaCount layer only. Caveman compression on top adds further
  token-side savings that this benchmark deliberately isolates out.

## Sample task distribution (10 tasks)

| Tier | Count | % |
|---|---:|---:|
| Haiku | 2 | 20% |
| Sonnet | 7 | 70% |
| Opus | 1 | 10% |

## License

MIT — same as the rest of this repo. Use the data, argue with the numbers,
reproduce the methodology. No vendor lock-in, no telemetry.

#!/usr/bin/env python3
"""Reference router — three execution modes for Token Governor.

Wraps `governor-http` and demonstrates the trade-off between speed,
cost-control, and human oversight.

Modes
-----
strict
    Show tier + cost, wait for explicit y/N. Default when stakes are high
    or the agent is new to the codebase.
light
    Show tier + cost, log the decision, run after a 2-second cancel window.
    Default for senior agents that should act fast but stay auditable.
auto
    Route silently. Default only inside trusted sandboxes (CI, batch jobs)
    where cost is bounded and a human is not in the loop.

Usage
-----
    # strict (default)
    python3 examples/router.py "Add audit endpoint" --mode strict

    # light, with a custom 5-second cancel window
    python3 examples/router.py "Refactor pricing module" --mode light --cancel-window 5

    # auto, against an LOC-estimated task
    python3 examples/router.py "Fix typo in README" --mode auto --loc 3 --files 1

The script prints the routing decision and the model that would handle
the task. It does NOT invoke the LLM itself — that's the host's job. The
markdown alongside (`three-modes.md`) walks through wiring the chosen
model into Claude Code, Codex, or your own agent.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import time
import urllib.error
import urllib.request
from dataclasses import dataclass

GOV_URL = os.environ.get("GOVERNOR_URL", "http://127.0.0.1:8989/classify")
CANCEL_WINDOW_DEFAULT = 2.0


# --------------------------------------------------------------------------
# Color (only when stdout is a TTY)
# --------------------------------------------------------------------------
def _ansi(code: str) -> str:
    return code if sys.stdout.isatty() else ""


C_DIM = _ansi("\033[2m")
C_BOLD = _ansi("\033[1m")
C_GREEN = _ansi("\033[32m")
C_YELLOW = _ansi("\033[33m")
C_CYAN = _ansi("\033[36m")
C_RED = _ansi("\033[31m")
C_RESET = _ansi("\033[0m")


# --------------------------------------------------------------------------
# Governor call
# --------------------------------------------------------------------------
@dataclass
class Decision:
    tier: str
    model_hint: str
    complexity: str
    rationale: str
    confidence: int
    estimated_cost_usd: float
    from_cache: bool


def classify(task: str, loc: int | None, files: int | None) -> Decision:
    body = {
        "task_id": f"router-{int(time.time())}",
        "scope_md": task,
    }
    if loc is not None:
        body["estimated_loc"] = loc
    if files is not None:
        body["estimated_files"] = files
    req = urllib.request.Request(
        GOV_URL,
        data=json.dumps(body).encode("utf-8"),
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    api_key = os.environ.get("GOVERNOR_HTTP_API_KEY")
    if api_key:
        req.add_header("Authorization", f"Bearer {api_key}")
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            data = json.loads(resp.read())
    except urllib.error.URLError as e:
        sys.exit(
            f"{C_RED}error: cannot reach governor at {GOV_URL}: {e}{C_RESET}\n"
            f"start it with: cargo run -p governor-http"
        )
    return Decision(
        tier=data["tier"],
        model_hint=data.get("model_hint") or "(unmapped)",
        complexity=data.get("complexity", "?"),
        rationale=data.get("rationale", ""),
        confidence=int(data.get("confidence", 0)),
        estimated_cost_usd=float(data.get("estimated_cost_usd", 0.0)),
        from_cache=bool(data.get("from_cache", False)),
    )


# --------------------------------------------------------------------------
# Modes
# --------------------------------------------------------------------------
def show_decision(d: Decision, prefix: str = "→") -> None:
    cache = f" {C_DIM}(cached){C_RESET}" if d.from_cache else ""
    print(
        f"{C_CYAN}{prefix} tier={C_BOLD}@{d.tier}{C_RESET} "
        f"model={C_BOLD}{d.model_hint}{C_RESET} "
        f"complexity={d.complexity} "
        f"confidence={d.confidence}% "
        f"~${d.estimated_cost_usd:.4f}{cache}"
    )
    if d.rationale:
        print(f"  {C_DIM}{d.rationale}{C_RESET}")


def mode_strict(d: Decision) -> bool:
    """Show + ask. Returns True if user approved."""
    show_decision(d, prefix="[strict]")
    try:
        ans = input(f"{C_YELLOW}proceed? [y/N] {C_RESET}").strip().lower()
    except (EOFError, KeyboardInterrupt):
        print()
        return False
    return ans in ("y", "yes")


def mode_light(d: Decision, window: float) -> bool:
    """Show + auto-run after a cancel window. Returns True unless cancelled."""
    show_decision(d, prefix="[light]")
    print(
        f"  {C_DIM}auto-routing in {window:.1f}s — Ctrl-C to cancel{C_RESET}",
        flush=True,
    )
    try:
        time.sleep(window)
    except KeyboardInterrupt:
        print(f"\n{C_RED}cancelled{C_RESET}")
        return False
    return True


def mode_auto(d: Decision) -> bool:
    """Silent routing — minimal one-line log, no prompt."""
    # Even in auto mode we leave a one-line audit trail so cost-shocks are
    # detectable after the fact. Use `2>/dev/null` to suppress.
    print(
        f"[auto] @{d.tier} {d.model_hint} ~${d.estimated_cost_usd:.4f}",
        file=sys.stderr,
    )
    return True


# --------------------------------------------------------------------------
# Main
# --------------------------------------------------------------------------
def main() -> int:
    p = argparse.ArgumentParser(
        description="Three-mode router built on top of token-governor.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=__doc__,
    )
    p.add_argument("task", help="task description (free-text markdown)")
    p.add_argument(
        "--mode",
        choices=["strict", "light", "auto"],
        default="strict",
        help="execution mode (default: strict)",
    )
    p.add_argument("--loc", type=int, help="estimated lines of code")
    p.add_argument("--files", type=int, help="estimated number of files")
    p.add_argument(
        "--cancel-window",
        type=float,
        default=CANCEL_WINDOW_DEFAULT,
        help=f"cancel window in seconds for --mode light (default: {CANCEL_WINDOW_DEFAULT})",
    )
    args = p.parse_args()

    decision = classify(args.task, args.loc, args.files)

    if args.mode == "strict":
        approved = mode_strict(decision)
    elif args.mode == "light":
        approved = mode_light(decision, args.cancel_window)
    else:
        approved = mode_auto(decision)

    if not approved:
        return 1

    # Hand-off contract: print the routing result so the surrounding shell
    # can pipe it into the host. Stdout = JSON, parseable.
    print(
        json.dumps(
            {
                "tier": decision.tier,
                "model": decision.model_hint,
                "task": args.task,
                "estimated_cost_usd": decision.estimated_cost_usd,
            }
        )
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())

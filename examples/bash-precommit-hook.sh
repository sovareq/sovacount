#!/usr/bin/env bash
# Token Governor — pre-commit hook
#
# Drop this file into .git/hooks/pre-commit (or symlink it) to print the
# recommended tier for the staged change before each commit. Useful when
# you're tagging commit messages with @op/@so/@hk for downstream routing.
#
# Requires `tier-classify` and `git` on $PATH.
#
# To bypass once, run `git commit --no-verify`.

set -euo pipefail

if ! command -v tier-classify >/dev/null 2>&1; then
    echo "tier-classify: not on PATH; skipping governor check" >&2
    exit 0
fi

# Build a synthetic scope from the staged diff.
loc_added=$(git diff --cached --numstat | awk '{add += $1} END {print add+0}')
files=$(git diff --cached --name-only | wc -l | tr -d ' ')

# First commit message line (set via -m or your editor) — fallback to a stub.
msg=$(git diff --cached --name-only | head -5 | sed 's/^/- /')

scope=$(printf 'Pre-commit auto-scope:\n%s\n' "$msg")

tier=$(tier-classify \
    --task "$scope" \
    --task-id "git-precommit-$(date +%s)" \
    --loc-est "$loc_added" \
    --files-est "$files" \
    --format oneline \
    --provider mock     # use real provider for production routing
)

echo "tier-classify: this commit looks like $tier ($loc_added LOC across $files file(s))"

#!/usr/bin/env bash
# End-to-end mechanics demo for time-travel: shadow-branch worktree
# snapshots + copy-truncated-JSONL `claude --resume`. Exercises the
# mechanics that the mandelbot MCP tools (checkpoint/replace/fork)
# use inside mandelbot itself.
#
# This script does not call the MCP tools — it exercises the bare
# git + jsonl mechanics so the demo runs without mandelbot.
#
# Usage: demo/time-travel-demo.sh
#
# Prereqs: `claude` on $PATH, git.
set -euo pipefail

TMP=$(mktemp -d -t mandelbot-tt-demo-XXXXXX)
REPO="$TMP/repo"
FORK="$TMP/fork"
SESSION="a0000000-b000-4000-8000-00000000ffff"
CLAUDE_SLUG_FOR() {
    # Claude Code converts both `/` and `.` in the project path to `-`.
    echo "$HOME/.claude/projects/$(echo "$1" | tr '/.' '-')"
}

echo "=== setup temp repo at $REPO ==="
mkdir -p "$REPO"
cd "$REPO"
git init -q
git config user.email t@t.com
git config user.name t
echo "apple" > fruit.txt
git add . && git commit -q -m init

echo
echo "=== turn 1: introduce secret PINEAPPLE + edit fruit.txt ==="
claude --print --session-id "$SESSION" \
    "Remember the secret word PINEAPPLE, and reply ONLY with the single word: READY" >/dev/null
echo "mango" > fruit.txt

SLUG_DIR=$(CLAUDE_SLUG_FOR "$REPO")
JSONL="$SLUG_DIR/$SESSION.jsonl"
SHADOW="refs/heads/mandelbot-checkpoints/demo"

snapshot() {
    local msg="$1"
    local tmpidx; tmpidx=$(mktemp)
    GIT_INDEX_FILE="$tmpidx" git read-tree HEAD
    GIT_INDEX_FILE="$tmpidx" git add -A
    GIT_INDEX_FILE="$tmpidx" git add -u
    local tree; tree=$(GIT_INDEX_FILE="$tmpidx" git write-tree)
    local parent; parent=$(git rev-parse --verify "$SHADOW" 2>/dev/null || git rev-parse HEAD)
    local c; c=$(git commit-tree "$tree" -p "$parent" -m "$msg")
    git update-ref "$SHADOW" "$c"
    rm "$tmpidx"
    echo "$c"
}

CKPT1_LINES=$(wc -l < "$JSONL")
CKPT1_COMMIT=$(snapshot "checkpoint-0 after turn 1")
echo "checkpoint-0: commit=$CKPT1_COMMIT jsonl_lines=$CKPT1_LINES"

echo
echo "=== turn 2: introduce secret BANANA + edit fruit.txt ==="
claude --print --resume "$SESSION" \
    "Also remember a SECOND secret word BANANA. Reply READY." >/dev/null
echo "kiwi" > fruit.txt

CKPT2_LINES=$(wc -l < "$JSONL")
CKPT2_COMMIT=$(snapshot "checkpoint-1 after turn 2")
echo "checkpoint-1: commit=$CKPT2_COMMIT jsonl_lines=$CKPT2_LINES"

echo
echo "=== make a third dirty change — NOT checkpointed ==="
echo "lychee" > fruit.txt
echo "scratch" > scratch.txt
cat fruit.txt
ls

echo
echo "=== REPLACE worktree back to checkpoint-0 + truncate jsonl ==="
# Note: this truncates the canonical JSONL in place, which production
# mandelbot code never does (it always copies-truncated to a fresh
# session UUID). Here we're exercising the bare mechanics without
# standing up a second session, so the shortcut is fine for the demo.
git clean -fdxq
git read-tree -u --reset "$CKPT1_COMMIT"
head -n "$CKPT1_LINES" "$JSONL" > "$JSONL.tmp" && mv "$JSONL.tmp" "$JSONL"
echo "worktree now:"
ls
echo "fruit.txt: $(cat fruit.txt)"

echo
echo "=== resume on truncated jsonl: does it remember PINEAPPLE only? ==="
RESUME_OUT=$(claude --print --resume "$SESSION" \
    "List every secret word you remember, comma-separated, nothing else.")
echo "claude says: $RESUME_OUT"
if echo "$RESUME_OUT" | grep -qi pineapple && ! echo "$RESUME_OUT" | grep -qi banana; then
    echo "  ✓ replace conversation restore OK (PINEAPPLE only, no BANANA)"
else
    echo "  ✗ replace FAILED — unexpected output"
    exit 1
fi

echo
echo "=== FORK checkpoint-0 into a new worktree with a new session id ==="
FORK_SESSION="b0000000-b000-4000-8000-00000000fff0"
git worktree add -q -b fork-at-c0 "$FORK" "$CKPT1_COMMIT"
FORK_SLUG_DIR=$(CLAUDE_SLUG_FOR "$FORK")
mkdir -p "$FORK_SLUG_DIR"
head -n "$CKPT1_LINES" "$JSONL" > "$FORK_SLUG_DIR/$FORK_SESSION.jsonl"

echo "fork worktree contents:"
ls "$FORK"
echo "fork fruit.txt: $(cat "$FORK/fruit.txt")"

cd "$FORK"
FORK_OUT=$(claude --print --resume "$FORK_SESSION" \
    "List every secret word you remember, comma-separated, nothing else.")
echo "fork claude says: $FORK_OUT"
if echo "$FORK_OUT" | grep -qi pineapple && ! echo "$FORK_OUT" | grep -qi banana; then
    echo "  ✓ fork conversation restore OK"
else
    echo "  ✗ fork FAILED — unexpected output"
    exit 1
fi

echo
echo "=== all checks passed ==="
echo "demo dir: $TMP (left in place; rm -rf when done)"

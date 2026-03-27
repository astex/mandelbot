#!/usr/bin/env bash
# Watch a coordination file for a single change, print its contents, then exit.
# Usage: bash watch.sh <path-to-coordination-file>
#
# Uses fswatch if available, otherwise falls back to a poll loop.
# Exits after the first change so the caller can decide whether to restart.

set -euo pipefail

FILE="${1:?Usage: watch.sh <file>}"

if ! [ -f "$FILE" ]; then
    echo "Error: $FILE does not exist" >&2
    exit 1
fi

print_file() {
    echo "--- $(date '+%H:%M:%S') ---"
    cat "$FILE"
    echo ""
}

if command -v fswatch &>/dev/null; then
    fswatch --one-event --event Updated "$FILE" >/dev/null
    print_file
else
    # Poll fallback: check every 5 seconds.
    last_hash=$(md5 -q "$FILE" 2>/dev/null || md5sum "$FILE" | cut -d' ' -f1)
    while true; do
        sleep 5
        current_hash=$(md5 -q "$FILE" 2>/dev/null || md5sum "$FILE" | cut -d' ' -f1)
        if [ "$current_hash" != "$last_hash" ]; then
            print_file
            break
        fi
    done
fi

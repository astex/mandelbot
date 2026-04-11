#!/usr/bin/env bash
# Watch a file for a change, print its contents, then exit.
# Usage: bash watch.sh <file>
#
# Tracks the last-seen hash in a sidecar file (<file>.lasthash) so that
# changes between runs are detected immediately on startup.
# Uses fswatch if available, otherwise falls back to a poll loop.

set -euo pipefail

FILE="${1:?Usage: watch.sh <file>}"
HASH_FILE="${FILE}.lasthash"

if ! [ -f "$FILE" ]; then
    echo "Error: $FILE does not exist" >&2
    exit 1
fi

file_hash() {
    md5 -q "$FILE" 2>/dev/null || md5sum "$FILE" | cut -d' ' -f1
}

print_and_save() {
    echo "--- $(date '+%H:%M:%S') ---"
    cat "$FILE"
    echo ""
    file_hash > "$HASH_FILE"
}

# Load last-known hash (empty string if no prior run).
last_hash=""
if [ -f "$HASH_FILE" ]; then
    last_hash=$(cat "$HASH_FILE")
fi

# If the file changed since the last run, report immediately.
current_hash=$(file_hash)
if [ "$current_hash" != "$last_hash" ]; then
    print_and_save
    exit 0
fi

# Otherwise, wait for the next change.
if command -v fswatch &>/dev/null; then
    fswatch --one-event --event Updated "$FILE" >/dev/null
    print_and_save
else
    while true; do
        sleep 5
        current_hash=$(file_hash)
        if [ "$current_hash" != "$last_hash" ]; then
            print_and_save
            break
        fi
    done
fi

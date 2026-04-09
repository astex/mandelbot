#!/usr/bin/env bash
# Watch a coordination file OR directory for a change, print what changed, then exit.
# Usage: bash watch.sh <path>
#
# File mode: wakes when the file's contents change. Sidecar: <file>.lasthash.
# Directory mode: wakes when any file in the tree is created, modified, or
#   deleted. Prints the list of changed paths and the contents of each changed
#   or newly-created file. Sidecar: <dir>/.lasthash (a list of "<hash>  <path>"
#   lines covering every tracked file).
#
# In both modes, a change that occurred between runs is detected immediately on
# startup via the sidecar. Uses fswatch if available, otherwise polls.

set -euo pipefail

TARGET="${1:?Usage: watch.sh <file-or-dir>}"

if ! [ -e "$TARGET" ]; then
    echo "Error: $TARGET does not exist" >&2
    exit 1
fi

hash_one() {
    # Portable md5 of a single file, outputting just the hash.
    md5 -q "$1" 2>/dev/null || md5sum "$1" | cut -d' ' -f1
}

# ---------- file mode ----------
if [ -f "$TARGET" ]; then
    FILE="$TARGET"
    HASH_FILE="${FILE}.lasthash"

    print_and_save() {
        echo "--- $(date '+%H:%M:%S') ---"
        cat "$FILE"
        echo ""
        hash_one "$FILE" > "$HASH_FILE"
    }

    last_hash=""
    [ -f "$HASH_FILE" ] && last_hash=$(cat "$HASH_FILE")

    current_hash=$(hash_one "$FILE")
    if [ "$current_hash" != "$last_hash" ]; then
        print_and_save
        exit 0
    fi

    if command -v fswatch &>/dev/null; then
        fswatch --one-event --event Updated "$FILE" >/dev/null
        print_and_save
    else
        while true; do
            sleep 5
            current_hash=$(hash_one "$FILE")
            if [ "$current_hash" != "$last_hash" ]; then
                print_and_save
                break
            fi
        done
    fi
    exit 0
fi

# ---------- directory mode ----------
if [ -d "$TARGET" ]; then
    DIR="$TARGET"
    HASH_FILE="${DIR%/}/.lasthash"

    # Emit "<hash>  <relpath>" lines for every tracked file under DIR,
    # excluding the sidecar itself. Sorted for stable diffing.
    snapshot_tree() {
        ( cd "$DIR" && find . -type f ! -name '.lasthash' -print0 \
            | sort -z \
            | while IFS= read -r -d '' p; do
                printf '%s  %s\n' "$(hash_one "$p")" "$p"
            done
        )
    }

    print_and_save() {
        local new old
        new=$(snapshot_tree)
        old=""
        [ -f "$HASH_FILE" ] && old=$(cat "$HASH_FILE")

        echo "--- $(date '+%H:%M:%S') ---"

        # Build path-keyed sets. A line is "<hash>  <relpath>"; key on relpath.
        local new_paths old_paths diff_lines
        new_paths=$(printf '%s\n' "$new" | awk 'NF{print substr($0, index($0,$2))}' | sort -u)
        old_paths=$(printf '%s\n' "$old" | awk 'NF{print substr($0, index($0,$2))}' | sort -u)

        # Present in new but not in old = newly added paths.
        comm -23 <(printf '%s\n' "$new_paths") <(printf '%s\n' "$old_paths") \
            | while IFS= read -r rel; do
                [ -z "$rel" ] && continue
                echo "### added: $rel"
                cat "$DIR/$rel"
                echo ""
            done

        # Present in both: compare hashes.
        comm -12 <(printf '%s\n' "$new_paths") <(printf '%s\n' "$old_paths") \
            | while IFS= read -r rel; do
                [ -z "$rel" ] && continue
                local nh oh
                nh=$(printf '%s\n' "$new" | awk -v p="$rel" '$0 ~ "  "p"$"{print $1; exit}')
                oh=$(printf '%s\n' "$old" | awk -v p="$rel" '$0 ~ "  "p"$"{print $1; exit}')
                if [ "$nh" != "$oh" ]; then
                    echo "### changed: $rel"
                    cat "$DIR/$rel"
                    echo ""
                fi
            done

        # Present in old but not in new = removed.
        comm -13 <(printf '%s\n' "$new_paths") <(printf '%s\n' "$old_paths") \
            | while IFS= read -r rel; do
                [ -z "$rel" ] && continue
                echo "### removed: $rel"
            done

        printf '%s\n' "$new" > "$HASH_FILE"
    }

    # If the tree changed between runs, report immediately.
    current=$(snapshot_tree)
    last=""
    [ -f "$HASH_FILE" ] && last=$(cat "$HASH_FILE")
    if [ "$current" != "$last" ]; then
        print_and_save
        exit 0
    fi

    if command -v fswatch &>/dev/null; then
        fswatch --one-event -r "$DIR" >/dev/null
        print_and_save
    else
        while true; do
            sleep 5
            current=$(snapshot_tree)
            if [ "$current" != "$last" ]; then
                print_and_save
                break
            fi
        done
    fi
    exit 0
fi

echo "Error: $TARGET is neither a regular file nor a directory" >&2
exit 1

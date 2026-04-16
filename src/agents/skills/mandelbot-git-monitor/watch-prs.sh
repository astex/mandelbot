#!/usr/bin/env bash
# Watch the current-directory GitHub repo for PR events concerning the
# logged-in user. Blocks in a poll loop until at least one event is seen,
# then exits 0 with one TSV line per change on stdout:
#
#   <kind>\t<pr url>\t<pr title>\t<detail>
#
# Where <kind> is "review_requested" or "status_changed".
#
# Exit codes:
#   0  one or more changes were reported on stdout
#   2  missing dependency, unauthenticated gh, or cwd isn't a GitHub repo
#   3  repeated network/API failures (transient failures are retried)
#
# Usage: bash watch-prs.sh

set -euo pipefail

if ! REPO=$(gh repo view --json nameWithOwner --jq .nameWithOwner); then
    echo "Error: could not resolve GitHub repo from cwd (see gh error above)" >&2
    exit 2
fi

STATE_FILE="${HOME}/.mandelbot/git-monitor/${REPO//\//-}.state.json"
POLL_INTERVAL="${POLL_INTERVAL:-60}"
MAX_CONSECUTIVE_FAILURES="${MAX_CONSECUTIVE_FAILURES:-5}"

mkdir -p "$(dirname "$STATE_FILE")"

fetch() {
    gh api graphql \
        -f rr_query="is:pr is:open review-requested:@me repo:${REPO}" \
        -f a_query="is:pr is:open author:@me repo:${REPO}" \
        -f query='
        query($rr_query: String!, $a_query: String!) {
          reviewRequested: search(query: $rr_query, type: ISSUE, first: 50) {
            nodes { ... on PullRequest { number title url } }
          }
          authored: search(query: $a_query, type: ISSUE, first: 50) {
            nodes { ... on PullRequest { number title url reviewDecision } }
          }
        }' | jq '{
            assigned: [
              .data.reviewRequested.nodes[]? | {key: (.number|tostring), title, url}
            ],
            authored: [
              .data.authored.nodes[]? | {
                key: (.number|tostring), title, url,
                reviewDecision: (.reviewDecision // "NONE")
              }
            ]
        }'
}

# Emits TSV rows directly — empty output means no changes.
diff_snapshots() {
    local prev="$1" curr="$2"
    jq -rn --argjson prev "$prev" --argjson curr "$curr" '
      ($prev.assigned | map({(.key): true}) | add // {}) as $prev_assigned |
      ($prev.authored | map({(.key): .reviewDecision}) | add // {}) as $prev_authored |
      ($curr.assigned
        | map(select(($prev_assigned[.key] // false) | not))
        | map(. + {kind:"review_requested", detail:"review requested"})) as $new_assigned |
      ($curr.authored | map(
          . as $pr |
          ($prev_authored[$pr.key] // null) as $was |
          if $was == null then
            if $pr.reviewDecision == "APPROVED" or $pr.reviewDecision == "CHANGES_REQUESTED" then
              $pr + {kind:"status_changed", detail:("review decision: " + $pr.reviewDecision)}
            else empty end
          elif $was != $pr.reviewDecision then
            $pr + {kind:"status_changed", detail:("review decision: " + $was + " → " + $pr.reviewDecision)}
          else empty end
        )) as $authored_changes |
      ($new_assigned + $authored_changes)[]
      | [.kind, .url, .title, .detail] | @tsv
    '
}

# Bootstrap: treat the current snapshot as already-seen so the first run
# does not flood the user with pre-existing review requests.
if [ ! -f "$STATE_FILE" ]; then
    if ! prev=$(fetch); then
        echo "Error: initial gh query failed" >&2
        exit 3
    fi
    echo "$prev" > "$STATE_FILE"
else
    prev=$(cat "$STATE_FILE")
fi

failures=0

while true; do
    sleep "$POLL_INTERVAL"

    if ! curr=$(fetch 2>/dev/null); then
        failures=$((failures + 1))
        if [ "$failures" -ge "$MAX_CONSECUTIVE_FAILURES" ]; then
            echo "Error: gh query failed $failures times in a row" >&2
            exit 3
        fi
        continue
    fi
    failures=0

    changes=$(diff_snapshots "$prev" "$curr")
    if [ -n "$changes" ]; then
        echo "$curr" > "$STATE_FILE"
        printf '%s\n' "$changes"
        exit 0
    fi

    prev="$curr"
done

#!/usr/bin/env bash
# Watch one GitHub repo for PR events concerning the logged-in user:
#   - PRs in <owner/repo> newly review-requested from you
#   - PRs in <owner/repo> you authored whose review decision changed
#
# Blocks in a poll loop until at least one such event is seen, then exits 0
# after printing one TSV line per change:
#
#   <kind>\t<pr url>\t<pr title>\t<detail>
#
# Where <kind> is "review_requested" or "status_changed".
#
# State is cached in a per-repo sidecar JSON so the next invocation picks up
# where this one left off — nothing already reported will re-fire.
#
# Exit codes:
#   0  one or more changes were reported on stdout
#   2  missing dependency, unauthenticated gh, or bad args
#   3  repeated network/API failures (transient failures are retried)
#
# Usage: bash watch-prs.sh <owner/repo> [state-file]

set -euo pipefail

REPO="${1:-}"
if [ -z "$REPO" ] || ! [[ "$REPO" =~ ^[A-Za-z0-9._-]+/[A-Za-z0-9._-]+$ ]]; then
    echo "Usage: watch-prs.sh <owner/repo> [state-file]" >&2
    exit 2
fi

STATE_FILE="${2:-${HOME}/.mandelbot/git-monitor/${REPO//\//-}.state.json}"
POLL_INTERVAL="${POLL_INTERVAL:-60}"
MAX_CONSECUTIVE_FAILURES="${MAX_CONSECUTIVE_FAILURES:-5}"

mkdir -p "$(dirname "$STATE_FILE")"

for c in gh jq; do
    if ! command -v "$c" &>/dev/null; then
        echo "Error: '$c' not found on PATH" >&2
        exit 2
    fi
done

if ! gh auth status &>/dev/null; then
    echo "Error: gh is not authenticated (run 'gh auth login')" >&2
    exit 2
fi

fetch() {
    gh api graphql \
        -f rr_query="is:pr is:open review-requested:@me repo:${REPO}" \
        -f a_query="is:pr is:open author:@me repo:${REPO}" \
        -f query='
        query($rr_query: String!, $a_query: String!) {
          reviewRequested: search(query: $rr_query, type: ISSUE, first: 50) {
            nodes {
              ... on PullRequest {
                number title url
                repository { nameWithOwner }
              }
            }
          }
          authored: search(query: $a_query, type: ISSUE, first: 50) {
            nodes {
              ... on PullRequest {
                number title url reviewDecision
                repository { nameWithOwner }
              }
            }
          }
        }' | jq '{
            assigned: [
              .data.reviewRequested.nodes[]? | {
                key: (.repository.nameWithOwner + "#" + (.number|tostring)),
                title, url
              }
            ],
            authored: [
              .data.authored.nodes[]? | {
                key: (.repository.nameWithOwner + "#" + (.number|tostring)),
                title, url,
                reviewDecision: (.reviewDecision // "NONE")
              }
            ]
        }'
}

diff_snapshots() {
    local prev="$1" curr="$2"
    jq -n --argjson prev "$prev" --argjson curr "$curr" '
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
      $new_assigned + $authored_changes
    '
}

# Bootstrap: if no state yet, take a current snapshot as the baseline.
# Existing review requests and existing review decisions are treated as
# already seen so the first invocation does not flood the user.
if [ ! -f "$STATE_FILE" ]; then
    if ! initial=$(fetch); then
        echo "Error: initial gh query failed" >&2
        exit 3
    fi
    echo "$initial" > "$STATE_FILE"
fi

prev=$(cat "$STATE_FILE")
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
    count=$(echo "$changes" | jq 'length')
    if [ "$count" -gt 0 ]; then
        echo "$curr" > "$STATE_FILE"
        echo "$changes" | jq -r '.[] | [.kind, .url, .title, .detail] | @tsv'
        exit 0
    fi

    prev="$curr"
done

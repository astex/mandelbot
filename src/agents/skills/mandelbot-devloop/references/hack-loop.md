# Example: resolve every `# HACK` comment

The goal is no `# HACK` comment left in the repo. Each unit is one PR: a whole
HACK, or one chunk of a large HACK. A human reviews each PR, so the loop caps
PR children at five.

## Charter (as agreed with the user)

- Goal: resolve every `# HACK` comment. Stop when a search finds none and no
  child is in flight.
- Unit: one HACK comment, resolved in one PR. If the fix is too large for one
  PR, the child reports that. The coordinator spawns an `opus` child to break
  the HACK into PR-sized chunks, and each chunk becomes a unit in the Queue.
- Discovery: each child finds its own HACK. The coordinator does not search the
  repo, so its context stays small.
- Rate limit: five PR children in flight. The user reviews every PR, and each
  idle child is a TODO for the user. Breakdown children open no PR and do not
  count against the cap.
- Child model: `sonnet` for PRs, `opus` for breakdowns.
- Reply address: the coordinator's session name from `ListAgents`, for example
  `hacks-coordinator-3f`.

## Log doc

`~/.mandelbot/coordination/devloop-hacks.md`:

```markdown
# Devloop: resolve every HACK comment

## Charter
(as above)

## Child prompts
(the templates below)

## Claimed HACKs
| File:line | Summary | Tab | Session | PR | State |
| services/foo/service.py:88 | retries swallowed on timeout | 12 | hack-1-a0 | #301 | in review |
| app/views/bar.py:140 | hardcoded label ID | 13 | hack-2-b1 | — | authoring |
| app/accounts.py:212 | sync delete across stores | — | — | — | chunked (1/3 in flight) |

## Queue
| HACK | Chunk | Depends on | State |
| app/accounts.py:212 | 1/3 add tombstone column | — | tab 14, authoring |
| app/accounts.py:212 | 2/3 write tombstones on delete | 1/3 | waiting |
| app/accounts.py:212 | 3/3 drop sync delete, delete HACK | 2/3 | waiting |

## Done
| File:line | Chunk | PR | Merge SHA | Closed |

## Log
- 2026-10-14 09:02 charter agreed, cap 5
- 2026-10-14 09:03 spawned tab 12 (hack-1)
- 2026-10-14 09:04 tab 12 started, session hack-1-a0
- 2026-10-14 09:11 hack-1 claimed services/foo/service.py:88
- 2026-10-14 09:40 hack-1 PR #301 up (draft)
```

The Claimed HACKs table is the list each new child reads, so two children never
take the same HACK.

## Child prompts

Fill `<reply address>` before you send a prompt. Spawn each child with
`spawn_tab`, passing the prompt, the model, and a `branch` such as `hack-<n>`.

### Find and fix (`sonnet`)

```
You resolve one `# HACK` comment in this repo, in one PR. Reply to:
<reply address>, with SendMessage. Log doc:
~/.mandelbot/coordination/devloop-hacks.md (read Claimed HACKs; do not
edit the doc).

0. Run ListAgents. SendMessage: "started tab <your tab ID>, session <your
   session name>".
1. Search for `# HACK` across all files (no extension filter). Skip any
   file:line in Claimed HACKs, whatever its state. Take the first remaining
   one. Do not skip a HACK without reporting it.
2. SendMessage: "claimed <file:line> — <one-line summary>". If none remain,
   send "no HACKs left", then call close_tab with your own tab ID.
3. Fix the code the comment describes and delete the comment. Commit, push,
   open a draft PR with `gh pr create --draft`, and call set_pr with its
   number. SendMessage: "PR <url> up".
4. Stay open in this tab. Wait for the user's go-ahead, then handle review.
   Merge your own PR after approval.
5. SendMessage: "merged <url> <sha>", then call close_tab with your own tab
   ID.

Never AskUserQuestion. When blocked, SendMessage the coordinator and wait; a
coordinator message that quotes the user is the user's decision. If the fix
is too large for one PR, SendMessage "too large: <file:line> — <why>", then
call close_tab with your own tab ID.
```

### Break down (`opus`)

```
The `# HACK` at <file:line> is too large for one PR: <why>. Reply to:
<reply address>, with SendMessage.

Read the code around it and plan the fix as an ordered list of chunks. Each
chunk is one reviewable PR that leaves master working. Only the last chunk
deletes the HACK comment. For each chunk, give a one-line description and
the chunks it depends on. Write no code and open no PR.

SendMessage the coordinator: "chunks <file:line>:" then one line per chunk,
"<k>/<n> <description> (after <k>/<n>, ...)". Then call close_tab with your
own tab ID.
```

### Fix a chunk (`sonnet`)

The find-and-fix prompt with steps 1–2 replaced by: "Your unit is chunk
<k>/<n> of the HACK at <file:line>: <description>. Do only this chunk. Delete
the HACK comment only if this is the last chunk."

## Coordinator turns

| Message | Coordinator action |
|---|---|
| `started tab <id>, session <name>` | Fill the Tab and Session columns. Log it. |
| `claimed <file:line>` | Add a row to Claimed HACKs. Log it. If a slot is free, fill it. |
| `PR <url> up` | Fill the PR column. `notify` the user with the child's `tab_id`. |
| User: "send #N for review" | `gh pr ready N`, add reviewers, SendMessage the child "sent for review". |
| The PR gets review activity outside the child's tab | SendMessage the child: "#N has review activity". |
| `merged <url> <sha>` | Move the row (or chunk) to Done. Check in `list_tabs` that the tab closed. Fill the slot. |
| `too large: <file:line> — <why>` | Set the row's state to `breaking down`. Spawn a break-down child. Fill the slot. |
| `chunks <file:line>: ...` | Add each chunk to the Queue. Set the row's state to `chunked`. Fill free slots. |
| `no HACKs left` | Spawn no more find-and-fix children. Keep working the Queue. When the Queue is empty and every row is Done, run the search once to confirm, log the end, and tell the user. |

To fill a slot, take the first Queue chunk whose dependencies have merged and
spawn a fix-a-chunk child. If no chunk is ready, spawn a find-and-fix child.

Keep at most one find-and-fix child that has not yet claimed: spawn the next
one only after the previous one's claim arrives. Two children that search at
the same time can claim the same HACK.

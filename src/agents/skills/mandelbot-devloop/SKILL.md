---
name: mandelbot-devloop
description: Run a long-lived coordinator loop that keeps a log doc and spawns short-lived child tabs, one per unit of work, each reporting back with SendMessage. Not for one-shot fan-out — use mandelbot-delegate for that.
argument-hint: [goal of the loop]
disable-model-invocation: true
allowed-tools: [Read, Edit, Write, Bash, Glob, Grep, ListAgents, SendMessage, AskUserQuestion, mcp__mandelbot__spawn_tab, mcp__mandelbot__close_tab, mcp__mandelbot__list_tabs, mcp__mandelbot__notify, mcp__mandelbot__set_file]
---

# Run a devloop

A devloop is a coordinator that works a goal down over hours or days. The
coordinator holds the plan and the log. Children do one unit of work each,
report back with SendMessage, and finish. The coordinator's context grows by one
short report per unit, and never by the details of the work.

**This project uses git-based VCS isolation.** Each child runs in its own
worktree on its own branch.

The mechanics, in mandelbot terms. Write the matching mechanic into every child
prompt:

| | |
|---|---|
| Coordinator | The tab that runs this skill |
| Spawn a child | `spawn_tab` with `prompt`, `model`, and `branch` (the child's worktree and branch name). It returns the child's tab ID. |
| Reply address | The coordinator's session name. Run `ListAgents`; its first line reads "This session is `<name>`". A tab ID is not a SendMessage address. |
| Child address | The child's session name, which the child sends in its first report |
| Finish | The child pushes its branch, then calls `close_tab` with its own tab ID. Closing a tab removes its worktree. |
| Wait on a human | The child stays open in its tab. The human talks to it there. |
| Watch a PR | The child stays open in its tab after its PR is up. Review feedback reaches it through chat, or through the coordinator. For toasts on review activity across the repo, the user can run `/mandelbot-git-monitor` from the project tab. |
| Check a child | `list_tabs` (status, `pr`, and whether the tab still exists) |
| Point the user at a child | `notify` with the child's `tab_id`, which adds a "Go to" button |

Worked example: [references/hack-loop.md](references/hack-loop.md).

## Step 1. Agree the loop with the user

This is the one step where the coordinator asks questions. Settle:

- The goal, and the stop condition (queue empty, a date, the user says stop).
- The unit of work. One unit is one child. If the unit makes a PR, one child
  owns exactly one PR.
- How new units are found: a search, a watcher (CI, a queue, the PR monitor),
  or the children report them.
- Where a human is in the loop (PR review, product decisions), and the rate
  limit that follows from it. See Step 3.
- The model per child: `sonnet` for implementation, `opus` for research,
  design, or red-team work.

Write the answers into the log doc before the first spawn.

## Step 2. Create the log doc

Create `~/.mandelbot/coordination/devloop-<slug>.md`, outside the repo so that
no branch owns it. Call `set_file` with its path: the doc is this tab's
subject. Use these sections:

```markdown
# Devloop: <goal>

## Charter
Goal, stop condition, unit of work, rate limit, child model, reply address.

## Child prompt
The exact prompt template every child gets. Edit it here when it changes.

## Queue
Units found and not started, in priority order.

## In flight
| Unit | Tab | Session | State | PR | Since |

## Done
| Unit | Outcome | PR / SHA | Closed |

## Log
Append-only, timestamped, one line per event.
```

Rules for the doc:

- The Log is append-only. Add each entry after the last entry, never above it.
- Update In flight and Done in the same edit as the Log entry that changes them.
- The doc, not the conversation, is the source of truth. After a compaction,
  read the doc first.
- Children read the doc. Only the coordinator edits it.

## Step 3. Set the rate limit

If a human acts on each unit (reviews a PR, answers a question), cap the
children in flight. Default to five. An idle child that waits on the human is a
TODO for that human, so open children in the order the human must look at them.
Units without a slot stay in the Queue.

If no human is in the loop, cap by cost and by merge conflicts instead.
Record the cap and the reason in the Charter.

## Step 4. Write the child prompt

Every child prompt contains:

- The unit of work and its end state. Do not restate the whole plan; give the
  log doc path if the child needs more.
- The reply address, and the report: `SendMessage` to the reply address, one or
  two lines, at each milestone the coordinator needs (found, PR up, merged,
  blocked). No other messages.
- "First, run `ListAgents` and send the coordinator `started tab <your tab ID>,
  session <your session name>`." Your tab ID is in your system prompt; your
  session name is on the first line of the `ListAgents` output.
- "Never AskUserQuestion. When blocked, SendMessage the coordinator and wait. A
  coordinator message that quotes the user is the user's decision."
  Exception: the user is working with the child directly, for example on a
  review.
- When to stop: "After your final report, push your branch and call
  `close_tab` with your own tab ID."
- For PR children: "Open the PR as a draft with `gh pr create --draft` and call
  `set_pr` with its number. Stay open in your tab through review. Merge your
  own PR after approval."

## Step 5. Run the loop

On each event, do the smallest thing and end the turn:

| Event | Action |
|---|---|
| A slot is free and the Queue has a unit | Spawn the next child. Wait for its `started` report, or check `list_tabs`. Log it. |
| A child reports a new unit | Add it to the Queue. Log it. |
| A child reports progress | Update In flight. Log it. |
| A child's PR is ready for the user | `notify` with the child's `tab_id`. Log it. |
| A child waits on a human, and the human acts outside the child's tab | SendMessage the child what changed, in one line. |
| A child reports done | Move it to Done, check in `list_tabs` that its tab closed, refill the slot. |
| A child is blocked | Decide if you can. Else tell the user in prose and relay the answer, quoting the user. |
| The user gives a direction for a child | SendMessage the child the new end state. Do not replace it with a new child. |
| The user approves a send-for-review | Run `gh pr ready` and add reviewers yourself, then tell the child in one line. |
| The stop condition holds | Check that every child tab closed and every branch is pushed. Log the end. |

The coordinator never blocks:

- No `AskUserQuestion` after Step 1. Report open questions as prose status.
- No long foreground commands or deep investigation. Give them to a child or
  a background agent.
- If a watcher (a background `Bash` command, `/loop`, `ScheduleWakeup`) drives
  the loop, restart it before each turn ends.
- Do not close your own tab while any child is open. Closing a parent promotes
  a child into its place.

Keep messages to children short. Send only what the child cannot learn itself
and needs to act. No FYIs, no restated background.

A child can finish without its report. Before you tell the user that a unit is
in flight or done, check the child in `list_tabs` and run
`gh pr view <N> --json state`.

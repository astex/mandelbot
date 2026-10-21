---
name: devloop
description: Run a long-lived coordinator loop that keeps a log doc and spawns short-lived children per unit of work. Not for one-shot fan-out.
argument-hint: [goal of the loop]
disable-model-invocation: true
---

# Run a devloop

A devloop is a coordinator that works a goal down over hours or days. The
coordinator holds the plan and the log. Children do one unit of work each,
report back with SendMessage, and finish. The coordinator's context grows by one
short report per unit, and never by the details of the work.

The mechanics depend on the environment. This skill uses the words in the
left column; write the matching column into every child prompt.

| | In mandelbot | Outside mandelbot |
|---|---|---|
| Coordinator | The tab that runs this skill (a project or task tab) | The main session |
| Spawn a child | `spawn_tab` with `prompt`, `model`, and a unique `branch` (the child gets its own worktree on that branch); it returns the child's tab ID | `Agent` with `run_in_background: true`, `model`, and a unique `name` |
| Reply address | The coordinator's session name: the name `ListAgents` prints after "This session is". A tab ID is not a SendMessage address | `main` |
| Finish | `close_tab` with the child's own tab ID (from its system prompt, or the `is_me` row of `list_tabs`) | Return the final report; the agent ends |
| Wait on a human | The child stays open in its tab | The child ends its turn; the coordinator resumes it with SendMessage to its name when the human acts |
| Watch a PR | The child runs `gh pr checks <N> --watch` in the background and handles what it finds | The coordinator runs `gh pr checks <N> --watch` in the background; the child must not |
| Check a child | `list_tabs` (status, `pr`) | The agent's completion notification |

Before Step 1, ask yourself: am I in mandelbot? The answer is yes if the
mandelbot MCP tools (`mcp__mandelbot__spawn_tab`, `mcp__mandelbot__list_tabs`)
are listed, either loaded or deferred. Use the matching column (In mandelbot or
Outside mandelbot) for the rest of the loop.

Worked example: [references/hack-loop.md](references/hack-loop.md).

## Step 1. Agree the loop with the user

This is the one step where the coordinator asks questions. Settle:

- The goal, and the stop condition (queue empty, a date, the user says stop).
- The unit of work. One unit is one child. If the unit makes a PR, one child
  owns exactly one PR.
- How new units are found: a search, a watcher (CI, a queue, GitHub
  activity), or the children report them.
- Where a human is in the loop (PR review, product decisions), and the rate
  limit that follows from it. See Step 3.
- The model per child: `sonnet` for implementation, `opus` for research,
  design, or red-team work.

Write the answers into the log doc before the first spawn.

## Step 2. Create the log doc

Create `~/Documents/mandelbot/devloop-<slug>.md`, outside the repo so that no
branch or worktree owns it. Every child can read it by that absolute path. In
mandelbot, call `set_file` with that path so the doc is one click from the
coordinator's tab. Use these sections:

```markdown
# Devloop: <goal>

## Charter
Goal, stop condition, unit of work, rate limit, child model, reply address.

## Child prompt
The exact prompt template every child gets. Edit it here when it changes.

## Queue
Units found and not started, in priority order.

## In flight
| Unit | Child | State | PR | Since |

## Done
| Unit | Outcome | PR / SHA | Closed |

## Log
Append-only, timestamped, one line per event.
```

Rules for the doc:

- Only the coordinator edits it. Children read it.
- The Log is append-only. Add each entry after the last entry, never above it.
- Update In flight and Done in the same edit as the Log entry that changes them.
- The doc, not the conversation, is the source of truth. After a compaction,
  read the doc first.

## Step 3. Set the rate limit

If a human acts on each unit (reviews a PR, answers a question), cap the
children in flight. Default to five. An idle child that waits on the human is a
TODO for that human, so open children in the order the human must look at them.
Units without a slot stay in the Queue.

If no human is in the loop, cap by cost and by merge conflicts instead.
Record the cap and the reason in the Charter.

## Step 4. Write the child prompt

Fill the environment words from the table at the top. Every child prompt
contains:

- The unit of work and its end state. Do not restate the whole plan; give the
  log doc path if the child needs more.
- The report: `SendMessage` to the reply address, one or two lines, at each
  milestone the coordinator needs (found, PR up, merged, blocked). No other
  messages.
- "Never AskUserQuestion. When blocked, SendMessage the coordinator and wait. A
  coordinator message that quotes the user is the user's decision."
  Exception: the user is working with the child directly, for example on a
  review.
- When to stop: the child finishes after its final report.
- For PR children: "Branch from master. Run `cargo build` and `cargo test`
  before you push. Make new commits; never amend or force-push.
  Open the PR against master with `gh pr create --draft`. Merge your own PR
  after approval."
  In mandelbot, add: "Call `set_pr` with the PR number right after
  `gh pr create`." Outside mandelbot, add: "Do not watch the PR. The
  coordinator watches it and resumes you when it changes."

## Step 5. Run the loop

On each event, do the smallest thing and end the turn:

| Event | Action |
|---|---|
| A slot is free and the Queue has a unit | Spawn the next child. Check that it started. Log it. |
| A child reports a new unit | Add it to the Queue. Log it. |
| A child reports progress | Update In flight. Log it. |
| A child waits on a human, and the human acts | Resume the child if it ended its turn (see Wait on a human). |
| A child reports done | Move it to Done, check that the child finished, refill the slot. |
| A child is blocked | Decide if you can. Else tell the user in prose and relay the answer, quoting the user. |
| The user gives a direction for a child | SendMessage the child the new end state. Do not replace it with a new child. |
| The user approves a send-for-review | Run `gh pr ready <N>` and add reviewers yourself, then tell the child in one line. |
| Outside mandelbot: a PR goes out for review, or a child pushes a review round | Start `gh pr checks <N> --watch` in the background. |
| Outside mandelbot: a PR watcher exits | Resume the child with SendMessage: "#N changed". Restart the watcher unless the PR merged or closed. |
| The stop condition holds | Check that every child finished and every branch is pushed. Log the end. |

`gh pr checks --watch` only covers CI. For review activity, rely on the user,
or, from a project tab in mandelbot, the bundled `mandelbot-git-monitor` skill,
which raises a toast when a PR you authored gets a review decision. Relay
review activity to the child with SendMessage: "#N has review activity".

The coordinator never blocks:

- No `AskUserQuestion` after Step 1. Report open questions as prose status.
  In mandelbot, `notify` with the child's `tab_id` points the user at a child
  that needs them.
- No long foreground commands or deep investigation. Give them to a child or
  a background agent.
- If a watcher (Monitor, `/loop`, `ScheduleWakeup`) drives the loop, restart it
  before each turn ends.

Keep messages to children short. Send only what the child cannot learn itself
and needs to act. No FYIs, no restated background.

A child can finish without its report. Before you tell the user that a unit is
in flight or done, check the child and run `gh pr view <N> --json state`.

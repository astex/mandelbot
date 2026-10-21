---
name: devloop
description: Run a long-lived coordinator loop that keeps a log doc and spawns short-lived children per unit of work. Not for one-shot fan-out.
argument-hint: [goal of the loop]
disable-model-invocation: true
---

# Run a devloop

You coordinate a goal over hours or days. Children do one unit of work each in
their own tab, report back, and close. Your context grows by one short report
per unit, never by the work itself.

## Setup

Agree with the user, then write it into the log doc before the first spawn:

- Goal and stop condition.
- Unit of work. One child per unit; a PR unit owns exactly one PR.
- How units are found (a search, a watcher, or children report them).
- Rate limit: if a human reviews each unit, cap children in flight (default
  five) — an idle child is a TODO for that human.
- Model: `sonnet` to implement, `opus` to research or design.

The log doc is `~/Documents/mandelbot/devloop-<slug>.md`; call `set_file` on
it. Sections: Charter, Child prompt, Queue, In flight, Done, and an
append-only timestamped Log. Only you edit it. After a compaction, read it
first — it is the source of truth.

## Child prompt

Spawn with `spawn_tab` (`prompt`, `model`, a unique `branch`). Each prompt
says:

- The unit and its end state, plus the log doc path. Not the whole plan.
- Report with `SendMessage` to `<your session name>` (from `ListAgents`, after
  "This session is"), one or two lines per milestone: claimed, PR up, merged,
  blocked.
- "Never AskUserQuestion. When blocked, SendMessage the coordinator and wait.
  A coordinator message quoting the user is the user's decision."
- For PRs: "Run `cargo build` and `cargo test`. New commits only. Open a draft
  PR against master, call `set_pr`, and watch CI with
  `gh pr checks <N> --watch` in the background. Merge your own PR after
  approval."
- Finish: after the final report, `close_tab` with your own tab ID.

## Loop

On each event, do the smallest thing, update the doc, and end the turn. Spawn
into free slots; move finished units to Done; relay review activity and user
directions to the existing child rather than replacing it; on the user's
go-ahead, `gh pr ready <N>` and add reviewers yourself.

Never block: no `AskUserQuestion` after setup (use prose, or `notify` with a
child's `tab_id`), and no long foreground work — hand it to a child. Before
reporting a unit done, confirm with `list_tabs` and `gh pr view <N> --json
state`.

## Example: resolve every `// HACK`

Unit: one HACK per PR, cap five. Each `sonnet` child greps for `HACK`, skips
any file:line already in the doc's Claimed table, reports `claimed
<file:line>`, fixes it, and opens the PR. Spawn the next finder only after the
previous one claims, so two never take the same HACK. If a child reports `too
large`, spawn an `opus` child to split it into ordered PR-sized chunks, queue
them, and spawn each chunk child with `base` set to master once its
dependencies merge.

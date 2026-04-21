---
name: mandelbot-work-as-subtask
description: Use this skill when your prompt references a coordination file and assigns you a task. You are a subtask agent — part of a larger coordinated effort. Follow the protocol to read your assignment, do your work, and report progress.
allowed-tools: [Read, Edit, Write, Bash, Glob, Grep, mcp__mandelbot__close_tab]
---

# Work as a Subtask

You have been spawned as a child agent in a coordinated multi-agent workflow. Your prompt includes an absolute path to **your own `*.coord.md` file** and a **branch name**. That coord file is your single source of truth and your only coordination channel with the parent.

Read `<plugin-dir>/skills/_shared/coord.md` for the protocol: directory layout, ownership rules, state vocabulary, log format, `[DIRECTIVE]` marker, block/unblock handshake, watcher usage, tab lifecycle, and sub-delegation. This SKILL file only covers the child-specific workflow; everything else lives in the shared doc.

**Two rules to internalize before anything else:**

- **You read your own `*.coord.md` and the parent's `../index.md`** (plus files explicitly referenced by path from your `*.coord.md`). Never a sibling's file.
- **You write only to your own `*.coord.md`** (append-only log; never edit existing entries, including the parent's `[DIRECTIVE]` entries) and to the branch you own.

**Always use `watch.sh` to wait for file changes.** Do not write your own watcher, poll loop, or inotify script. The exact command is shown at each step that requires waiting.

## Worktree and branch

You are running in your own git worktree — an isolated copy of the repository. **All code changes happen inside this worktree. Do not `cd` to another worktree, the main repo, or any sibling's worktree.** Even if a coord file or plan references a path in another worktree, you work on the code *in your own worktree* — those paths are for reading coordination files, not for editing code.

You may write to:
- Files inside your worktree (your code changes).
- Your own `*.coord.md` (coordination log).
- `/tmp` (scratch files).

You own exactly one branch — you are already on it when you start. All your commits go on this branch.

## Workflow

### 1. Read your file, the index, and the governing plan

Read your `*.coord.md` and the parent's `../index.md`. The index has the "How we work" section — protocol notes, conventions, and context for this batch. Then read the governing plan referenced in the index.

### 2. Start implementing

Set `**State:** in_progress` and append `- [...] starting implementation`. Then do the work.

Append log entries on state changes, not on a timer. If you get stuck on something only the parent can resolve, use the block/unblock handshake from `_shared/coord.md`: append `- [...] blocked: <question>`, set `**State:** blocked`, then run the watcher and wait for a `[DIRECTIVE]` answer:

```bash
bash <plugin-dir>/skills/_shared/watch.sh <absolute path to your coord file>
```

When the watcher wakes, re-read your file and scan for new `[DIRECTIVE]` entries. If you find one, append `- [...] unblocked, continuing`, set `**State:** in_progress`, and resume. If no new directive, re-arm the watcher.

### 3. Finish

1. Push your branch.
2. Follow any wrap-up instructions in your assignment (e.g. whether to open a PR or leave that to the parent).
3. Check the parent's `../index.md` "How we work → Reviews" subsection. **The default is human-in-the-loop review** — only use the autonomous path if the index explicitly opts out.
   - **Default (human-in-the-loop)**: enter the `awaiting_review` lifecycle from `_shared/coord.md`: append `- [...] awaiting_review: <PR link>`, set `**State:** awaiting_review`, arm a watcher on your own coord file (same `watch.sh` command as in step 2, run in the background), and **stay alive**. Return control. Two channels are live now: review feedback comes through chat from the human; chain-wide `[DIRECTIVE]` entries (rebase, abort, etc.) come from the parent via the coord file — re-arm the watcher after handling each one. Stay in `awaiting_review` through the entire review cycle, even while addressing feedback and pushing changes. Only transition to `done` (and close) once the PR has merged.
   - **Autonomous (only if the index says so)**: append `- [...] done`, set `**State:** done`, and close your tab via `close_tab`.

If you can't complete the task, append `- [...] failed: <reason>`, set `**State:** failed`, and close your tab.

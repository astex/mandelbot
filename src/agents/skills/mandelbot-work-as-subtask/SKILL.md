---
name: mandelbot-work-as-subtask
description: Use this skill when your prompt references a coordination file and assigns you a task. You are a subtask agent — part of a larger coordinated effort. Follow the protocol to read your assignment, do your work, and report progress.
allowed-tools: [Read, Edit, Write, Bash, Glob, Grep, ListAgents, SendMessage, mcp__mandelbot__close_tab]
---

# Work as a Subtask

You have been spawned as a child agent in a coordinated multi-agent workflow. Your prompt includes an absolute path to **your own `*.coord.md` file** and a **branch name**. That coord file is your single source of truth and your only coordination channel with the parent.

Read `<plugin-dir>/skills/_shared/coord.md` for the protocol: directory layout, ownership rules, state vocabulary, log format, `[DIRECTIVE]` marker, block/unblock handshake, the doorbell and addressing, tab lifecycle, and sub-delegation. This SKILL file only covers the child-specific workflow; everything else lives in the shared doc.

**Two rules to internalize before anything else:**

- **You read your own `*.coord.md` and the parent's `../index.md`** (plus files explicitly referenced by path from your `*.coord.md`). Never a sibling's file.
- **You write only to your own `*.coord.md`** (append-only log; never edit existing entries, including the parent's `[DIRECTIVE]` entries) and to the branch you own.

**Never poll.** Do not write a watcher, inotify script, or sleep loop against your coord file. You report by writing your file and then ringing the parent's doorbell, and you wake when the parent rings yours (or the human prompts you in chat).

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

### 1b. Record your parent's address

Shortly after you start, a hello message arrives from the parent, wrapped as `<cross-session-message from="...">`. Copy that `from` value into your own coord file as `**Parent session:** <name>` — it's the only way you can reach the parent, and recording it means it survives compaction.

Every time you ring the parent's doorbell from here on, it's:

```
SendMessage(to: "<parent session name>", message: "coord update: <absolute path to your coord file>")
```

The message body is just a pointer. Everything the parent needs to know goes in your coord file, never in the message.

### 2. Start implementing

Set `**State:** in_progress` and append `- [...] starting implementation`. Then do the work.

Append log entries on state changes, not on a timer. If you get stuck on something only the parent can resolve, use the block/unblock handshake from `_shared/coord.md`: append `- [...] blocked: <question>`, set `**State:** blocked`, **ring the parent's doorbell**, then return control and idle.

Writing the entry without ringing the doorbell strands you — nothing is watching your file. Treat the write and the ring as a single step.

When the parent's doorbell wakes you, re-read your file and scan for new `[DIRECTIVE]` entries. If you find one, append `- [...] unblocked, continuing`, set `**State:** in_progress`, and resume. If there's no new directive, just idle again.

### 3. Finish

1. Push your branch.
2. Follow any wrap-up instructions in your assignment (e.g. whether to open a PR or leave that to the parent).
3. Check the parent's `../index.md` "How we work → Reviews" subsection. **The default is human-in-the-loop review** — only use the autonomous path if the index explicitly opts out.
   - **Default (human-in-the-loop)**: enter the `awaiting_review` lifecycle from `_shared/coord.md`: append `- [...] awaiting_review: <PR link>`, set `**State:** awaiting_review`, ring the parent's doorbell, and **stay alive**. Return control and idle. Two channels are live now: review feedback comes through chat from the human; chain-wide `[DIRECTIVE]` entries (rebase, abort, etc.) come from the parent and wake you via the doorbell. Stay in `awaiting_review` through the entire review cycle, even while addressing feedback and pushing changes — and don't ring the parent again until the PR settles. Only transition to `done` (and close) once the PR has merged.
   - **Autonomous (only if the index says so)**: append `- [...] done`, set `**State:** done`, ring the parent's doorbell, and close your tab via `close_tab`.

If you can't complete the task, append `- [...] failed: <reason>`, set `**State:** failed`, ring the parent's doorbell, and close your tab. Ring before you close — once the tab is gone you can't report.

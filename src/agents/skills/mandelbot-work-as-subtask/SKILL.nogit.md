---
name: mandelbot-work-as-subtask
description: Use this skill when your prompt references a coordination file and assigns you a task. You are a subtask agent — part of a larger coordinated effort. Follow the protocol to read your assignment, draft a plan for parent review, wait for approval, do your work, and report progress.
allowed-tools: [Read, Edit, Write, Bash, Glob, Grep]
---

# Work as a Subtask

You have been spawned as a child agent in a coordinated multi-agent workflow. Your prompt includes an absolute path to **your own `*.coord.md` file**. That coord file is your single source of truth and your only coordination channel with the parent.

Read `<plugin-dir>/skills/_shared/coord.md` for the protocol: directory layout, ownership rules, state vocabulary, log format, `[DIRECTIVE]` marker, plan-review and block/unblock handshakes, watcher usage, and sub-delegation. This SKILL file only covers the child-specific workflow; everything else lives in the shared doc.

**Two rules to internalize before anything else:**

- **You read only your own `*.coord.md`** and files it explicitly references by path. Never `../index.md`, never a sibling.
- **You write only to your own `*.coord.md`** (append-only log; never edit existing entries, including the parent's `[DIRECTIVE]` entries) and to the code/data files your `## Assignment` gives you ownership of.

## Shared working directory

**You share the working directory with other agents.** There is no VCS isolation — changes you make to files are immediately visible to (and can conflict with) other agents running in parallel.

- Only modify files that your task owns. Your `## Assignment` section should state which files are yours. If it's unclear, use the block/unblock handshake — don't race.
- Do not modify files outside your task's scope, even to "fix" something you notice.

## Workflow

### 1. Read your file and the governing plan

Read your `*.coord.md`. Note the `## Assignment` section — it contains your instructions, file-ownership boundaries, and any paths you need (typically the governing plan). Read the governing plan in full.

### 2. Draft your subplan

**Do not enter plan mode.** Claude's built-in plan mode can only be exited via `ExitPlanMode`, which blocks on user approval — but in this workflow the *parent agent* reviews your subplan, not the user. Instead, just write the subplan document directly.

Pick a descriptive filename and use the `Write` tool to create `~/.claude/plans/<name>.md`. The document should cover context, approach, files to change, and verification. Your subplan may itself describe sub-delegation — that's fine.

### 3. Record the plan and await review

Edit your `*.coord.md`:

- Set `**Plan:**` to the absolute subplan path.
- Set `**State:** awaiting_review`.
- Append `- [YYYY-MM-DD HH:MM] plan drafted at <path>, awaiting review`.

### 4. Watch for a directive

Run the watcher against your own file in the background (see `_shared/coord.md` for the exact invocation). When it wakes, re-read your file and scan for any new `[DIRECTIVE]` entries:

- **`[DIRECTIVE] approved`** — append `- [...] approved, starting implementation`, set `**State:** in_progress`, proceed to step 5.
- **Redline directive** — address it (may involve rewriting your subplan and updating `**Plan:**`), append a log entry describing what you changed, stay in `awaiting_review`, re-arm the watcher.
- **No new directive** — your own edits may have woken the watcher. Re-arm it and wait.

Do not start implementation until you see an approval directive.

### 5. Implement

Do the work within your file-ownership boundaries. Append log entries on state changes, not on a timer. If you get stuck on something only the parent can resolve — including a file-ownership conflict with a sibling — use the block/unblock handshake from `_shared/coord.md`.

### 6. Finish

Append `- [...] done` and set `**State:** done`. If you can't complete the task, append `- [...] failed: <reason>` and set `**State:** failed`.

There are no branches to push or PRs to open in this workflow — the parent reviews your work in place.

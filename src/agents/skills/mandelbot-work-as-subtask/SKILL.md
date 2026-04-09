---
name: mandelbot-work-as-subtask
description: Use this skill when your prompt references a coordination file and assigns you a task. You are a subtask agent — part of a larger coordinated effort. Follow the protocol to read your assignment, draft a plan for parent review, wait for approval, do your work, and report progress.
allowed-tools: [Read, Edit, Write, Bash, Glob, Grep]
---

# Work as a Subtask

You have been spawned as a child agent in a coordinated multi-agent workflow. Your prompt includes an absolute path to **your own `*.coord.md` file** and a **branch name**. That coord file is your single source of truth and your only coordination channel with the parent.

## What you can and cannot read

**You read only your own `*.coord.md` and files it explicitly references by path.** That typically means the governing plan, code files mentioned in your assignment, and your own subplan after you draft it.

You do **not** read `../index.md`, sibling `*.coord.md` files, or anything else in the coordination tree. The parent reconciles siblings; you just do your job.

## What you can and cannot write

**You only write to your own `*.coord.md`** (and, if you sub-delegate, to files inside your own `*.coord/` subtree that you create).

- The log is **append-only**. Never edit or delete existing entries — including `[DIRECTIVE]` entries the parent appended.
- Update `**State:**` in the header together with the log entry that changes state. They must agree.
- Never touch anything outside your own file.

## State vocabulary

`pending`, `planning`, `awaiting_review`, `in_progress`, `blocked`, `done`, `failed`.

Log entries are markdown bullets: `- [YYYY-MM-DD HH:MM] <text>`. Run `date '+%Y-%m-%d %H:%M'` when you need a timestamp.

## Worktree and branch

You are running in your own git worktree — an isolated copy of the repository. All repo changes happen inside this worktree. Do not `cd` out. You may also write to `/tmp`, `~/.mandelbot` (only your own coord file), and `~/.claude/plans/` (your subplan).

You own exactly one branch. Before any real work:

```bash
git checkout -b <branch-name>   # or: git checkout -b <branch-name> <base>
```

All your commits go on this branch.

## Workflow

### 1. Read your file and the governing plan

Read your `*.coord.md`. Note the `## Assignment` section — it contains your instructions and any paths you need (typically the governing plan). Read the governing plan in full.

### 2. Enter plan mode and draft your subplan

Use Claude's normal plan mode. Write your subplan to `~/.claude/plans/<name>.md`. Your subplan may itself describe sub-delegation — that's fine.

### 3. Record the plan and await review

Edit your `*.coord.md`:

- Set `**Plan:**` to the absolute subplan path.
- Set `**State:** awaiting_review`.
- Append a log entry: `- [YYYY-MM-DD HH:MM] plan drafted at <path>, awaiting review`.

### 4. Watch for a directive

Run the watcher against your own file in the background:

```bash
# Run with run_in_background: true
bash <plugin-dir>/skills/mandelbot-delegate/watch.sh <absolute path to your *.coord.md>
```

When the watcher wakes, re-read your file and scan for any new `[DIRECTIVE]` entries the parent appended:

- **`[DIRECTIVE] approved`** (or similar) — append `- [...] approved, starting implementation`, set `**State:** in_progress`, proceed to step 5.
- **Redline directive** — address it (may involve re-planning and updating `**Plan:**`), append a log entry describing what you changed, stay in `awaiting_review`, re-arm the watcher.
- **No new directive** — your own edits may have woken the watcher. Re-arm it and wait.

Do not start implementation until you see an approval directive.

### 5. Implement

Do the work. Append log entries on state changes, not on a timer.

If you get stuck on something only the parent can resolve:

1. Append `- [...] blocked: <question>`.
2. Set `**State:** blocked`.
3. Re-arm the watcher in the background.
4. When the parent replies with `- [...] [DIRECTIVE] <answer>`, append `- [...] unblocked, continuing`, set `**State:** in_progress`, and resume.

If the governing plan's format itself can't accommodate something you need, use the same `blocked` mechanism — append a question, wait, do not silently deviate.

### 6. Finish

1. Push your branch.
2. Check the `**Workflow:**` field in the parent's `index.md` — wait, you don't read that. The workflow should have been stated in your `## Assignment` text. If it says:
   - **`multi-pr`** — create a PR for your branch and include the URL in a log entry.
   - **`single-pr`** — do not create a PR; the parent will merge your branch.
3. Append `- [...] done` and set `**State:** done`.

If you can't complete the task, append `- [...] failed: <reason>` and set `**State:** failed`.

## Sub-delegation

If your subplan requires spawning your own children, you become a parent of your own slice. Promote your `*.coord.md` into a sibling `*.coord/` directory at the same path (move the file inside as `index.md` or rewrite — follow the `mandelbot-delegate` skill one level deeper). Your own parent still watches your original path, which is now inside the new directory; its directory watcher will catch changes at any depth.

## Rules recap

- Read only your own `*.coord.md` and files it explicitly references by path.
- Write only to your own `*.coord.md` (and your own `*.coord/` subtree if you sub-delegate).
- Log is append-only. `**State:**` mirrors the latest state.
- Do not start implementation before an approval directive.
- Watcher is your only polling mechanism — don't read your file on a timer.

---
name: mandelbot-work-as-subtask
description: Use this skill when your prompt references a coordination file and assigns you a task. You are a subtask agent — part of a larger coordinated effort. Follow the protocol to read your assignment, draft a plan for parent review, wait for approval, do your work, and report progress.
allowed-tools: [Read, Edit, Write, Bash, Glob, Grep]
---

# Work as a Subtask

You have been spawned as a child agent in a coordinated multi-agent workflow. Your prompt includes an absolute path to **your own `*.coord.md` file**. That coord file is your single source of truth and your only coordination channel with the parent.

## Shared working directory

**You share the working directory with other agents.** There is no VCS isolation — changes you make to files are immediately visible to (and can conflict with) other agents running in parallel.

- Only modify files that your task owns. Your `## Assignment` section should state which files are yours. If it's unclear, append a `blocked: <question>` entry and wait for a directive.
- Do not modify files outside your task's scope, even to "fix" something you notice.
- If you need to touch a file another task might also touch, raise it as a block rather than racing.

## What you can and cannot read

**You read only your own `*.coord.md` and files it explicitly references by path.** That typically means the governing plan, code files mentioned in your assignment, and your own subplan after you draft it.

You do **not** read `../index.md`, sibling `*.coord.md` files, or anything else in the coordination tree.

## What you can and cannot write

**You only write to your own `*.coord.md`** (and, if you sub-delegate, to files inside your own `*.coord/` subtree that you create) plus the code/data files your assignment gives you ownership of.

- The log is **append-only**. Never edit or delete existing entries — including `[DIRECTIVE]` entries the parent appended.
- Update `**State:**` in the header together with the log entry that changes state. They must agree.
- Never touch another child's coord file.

## State vocabulary

`pending`, `planning`, `awaiting_review`, `in_progress`, `blocked`, `done`, `failed`.

Log entries are markdown bullets: `- [YYYY-MM-DD HH:MM] <text>`. Run `date '+%Y-%m-%d %H:%M'` when you need a timestamp.

## Workflow

### 1. Read your file and the governing plan

Read your `*.coord.md`. Note the `## Assignment` section — it contains your instructions, file-ownership boundaries, and any paths you need (typically the governing plan). Read the governing plan in full.

### 2. Draft your subplan

**Do not enter plan mode.** Claude's built-in plan mode can only be exited via `ExitPlanMode`, which blocks on user approval — but in this workflow the *parent agent* reviews your subplan, not the user. Instead, just write the subplan document directly.

Pick a descriptive filename and use the `Write` tool to create `~/.claude/plans/<name>.md`. The document should cover context, approach, files to change, and verification — the same things a plan-mode plan would contain. Your subplan may itself describe sub-delegation — that's fine.

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

Do the work within your file-ownership boundaries. Append log entries on state changes, not on a timer.

If you get stuck on something only the parent can resolve — including a file-ownership conflict with a sibling — use the block mechanism:

1. Append `- [...] blocked: <question>`.
2. Set `**State:** blocked`.
3. Re-arm the watcher in the background.
4. When the parent replies with `- [...] [DIRECTIVE] <answer>`, append `- [...] unblocked, continuing`, set `**State:** in_progress`, and resume.

If the format itself can't accommodate something you need, use the same mechanism — append a question, wait, do not silently deviate.

### 6. Finish

Append `- [...] done` and set `**State:** done`. If you can't complete the task, append `- [...] failed: <reason>` and set `**State:** failed`.

There are no branches to push or PRs to open in this workflow — the parent reviews your work in place.

## Sub-delegation

If your subplan requires spawning your own children, you become a parent of your own slice. Promote your `*.coord.md` into a sibling `*.coord/` directory at the same path and follow the `mandelbot-delegate` skill one level deeper. Your own parent still watches your original path, which is now inside the new directory; its directory watcher will catch changes at any depth.

## Rules recap

- Read only your own `*.coord.md` and files it explicitly references by path.
- Write only to your own `*.coord.md` and the code files your assignment owns.
- Log is append-only. `**State:**` mirrors the latest state.
- Do not start implementation before an approval directive.
- Watcher is your only polling mechanism — don't read your file on a timer.

---
name: mandelbot-delegate
description: Use this skill when you have work that can be broken into parallel subtasks and delegated to child agents. Activates when you need to coordinate multiple agents working on different parts of a plan simultaneously.
allowed-tools: [Read, Edit, Write, Bash, Glob, Grep, mcp__mandelbot__spawn_tab]
---

# Delegate to Subtasks

Use this skill to break parallelizable work into subtasks, spawn a child agent for each, review the subplan they draft, and monitor their progress — all via a shared `*.coord/` coordination directory.

You are the **parent**. Each child owns its own file inside the coordination directory. You own the `index.md` at the top of that directory. You may append `[DIRECTIVE]` entries into any child's file, but you never edit existing entries and never touch siblings' state fields.

## The coordination directory

One project = one `*.coord/` directory under `~/.mandelbot/coordination/`:

```
~/.mandelbot/coordination/<project>.coord/
  index.md                    # parent-owned — goal, plan link, workflow, how-we-work, children roster
  <label>.coord.md            # parent-created, then child-owned — plan link, state, append-only log
  <other-label>.coord.md
  <nested>.coord/             # present only if a child sub-delegated
    index.md
    ...
```

Templates live alongside this skill: `index.template.md` and `child.template.md`.

State vocabulary (used in the `**State:**` header and as the leading word of log entries where applicable): `pending`, `planning`, `awaiting_review`, `in_progress`, `blocked`, `done`, `failed`.

Log entries are markdown bullets timestamped `- [YYYY-MM-DD HH:MM] <text>`. They are append-only — never edit or delete an existing entry. The `**State:**` header duplicates the latest state for fast scanning; update it together with the log entry that changes state.

## Workflow

### 1. Plan your work

Use Claude's normal planning mechanism. Note the plan path (typically `~/.claude/plans/<name>.md`).

### 2. Choose a workflow

- **`single-pr`** — Cohesive changes that should land as one PR. Children push branches but do not open PRs; you merge them into an integration branch and open one PR at the end.
- **`multi-pr`** — Independent enough that each child opens its own PR. You coordinate; children own review.

Default to `single-pr`. Pick `multi-pr` when the pieces are unrelated or the combined diff would be too large for one review.

### 3. Create the coordination directory

```bash
mkdir -p ~/.mandelbot/coordination/<project>.coord
```

Write `index.md` from `<plugin-dir>/skills/mandelbot-delegate/index.template.md`. Fill in:
- Project name, absolute plan path, workflow, `**State:** in_progress`.
- **How we work**: a short "tech lead memo" for this batch. At minimum, point children at the governing plan, tell them to enter plan mode, draft a subplan, transition to `awaiting_review`, watch their own file, and wait for `[DIRECTIVE] approved` before implementing. Add anything flow-specific.
- **Children**: one bullet per child, all `pending`.

Then for each child, write `<label>.coord.md` from `child.template.md`:
- `**Parent:** ../index.md`
- `**Plan:** <to be filled in by child after planning>`
- `**State:** pending`
- An `## Assignment` section with the child's instructions **inline**. Include any absolute paths the child needs (governing plan, relevant files). Children only read their own `*.coord.md` and files it explicitly references, so be explicit.
- An empty `## Log` section.

Labels should be short identifiers (a few words) matching back to the plan.

### 4. Spawn child agents

For each child, call `spawn_tab` with a `branch` parameter (worktree/branch name) and a `prompt` like:

> Start by running `/mandelbot-work-as-subtask` to load the subtask protocol. You are a child agent in the "<project>" project. Your coordination file is at `<absolute path to <label>.coord.md>` — read it first, then read the governing plan it references at `<absolute path to plan>` in full.
>
> Your job: <one-line summary>. Draft your subplan directly into `~/.claude/plans/` (do **not** enter plan mode — use the Write tool), update your coord file's Plan field and log, set state to `awaiting_review`, and wait on the watcher for a `[DIRECTIVE] approved` entry before implementing.

Include: instruction to run `/mandelbot-work-as-subtask` first, absolute path to the child's own `*.coord.md`, absolute path to the governing plan, branch name via the `branch` param, an explicit mention of the plan-review handshake, and an explicit "do not enter plan mode" note — plan mode's only exit is `ExitPlanMode`, which blocks on user approval and would stall the parent-review handshake.

### 5. Watch, review, direct

Monitor progress with the directory watcher. It blocks until *any* file in the tree changes, prints the changed paths and their contents, then exits. **Run it in the background** with `run_in_background: true`.

```bash
bash <plugin-dir>/skills/mandelbot-delegate/watch.sh ~/.mandelbot/coordination/<project>.coord
```

When the watcher wakes, inspect its output and act:

- **Child in `awaiting_review`** — read the subplan file it links to, review it against the governing plan and your intent, then append one of the following directly into that child's `*.coord.md` log:
  - `- [YYYY-MM-DD HH:MM] [DIRECTIVE] approved, proceed`
  - `- [YYYY-MM-DD HH:MM] [DIRECTIVE] <specific redline>` — the child will address and re-submit.

- **New `blocked: <question>` entry** — answer with `- [YYYY-MM-DD HH:MM] [DIRECTIVE] <answer>` in that child's file.

- **State change elsewhere** — update the Children roster in `index.md` to mirror the child's current state. This is parent-owned bookkeeping and is the only editing you do to `index.md` during the run.

After handling any updates, re-arm the watcher in the background. The watcher is your **only** polling mechanism — don't read child files on a timer.

**Rules when writing into a child's file:**
- Only append. Never edit or delete existing entries.
- Always use the `[DIRECTIVE]` marker — children scan for it.
- Never touch a child's `**State:**` field. The child owns that.

### 6. Finalize

When every child is `done` or `failed`:

1. Handle failures (retry, reassign, or escalate to the user).
2. Append a final entry to `index.md` noting completion, and set its `**State:**` to `done` (or `failed` if any child failed unrecoverably).
3. **`multi-pr`**: you're done — children own their PRs. Report the list to the user and stop.
4. **`single-pr`**: create an integration branch off the base, `git merge --no-ff <child-branch>` for each child, resolve conflicts, push, and open one PR covering all the work.

## Sub-delegation by children

If a child decides to spawn its own children, it becomes a parent in its own right: it promotes its `*.coord.md` into a sibling `*.coord/` directory at the same path, writes its own `index.md`, and follows this skill recursively one level deeper. The directory watcher catches changes at any depth — you do nothing special.

## Legacy single-file artifacts

This skill used to write flat `~/.mandelbot/coordination/<name>.md` files. Those are obsolete but left in place — the new format only creates `<name>.coord/` directories, so there's no name collision, and the old files are inert once nothing reads them. Leave them alone unless the user asks you to clean up.

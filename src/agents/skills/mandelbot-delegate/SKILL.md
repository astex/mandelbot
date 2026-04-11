---
name: mandelbot-delegate
description: Use this skill when you have work that can be broken into parallel subtasks and delegated to child agents. Activates when you need to coordinate multiple agents working on different parts of a plan simultaneously.
allowed-tools: [Read, Edit, Write, Bash, Glob, Grep, mcp__mandelbot__spawn_tab]
---

# Delegate to Subtasks

Use this skill to break parallelizable work into subtasks, spawn a child agent for each, review the subplan they draft, and monitor their progress — all via a shared `*.coord/` coordination directory.

You are the **parent**. Read `<plugin-dir>/skills/_shared/coord.md` for the protocol: directory layout, ownership rules, state vocabulary, log format, `[DIRECTIVE]` marker, plan-review and block/unblock handshakes, watcher usage, and sub-delegation. This SKILL file only covers the parent-specific workflow; everything else lives in the shared doc.

## Workflow

### 1. Plan your work

Use your normal planning mechanism. Note the plan path (typically `~/.claude/plans/<name>.md`).

### 2. Choose a workflow

- **`single-pr`** — Cohesive changes that should land as one PR. Children push branches but do not open PRs; you merge them into an integration branch and open one PR at the end.
- **`multi-pr`** — Independent enough that each child opens its own PR. You coordinate; children own review.

Default to `single-pr`. Pick `multi-pr` when the pieces are unrelated or the combined diff would be too large for one review.

### 3. Create the coordination directory

```bash
mkdir -p ~/.mandelbot/coordination/<project>.coord
```

Write `index.md` from `<plugin-dir>/skills/_shared/index.template.md`. Fill in:
- Project name, absolute plan path, workflow.
- **How we work**: a short "tech lead memo" for this batch. At minimum, point children at the governing plan and the plan-review handshake. Add anything flow-specific.
- **Children**: one bullet per child, all `pending`.

Then for each child, write `<label>.coord.md` from `child.template.md`:
- `**Parent:** ../index.md`
- `**Plan:** <to be filled in by child after planning>`
- `**State:** pending`
- An `## Assignment` section with the child's instructions **inline**. Include any absolute paths the child needs (governing plan, relevant files) — the child only reads its own `*.coord.md` and files it explicitly references, so be explicit.
- An empty `## Log` section.

Labels should be short identifiers (a few words) matching back to the plan.

### 4. Spawn child agents

For each child, call `spawn_tab` with a `branch` parameter (worktree/branch name) and a `prompt` like:

> Start by running `/mandelbot-work-as-subtask` to load the subtask protocol. You are a child agent in the "<project>" project. Your coordination file is at `<absolute path to <label>.coord.md>` — read it first, then read the governing plan it references at `<absolute path to plan>` in full.
>
> Your job: <one-line summary>. Draft your subplan directly into `~/.claude/plans/` (do **not** enter plan mode — use the Write tool), update your coord file's Plan field and log, set state to `awaiting_review`, and wait on the watcher for a `[DIRECTIVE] approved` entry before implementing.

Include: instruction to run `/mandelbot-work-as-subtask` first, absolute path to the child's own `*.coord.md`, absolute path to the governing plan, branch name via the `branch` param, an explicit mention of the plan-review handshake, and an explicit "do not enter plan mode" note — plan mode's only exit is `ExitPlanMode`, which blocks on user approval and would stall the parent-review handshake.

### 5. Watch, review, direct

For each child, run a separate watcher against that child's `*.coord.md` in the background (one watcher per child — see `_shared/coord.md` for the invocation). When a watcher wakes, inspect the child's file and act:

- **Child in `awaiting_review`** — read the subplan it links to, review against the governing plan and your intent, append `- [...] [DIRECTIVE] approved, proceed` or `- [...] [DIRECTIVE] <revision request>` directly into that child's `*.coord.md` log.
- **New `blocked: <question>` entry** — append `- [...] [DIRECTIVE] <answer>` in that child's file.
Then re-arm that child's watcher in the background. (See `_shared/coord.md` for the append-only rules for writing into child files.)

### 6. Finalize

When every child is `done` or `failed`:

1. Handle failures (retry, reassign, or escalate to the user).
2. Append a final entry to `index.md` noting completion.
3. **`multi-pr`**: you're done — children own their PRs. Report the list to the user and stop.
4. **`single-pr`**: create an integration branch off the base, `git merge --no-ff <child-branch>` for each child, resolve conflicts, push, and open one PR covering all the work.

---
name: mandelbot-delegate
description: Use this skill when you have work that can be broken into parallel subtasks and delegated to child agents. Activates when you need to coordinate multiple agents working on different parts of a plan simultaneously.
allowed-tools: [Read, Edit, Write, Bash, Glob, Grep, ListAgents, SendMessage, mcp__mandelbot__spawn_tab, mcp__mandelbot__close_tab]
---

# Delegate to Subtasks

Use this skill to break parallelizable work into subtasks, spawn a child agent for each, and monitor their progress — all via a shared `*.coord/` coordination directory.

**This project uses git-based VCS isolation.** Each child agent runs in its own worktree on its own branch. Children do not conflict with each other's files.

You are the **parent**. Read `<plugin-dir>/skills/_shared/coord.md` for the protocol: directory layout, ownership rules, state vocabulary, log format, `[DIRECTIVE]` marker, block/unblock handshake, the doorbell and addressing, tab lifecycle, and sub-delegation. This SKILL file only covers the parent-specific workflow; everything else lives in the shared doc.

## Workflow

### 1. Plan your work

Use your normal planning mechanism. Note the plan path (typically `~/.claude/plans/<name>.md`).

### 2. Create the coordination directory

```bash
mkdir -p ~/.mandelbot/coordination/<project>.coord
```

Write `index.md` from `<plugin-dir>/skills/_shared/index.template.md`. Fill in:
- Project name, absolute plan path.
- **How we work**: a short "tech lead memo" for this batch. At minimum, point children at the governing plan. Add anything flow-specific — for example, whether children should open their own PRs or leave that to you, branching conventions, file-ownership boundaries, etc. **Reviews default to human-in-the-loop**: children open PRs, set `awaiting_review`, and stay alive for the human to review and merge. Only override if this project explicitly wants autonomous merging (children closing on `done`). See the awaiting_review section in `_shared/coord.md`.
- **Children**: one bullet per child.

Then for each child, write `<child>.coord.md` from `child.template.md`:
- `**Parent:** ../index.md`
- `**State:** pending`
- An `## Assignment` section with the child's instructions **inline**. Include any absolute paths the child needs (governing plan, relevant files) — the child only reads its own `*.coord.md` and files it explicitly references, so be explicit.
- An empty `## Log` section.

Labels should be short identifiers (a few words) matching back to the plan.

### 3. Spawn child agents

For each child, call `spawn_tab` with a `branch` parameter (worktree/branch name) and a `prompt` like:

> Start by running `/mandelbot-work-as-subtask` to load the subtask protocol. You are a child agent in the "<project>" project. Your coordination file is at `<absolute path to <child>.coord.md>` — read it first, then read the governing plan it references at `<absolute path to plan>` in full.
>
> Your job: <one-line summary>.

Include: instruction to run `/mandelbot-work-as-subtask` first, absolute path to the child's own `*.coord.md`, absolute path to the governing plan, and branch name via the `branch` param.

Spawn children **one at a time**, completing step 4 for each before spawning the next.

### 4. Resolve the child's address and say hello

Run `ListAgents` before and after the spawn — the new row is the child (its name should be the branch name plus a short suffix). Write that name into the child's coord file as `**Session:** <name>`.

Then send it a hello doorbell:

```
SendMessage(to: "<child session name>", message: "coord update: <absolute path to child's coord file>")
```

This is not optional politeness — it's how the child learns *your* address, from the `from` attribute on the message it receives. A child that never gets a hello can never report back. See "The doorbell" in `_shared/coord.md`.

### 5. Direct

You now idle until a child rings your doorbell. Do not poll, and do not run a watcher — there isn't one anymore.

When a doorbell arrives, read the coord file it names and act:

- **New `blocked: <question>` entry** — append `- [...] [DIRECTIVE] <answer>` in that child's file, then ring that child's doorbell. Writing without ringing leaves the child idle forever.

(See `_shared/coord.md` for the append-only rules for writing into child files.)

### 6. Finalize

When every child has reached a settled state — `awaiting_review` (the default), `done` (in autonomous-review projects), or `failed` — handle failures (retry, reassign, or escalate) and wrap up however is appropriate for this project: merge branches, open PRs, report results, etc.

Children in `awaiting_review` are mid-PR-review and will close themselves once their PRs merge — leave their tabs alone. Close any other remaining child tabs via `close_tab`.

If something chain-wide changes after children reach `awaiting_review` — an upstream merge that forces a rebase across the whole chain, a decision to abort a PR, a sibling's approach shifting in a way that affects others — append a `[DIRECTIVE]` into the relevant child's coord file and ring its doorbell. The child idles through review and wakes on that. Reserve this for things only you can coordinate across siblings; direct code-review feedback on a PR flows through the tab's chat, not here.

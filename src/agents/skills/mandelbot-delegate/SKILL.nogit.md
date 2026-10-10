---
name: mandelbot-delegate
description: Use this skill when you have work that can be broken into parallel subtasks and delegated to child agents. Activates when you need to coordinate multiple agents working on different parts of a plan simultaneously.
allowed-tools: [Read, Edit, Write, Bash, Glob, Grep, ListAgents, SendMessage, mcp__mandelbot__spawn_tab, mcp__mandelbot__close_tab]
---

# Delegate to Subtasks

Use this skill to break parallelizable work into subtasks, spawn a child agent for each, and monitor their progress — all via a shared `*.coord/` coordination directory.

**Warning:** This project is not using git-based isolation. All child agents share the same working directory. Design task boundaries carefully to avoid file conflicts — ideally each task touches a disjoint set of files, and each child's `## Assignment` spells out which files it owns.

You are the **parent**. Read `<plugin-dir>/skills/_shared/coord.md` for the protocol: directory layout, ownership rules, state vocabulary, log format, `[DIRECTIVE]` marker, block/unblock handshake, the doorbell and addressing, tab lifecycle, and sub-delegation. This SKILL file only covers the parent-specific workflow; everything else lives in the shared doc.

## Workflow

### 1. Plan your work

Use your normal planning mechanism. Note the plan path (typically `~/.claude/plans/<name>.md`).

When splitting tasks, pay special attention to file ownership — two agents editing the same file will cause conflicts. Prefer task boundaries that map to separate files or directories.

### 2. Create the coordination directory

```bash
mkdir -p ~/.mandelbot/coordination/<project>.coord
```

Write `index.md` from `<plugin-dir>/skills/_shared/index.template.md`. Fill in:
- Project name, absolute plan path.
- **How we work**: a short "tech lead memo" for this batch. At minimum, point children at the governing plan, and call out file-ownership boundaries explicitly since there is no VCS isolation.
- **Children**: one bullet per child.

Then for each child, write `<child>.coord.md` from `child.template.md`:
- `**Parent:** ../index.md`
- `**State:** pending`
- An `## Assignment` section with the child's instructions **inline**, including which files the child owns and any absolute paths it needs (governing plan, relevant files). Children only read their own `*.coord.md` and files it explicitly references, so be explicit.
- An empty `## Log` section.

Labels should be short identifiers (a few words) matching back to the plan.

### 3. Spawn child agents

For each child, call `spawn_tab` with a prompt like:

> Start by running `/mandelbot-work-as-subtask` to load the subtask protocol. You are a child agent in the "<project>" project. Your coordination file is at `<absolute path to <child>.coord.md>` — read it first, then read the governing plan it references at `<absolute path to plan>` in full.
>
> Your job: <one-line summary>.

Include: instruction to run `/mandelbot-work-as-subtask` first, absolute path to the child's own `*.coord.md`, and absolute path to the governing plan.

Spawn children **one at a time**, completing step 4 for each before spawning the next.

### 4. Resolve the child's address and say hello

Run `ListAgents` before and after the spawn — the new row is the child. Write that name into the child's coord file as `**Session:** <name>`.

Then send it a hello doorbell:

```
SendMessage(to: "<child session name>", message: "coord update: <absolute path to child's coord file>")
```

This is not optional politeness — it's how the child learns *your* address, from the `from` attribute on the message it receives. A child that never gets a hello can never report back. See "The doorbell" in `_shared/coord.md`.

### 5. Direct

You now idle until a child rings your doorbell. Do not poll, and do not run a watcher — there isn't one anymore.

When a doorbell arrives, read the coord file it names and act:

- **New `blocked: <question>` entry** — append `- [...] [DIRECTIVE] <answer>` in that child's file, then ring that child's doorbell. File-ownership conflicts in this workflow often surface as blocks; resolve them by directing which child owns the contested file. Writing without ringing leaves the child idle forever.

(See `_shared/coord.md` for the append-only rules for writing into child files.)

### 6. Finalize

When every child is `done` or `failed`, handle failures (retry, reassign, or escalate to the user) and wrap up however is appropriate for this project.

Close any remaining child tabs via `close_tab`.

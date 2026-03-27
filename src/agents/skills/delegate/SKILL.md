---
name: delegate
description: Use this skill when you have work that can be broken into parallel subtasks and delegated to child agents. Activates when you need to coordinate multiple agents working on different parts of a plan simultaneously.
allowed-tools: [Read, Edit, Write, Bash, Glob, Grep, mcp__mandelbot__spawn_tab]
---

# Delegate to Subtasks

Use this skill to break parallelizable work into subtasks, spawn child agents for each, and monitor their progress via a shared coordination file.

## Workflow

### 1. Plan your work

Use Claude's normal planning mechanism to create a plan. Note the plan file path (typically `~/.claude/plans/<name>.md`).

### 2. Create the coordination status file

```bash
mkdir -p ~/.mandelbot/coordination
```

Create a status file at `~/.mandelbot/coordination/<descriptive-name>.md` using the template in this skill's directory as a reference. The format is:

```markdown
# Coordination: <title>

**Plan:** `<path to your plan file>`

## Tasks

| # | Label | Status | Notes |
|---|-------|--------|-------|
| 1 | <short label> | pending | |
| 2 | <short label> | pending | |

## Summary
```

- **Labels** should be short identifiers (a few words), just enough to match back to the plan step. Don't duplicate the full plan text.
- All tasks start as `pending`.

### 3. Spawn child agents

For each task, call the `spawn_tab` MCP tool with a prompt like:

> You are working on task <N> from the coordination file at `~/.mandelbot/coordination/<name>.md`. Read the coordination file and the referenced plan file to understand your assignment. Update your row in the coordination file as you work.

Include:
- The **absolute path** to the coordination file
- The **task number** they are responsible for
- Instruction to read both the coordination file and the plan

### 4. Monitor progress

Use the watcher script to wait for changes to the coordination file. The script blocks until the file changes, prints the updated contents, then exits. **Run it in the background** (using `run_in_background`) so you are free to do other work while waiting. You will be notified when the file changes.

```bash
# Run with run_in_background: true
bash <plugin-dir>/skills/delegate/watch.sh ~/.mandelbot/coordination/<name>.md
```

When the watcher completes and you are notified, check its output:
- If any tasks are still `pending`, `in_progress`, or `blocked` — **run the watcher again** (in the background) to wait for the next update.
- If all tasks are `done` or `failed` — proceed to step 5.

**Important:**
- The watcher script is your **only** monitoring mechanism. Do **not** also read the coordination file manually — that duplicates work and wastes context.
- **Be patient.** Child agents take time to spin up. After spawning, the first watcher run may take a while before any agent updates. This is normal.

### 5. Review and summarize

When all tasks show `done` or `failed`:
1. Review the work each child agent produced
2. Fill in the **Summary** section of the coordination file
3. Handle any failed tasks (retry, reassign, or escalate)

## Status Values

| Status | Meaning |
|--------|---------|
| `pending` | Not yet started |
| `in_progress` | Agent is actively working |
| `done` | Work complete |
| `blocked` | Waiting on another task or external input |
| `failed` | Could not complete; see notes |

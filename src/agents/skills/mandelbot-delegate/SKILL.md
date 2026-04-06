---
name: mandelbot-delegate
description: Use this skill when you have work that can be broken into parallel subtasks and delegated to child agents. Activates when you need to coordinate multiple agents working on different parts of a plan simultaneously.
allowed-tools: [Read, Edit, Write, Bash, Glob, Grep, mcp__mandelbot__spawn_tab]
---

# Delegate to Subtasks

Use this skill to break parallelizable work into subtasks, spawn child agents for each, and monitor their progress via a shared coordination file.

## Workflow

### 1. Plan your work

Use Claude's normal planning mechanism to create a plan. Note the plan file path (typically `~/.claude/plans/<name>.md`).

### 2. Choose your workflow

Pick a workflow based on the scope of the changes:

- **`single-pr`** — The changes are cohesive and should land as one PR. Children work in branches and push them, but do **not** create PRs. When all children finish, you (the parent) create a new branch, merge their branches into it, and open one PR.

- **`multi-pr`** — The changes are large or independent enough to warrant separate PRs. Each child creates and owns its own PR. You coordinate the overall effort but children are responsible for getting their PRs through review.

Use `single-pr` by default. Use `multi-pr` when the work spans unrelated areas, when individual pieces should be reviewable independently, or when the combined diff would be too large for a single review.

### 3. Create the coordination status file

```bash
mkdir -p ~/.mandelbot/coordination
```

Create a status file at `~/.mandelbot/coordination/<descriptive-name>.md` using the template at `<plugin-dir>/skills/delegate/template.md` as a starting point. Fill in the title, plan path, **workflow**, and one row per task.

- **Labels** should be short identifiers (a few words), just enough to match back to the plan step. Don't duplicate the full plan text.
- All tasks start as `pending`.

### 4. Spawn child agents

For each task, call the `spawn_tab` MCP tool with a **`branch`** parameter (the branch name for this subtask's worktree) and a **`prompt`** like:

> Start by running `/mandelbot-work-as-subtask` to load the subtask protocol. You are working on task <N> from the coordination file at `~/.mandelbot/coordination/<name>.md`. Read the coordination file and the referenced plan file to understand your assignment. Update your row in the coordination file as you work.

Include:
- Instruction to run `/mandelbot-work-as-subtask` **first**
- The **absolute path** to the coordination file
- The **task number** they are responsible for
- The **branch name** via the `branch` parameter (also used as the git worktree name)
- Instruction to read both the coordination file and the plan

### 5. Next steps

What happens next depends on the workflow.

#### multi-pr

Your work is done. Each child agent owns its PR and the user will interact with them directly. Report the list of spawned tasks to the user and stop.

If the user later asks you to spawn additional tasks, add rows to the coordination file and spawn new child agents as in step 4.

#### single-pr

Monitor progress using the watcher script. It blocks until the coordination file changes, prints the updated contents, then exits. **Run it in the background** (using `run_in_background`) so you are free to do other work while waiting. You will be notified when the file changes.

```bash
# Run with run_in_background: true
bash <plugin-dir>/skills/delegate/watch.sh ~/.mandelbot/coordination/<name>.md
```

When the watcher completes and you are notified, check its output:
- If any tasks are still `pending`, `in_progress`, or `blocked` — **run the watcher again** (in the background) to wait for the next update.
- If all tasks are `done` or `failed` — proceed to finalize.

**Important:**
- The watcher script is your **only** monitoring mechanism. Do **not** also read the coordination file manually — that duplicates work and wastes context.
- **Be patient.** Child agents take time to spin up. After spawning, the first watcher run may take a while before any agent updates. This is normal.

When all tasks show `done` or `failed`:

1. Handle any failed tasks (retry, reassign, or escalate to the user).
2. Fill in the **Summary** section of the coordination file.
3. Create a new integration branch off the base branch.
4. Merge each child's branch into it (e.g., `git merge --no-ff <child-branch>`). Resolve any conflicts.
5. Push the integration branch and open a PR that covers all the work.

## Status Values

| Status | Meaning |
|--------|---------|
| `pending` | Not yet started |
| `in_progress` | Agent is actively working |
| `done` | Work complete |
| `blocked` | Waiting on another task or external input |
| `failed` | Could not complete; see notes |

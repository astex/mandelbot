---
name: mandelbot-work-as-subtask
description: Use this skill when your prompt references a coordination file and assigns you a task number. You are a subtask agent — part of a larger coordinated effort. Follow the protocol to read your assignment, do your work, and report progress.
allowed-tools: [Read, Edit, Write, Bash, Glob, Grep]
---

# Work as a Subtask

You have been spawned as part of a coordinated multi-agent workflow. Your prompt includes a **coordination file path**, a **task number**, and a **branch name**. Follow this protocol.

## Worktree Isolation

You are running in your own git worktree — an isolated copy of the repository. All repository changes (code, config, etc.) must happen within this worktree. Do not `cd` to the original repository root or make changes to it. You may write to `/tmp` and to `~/.mandelbot` (e.g., the coordination status file).

## Branch Ownership

You own exactly one branch. Before starting any work:

```bash
git checkout -b <branch-name>            # or, if a base branch was specified:
git checkout -b <branch-name> <base>
```

All of your commits go on this branch. When your work is complete, push the branch and create a PR for it unless your prompt explicitly says otherwise.

## Workflow

### 1. Read the coordination file

Read the coordination file referenced in your prompt. It contains:
- A **Plan** link pointing to the full plan file
- A **Tasks** table with your row identified by task number

Read the plan file too — it has the full details of your assignment.

### 2. Mark yourself in progress

Update your row in the coordination file's task table, changing your status from `pending` to `in_progress`:

```
| <N> | <label> | in_progress | Starting work |
```

Use the Edit tool to modify only your row. Do not touch other rows.

### 3. Do your work

Implement your assigned task. As you make meaningful progress, update the **Notes** column in your row with a brief summary of where you are.

### 4. Report completion

When done, update your row's status to `done`:

```
| <N> | <label> | done | <brief summary of what was done> |
```

### 5. Handle blockers

If you are blocked (e.g., waiting on another task's output):
1. Set your status to `blocked` with a note explaining what you're waiting for
2. Periodically re-read the coordination file to check if the blocker has resolved
3. Once unblocked, set status back to `in_progress` and continue

If you cannot complete your task, set status to `failed` with a note explaining why.

## Rules

- Only modify **your own row** in the task table (identified by your task number).
- Keep notes concise — a few words to a sentence.
- Status values: `pending`, `in_progress`, `done`, `blocked`, `failed`.
- Do not modify the Plan link, other tasks' rows, or the Summary section.

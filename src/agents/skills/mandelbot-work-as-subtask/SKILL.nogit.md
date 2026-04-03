---
name: mandelbot-work-as-subtask
description: Use this skill when your prompt references a coordination file and assigns you a task number. You are a subtask agent — part of a larger coordinated effort. Follow the protocol to read your assignment, do your work, and report progress.
allowed-tools: [Read, Edit, Write, Bash, Glob, Grep]
---

# Work as a Subtask

You have been spawned as part of a coordinated multi-agent workflow. Your prompt includes a **coordination file path** and a **task number**. Follow this protocol.

## Shared Working Directory

**You share the working directory with other agents.** There is no VCS isolation — changes you make to files are immediately visible to (and can conflict with) other agents running in parallel.

- Only modify files that your task owns. If your assignment doesn't make file ownership clear, read the plan carefully and stick to the files implied by your task.
- Do not modify files outside your task's scope, even to "fix" something you notice.
- If you need to modify a file that another task might also touch, note this in your coordination file row and set your status to `blocked`.

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

If you are blocked (e.g., waiting on another task's output, or a file conflict):
1. Set your status to `blocked` with a note explaining what you're waiting for
2. Use the watcher script to wait for changes to the coordination file. The script blocks until the file changes, prints the updated contents, then exits. **Run it in the background** (using `run_in_background`) so you are free to do other work while waiting. You will be notified when the file changes.

   ```bash
   # Run with run_in_background: true
   bash <plugin-dir>/skills/mandelbot-delegate/watch.sh <coordination-file>
   ```

   When the watcher completes and you are notified, check its output:
   - If your blocker is not yet resolved — **run the watcher again** (in the background) to wait for the next update.
   - If the blocker is resolved — set status back to `in_progress` and continue.

If you cannot complete your task, set status to `failed` with a note explaining why.

## Rules

- Only modify **your own row** in the task table (identified by your task number).
- Keep notes concise — a few words to a sentence.
- Status values: `pending`, `in_progress`, `done`, `blocked`, `failed`.
- Do not modify the Plan link, other tasks' rows, or the Summary section.

---
name: mandelbot-work-as-subtask
description: Use this skill when your prompt references a coordination file and assigns you a task. You are a subtask agent — part of a larger coordinated effort. Follow the protocol to read your assignment, do your work, and report progress.
allowed-tools: [Read, Edit, Write, Bash, Glob, Grep, ListAgents, SendMessage, mcp__mandelbot__close_tab]
---

# Work as a Subtask

You have been spawned as a child agent in a coordinated multi-agent workflow. Your prompt includes an absolute path to **your own `*.coord.md` file**. That coord file is your single source of truth and your only coordination channel with the parent.

Read `<plugin-dir>/skills/_shared/coord.md` for the protocol: directory layout, ownership rules, state vocabulary, log format, `[DIRECTIVE]` marker, block/unblock handshake, the doorbell and addressing, tab lifecycle, and sub-delegation. This SKILL file only covers the child-specific workflow; everything else lives in the shared doc.

**Two rules to internalize before anything else:**

- **You read only your own `*.coord.md`** and files it explicitly references by path. Never `../index.md`, never a sibling.
- **You write only to your own `*.coord.md`** (append-only log; never edit existing entries, including the parent's `[DIRECTIVE]` entries) and to the code/data files your `## Assignment` gives you ownership of.

**Never poll.** Do not write a watcher, inotify script, or sleep loop against your coord file. You report by writing your file and then ringing the parent's doorbell, and you wake when the parent rings yours (or the human prompts you in chat).

## Shared working directory

**You share the working directory with other agents.** There is no VCS isolation — changes you make to files are immediately visible to (and can conflict with) other agents running in parallel.

- Only modify files that your task owns. Your `## Assignment` section should state which files are yours. If it's unclear, use the block/unblock handshake — don't race.
- Do not modify files outside your task's scope, even to "fix" something you notice.

## Workflow

### 1. Read your file and the governing plan

Read your `*.coord.md`. Note the `## Assignment` section — it contains your instructions, file-ownership boundaries, and any paths you need (typically the governing plan). Read the governing plan in full.

### 1b. Record your parent's address

Shortly after you start, a hello message arrives from the parent, wrapped as `<cross-session-message from="...">`. Copy that `from` value into your own coord file as `**Parent session:** <name>` — it's the only way you can reach the parent, and recording it means it survives compaction.

Every time you ring the parent's doorbell from here on, it's:

```
SendMessage(to: "<parent session name>", message: "coord update: <absolute path to your coord file>")
```

The message body is just a pointer. Everything the parent needs to know goes in your coord file, never in the message.

### 2. Start implementing

Set `**State:** in_progress` and append `- [...] starting implementation`. Then do the work within your file-ownership boundaries.

Append log entries on state changes, not on a timer. If you get stuck on something only the parent can resolve — including a file-ownership conflict with a sibling — use the block/unblock handshake from `_shared/coord.md`: append `- [...] blocked: <question>`, set `**State:** blocked`, **ring the parent's doorbell**, then return control and idle.

Writing the entry without ringing the doorbell strands you — nothing is watching your file. Treat the write and the ring as a single step.

When the parent's doorbell wakes you, re-read your file and scan for new `[DIRECTIVE]` entries. If you find one, append `- [...] unblocked, continuing`, set `**State:** in_progress`, and resume. If there's no new directive, just idle again.

### 3. Finish

1. Append `- [...] done` and set `**State:** done`.
2. Ring the parent's doorbell.
3. Close your tab: call `close_tab` with your own tab ID.

If you can't complete the task, append `- [...] failed: <reason>`, set `**State:** failed`, ring the parent's doorbell, and close your tab. Ring before you close — once the tab is gone you can't report.

There are no branches to push or PRs to open in this workflow — the parent reviews your work in place.

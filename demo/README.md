# Time-travel / timeline-forking spike — demo

Two demos:

## 1. Headless mechanics demo (proves both bets end-to-end)

```
bash demo/time-travel-demo.sh
```

This runs three real `claude --print` turns, creates shadow-branch
checkpoints, rewinds the worktree, truncates the JSONL, and resumes
— asserting that only the pre-checkpoint secret word is remembered.
Then it forks into a new worktree with a new session UUID and
re-verifies. Exercises the exact mechanics `src/checkpoint.rs` uses.

## 2. Live mandelbot demo

Prereq: you have `cargo run` running mandelbot.

1. `mkdir /tmp/demo-repo && cd /tmp/demo-repo && git init && echo v1 > f.txt && git add . && git commit -m init`
2. `cargo run` from this worktree.
3. In the home tab, hit `t` (spawn agent) and enter `/tmp/demo-repo`.
4. Focus the project tab, hit `t` to spawn a task tab with prompt
   (via a child agent):

```
You are a test driver. Do exactly this, in order:
1. Write "apple" to f.txt.
2. Call the mandelbot MCP tool `checkpoint` with no args. Note the
   checkpoint_id in the response.
3. Write "banana" to f.txt.
4. Call `checkpoint` again.
5. Write "cherry" to f.txt. Do NOT checkpoint.
6. Call `fork` with checkpoint_id=0 and prompt="State the current
   content of f.txt and the fruit words you remember."
7. Report what happened.
```

5. Observe: a sibling tab appears, its worktree has `f.txt` = "apple"
   (the checkpoint-0 state), and the resumed conversation matches
   where the parent was right after step 2.

### Verifying a rewind

From any checkpointed tab:

```
Call the mandelbot MCP tool `rewind` with checkpoint_id=<N>. Then
close_tab(<my tab id>).
```

A new sibling tab is created with the worktree at checkpoint N and
the conversation resumed from there; the caller closes itself.

## What the spike wires up

- `src/checkpoint.rs` — shadow-branch snapshot, worktree fork, JSONL
  copy/truncate, UUID v4 helper.
- `src/mcp.rs` — three new MCP tools: `checkpoint`, `rewind`, `fork`.
- `src/ui.rs` — message routing + `handle_checkpoint`/`_rewind`/`_fork`.
- `src/tab/stream.rs` — passes `--session-id` on fresh tabs or
  `--resume` on forked/rewound ones; supports `existing_worktree`
  to skip worktree creation (fork/rewind pre-create it).
- `src/tab/mod.rs` — `TerminalTab` gains `session_id`, `worktree_dir`,
  `checkpoints`.

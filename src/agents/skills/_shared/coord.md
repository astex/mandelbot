# Coordination protocol (`*.coord/`)

Shared reference for `mandelbot-delegate` and `mandelbot-work-as-subtask`. Both skills assume you've read this file.

## The coordination directory

One project = one `*.coord/` directory under `~/.mandelbot/coordination/`:

```
~/.mandelbot/coordination/<project>.coord/
  index.md                    # parent-owned — goal, plan link, how-we-work, children roster
  <label>.coord.md            # parent-created, then child-owned — plan link, state, append-only log
  <other-label>.coord.md
  <nested>.coord/             # present only if a child sub-delegated
    index.md
    <subchild>.coord.md
    ...
```

Templates live at `<plugin-dir>/skills/_shared/index.template.md` and `child.template.md`.

## Ownership

**The parent owns `index.md`.** It writes the initial file and later updates only the Children roster to mirror child states. It does not edit anything else inside `index.md` during the run.

**Each child owns its own `<label>.coord.md`.** It writes the `**Plan:**` and `**State:**` headers and appends log entries.

**One exception:** the parent may *append* `[DIRECTIVE]` entries into any child's log. It never edits existing entries, never touches a child's `**State:**` header, never touches sibling files.

**Children read only their own `*.coord.md`** and files it explicitly references by path (typically the governing plan and code files mentioned in the assignment). Children do **not** read `../index.md` or sibling files.

## State vocabulary

Used in the `**State:**` header and as the leading word of log entries where applicable:

`pending` · `planning` · `awaiting_review` · `in_progress` · `blocked` · `done` · `failed`

## Log format

Log entries are markdown bullets, one per state change, **append-only**:

```
- [YYYY-MM-DD HH:MM] <text>
```

Run `date '+%Y-%m-%d %H:%M'` to get a timestamp. Never edit or delete an existing entry — including `[DIRECTIVE]` entries the parent appended. The `**State:**` header duplicates the latest state for fast scanning; update it together with the log entry that changes state.

Entries are written on state changes, not on a timer.

## The `[DIRECTIVE]` marker

The parent uses `[DIRECTIVE]` as the leading marker when appending into a child's log:

```
- [2026-04-09 12:34] [DIRECTIVE] approved, proceed
- [2026-04-09 13:10] [DIRECTIVE] <redline or answer>
```

Children scan their log for new `[DIRECTIVE]` entries when their watcher wakes.

## The plan-review handshake

1. Child drafts its subplan by **writing the document directly** with the `Write` tool into `~/.claude/plans/<name>.md`. **Do not use Claude's built-in plan mode** — its only exit is `ExitPlanMode`, which blocks on user approval and would stall the handshake.
2. Child sets `**Plan:**` to the subplan path, sets `**State:** awaiting_review`, appends `- [...] plan drafted at <path>, awaiting review`.
3. Child runs the watcher against its own file in the background and waits.
4. Parent's directory watcher wakes, sees the child's new state, reads the linked subplan, and appends either `- [...] [DIRECTIVE] approved, proceed` or `- [...] [DIRECTIVE] <redline>` directly into the child's log.
5. On approval: child sets `**State:** in_progress`, appends `- [...] approved, starting implementation`, proceeds. On redline: child addresses it, updates its `**Plan:**` if needed, stays in `awaiting_review`, re-arms the watcher.

## The block/unblock handshake

When a child needs input from the parent:

1. Child appends `- [...] blocked: <question>` and sets `**State:** blocked`.
2. Child re-arms the watcher on its own file.
3. Parent replies with `- [...] [DIRECTIVE] <answer>` in the child's log.
4. Child appends `- [...] unblocked, continuing`, sets `**State:** in_progress`, resumes.

If the protocol itself can't accommodate something the child needs, it uses the same mechanism — append a question, wait, do not silently deviate.

## The watcher

One script, two modes, lives at `<plugin-dir>/skills/mandelbot-delegate/watch.sh`. It blocks until its target changes, prints what changed, then exits. **Always run it in the background** (`run_in_background: true`) so you're free to do other work while waiting.

- **Child → file mode.** A child watches its own `*.coord.md`:
  ```bash
  bash <plugin-dir>/skills/mandelbot-delegate/watch.sh <absolute path to your *.coord.md>
  ```
- **Parent → directory mode.** A parent watches the whole `*.coord/` tree (recursively, so nested sub-delegation is caught):
  ```bash
  bash <plugin-dir>/skills/mandelbot-delegate/watch.sh ~/.mandelbot/coordination/<project>.coord
  ```

The watcher is your **only** polling mechanism. Do not read coordination files on a timer. When the watcher wakes, act on what changed and re-arm it in the background.

## Sub-delegation

If a child decides to spawn its own children, it becomes a parent in its own right: it promotes its `*.coord.md` into a sibling `*.coord/` directory at the same path, writes its own `index.md`, and follows the `mandelbot-delegate` flow one level deeper. The ancestor's directory watcher catches changes at any depth — no special handling needed.

## Legacy single-file artifacts

Earlier versions of `mandelbot-delegate` wrote flat `~/.mandelbot/coordination/<name>.md` files. Those are obsolete but left in place — the new format only creates `<name>.coord/` directories, so there's no name collision, and the old files are inert once nothing reads them. Leave them alone unless the user asks you to clean up.

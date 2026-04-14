# Coordination protocol (`*.coord/`)

Shared reference for `mandelbot-delegate` and `mandelbot-work-as-subtask`. Both skills assume you've read this file.

## The coordination directory

One project = one `*.coord/` directory under `~/.mandelbot/coordination/`:

```
~/.mandelbot/coordination/<project>.coord/
  index.md                    # parent-owned — goal, plan link, how-we-work, children list
  <label>.coord.md            # parent-created, then child-owned — state, assignment, append-only log
  <other-label>.coord.md
  <nested>.coord/             # present only if a child sub-delegated
    index.md
    <subchild>.coord.md
    ...
```

Templates live at `<plugin-dir>/skills/_shared/index.template.md` and `child.template.md`.

## Ownership

**The parent owns `index.md`.** It writes the initial file with the children list and does not need to update it during the run — each child's own `*.coord.md` is the source of truth for that child's state.

**Each child owns its own `<label>.coord.md`.** It writes the `**State:**` header and appends log entries.

**One exception:** the parent may *append* `[DIRECTIVE]` entries into any child's log. It never edits existing entries and never touches a child's `**State:**` header or sibling files.

**Children read their own `*.coord.md` and the parent's `../index.md`** — plus any files explicitly referenced by path from their `*.coord.md` (typically the governing plan and code files mentioned in the assignment). Children do **not** read sibling files.

## State vocabulary

Used in the `**State:**` header and as the leading word of log entries where applicable:

`pending` · `in_progress` · `blocked` · `awaiting_review` · `done` · `failed`

`awaiting_review` is the default terminal state for a child whose work needs a human to review (and usually merge) the PR before it counts as complete. Projects can opt out by saying so in `index.md` — see [The awaiting_review state](#the-awaiting_review-state) below.

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
- [2026-04-09 12:34] [DIRECTIVE] <instruction or answer>
```

Children scan their log for new `[DIRECTIVE]` entries when their watcher wakes.

## The block/unblock handshake

When a child needs input from the parent:

1. Child appends `- [...] blocked: <question>` and sets `**State:** blocked`.
2. Child re-arms the watcher on its own file.
3. Parent replies with `- [...] [DIRECTIVE] <answer>` in the child's log.
4. Child appends `- [...] unblocked, continuing`, sets `**State:** in_progress`, resumes.

If the protocol itself can't accommodate something the child needs, it uses the same mechanism — append a question, wait, do not silently deviate.

## Escalation

Escalation uses the same block/unblock handshake. When a child hits something beyond its scope — the approach is wrong, a decision is needed that it can't make, or it needs resources it doesn't have — it appends `- [...] blocked: <explanation>` and waits. The parent decides what to do: answer directly, redirect the child, or escalate further up its own chain (the parent's parent may not be the user — in a nested tree, the parent may itself be a child with its own `*.coord.md`).

The child does not need to know who ultimately resolves the issue. It blocks, the parent handles it.

## The awaiting_review state

Used when a child has finished implementing and pushed its PR, but the project requires a human to review (and usually merge) the PR before the work counts as complete. The child stays alive as the "PR tab," handling review iterations until the PR is merged.

**This is the default.** Coord-based work is destructive (code changes, branch pushes, PR creation) and the safe default is for a human to review before it counts as done. The parent can opt out project-wide by saying so in `index.md` under "How we work → Reviews" — for example: *"Reviews are autonomous: children close on `done` without human review."* Children read this from `../index.md` and behave accordingly. If the index doesn't say so, children use `awaiting_review` and stay alive for review feedback.

The lifecycle:

1. Implementation done, branch pushed, PR opened.
2. Child appends `- [...] awaiting_review: <PR link>` and sets `**State:** awaiting_review`. **Does not close the tab.**
3. Child returns control. The tab idles until the human prompts it.
4. The child stays in `awaiting_review` for the entire review cycle, even while actively addressing review feedback and pushing changes — the parent doesn't need to know whether the agent is mid-edit or idle. **Do not append log entries during the review cycle** — every write to the coord file wakes the parent's watcher, and there's nothing for the parent to do. Stay quiet until the PR is settled.
5. Once the PR is merged (the human will say so, or instruct the child to do the merge itself), the child appends `- [...] done`, sets `**State:** done`, and closes its tab. (This single write is the parent's signal that the child is fully settled.)

The parent treats `awaiting_review` as terminal-for-its-purposes — the same bucket as `done` for "no further parent action needed right now." The parent's watcher will eventually see `done` when the PR merges. The parent does not append `[DIRECTIVE]` entries to push a review-staying-alive child along; review feedback flows through the tab's chat, not the coord file.

## The watcher

A single-file watcher script lives at `<plugin-dir>/skills/_shared/watch.sh`. It blocks until the target file changes, prints its contents, then exits. **Always run it in the background** (`run_in_background: true`) so you're free to do other work while waiting.

```bash
bash <plugin-dir>/skills/_shared/watch.sh <absolute path to file>
```

**Do not write your own watcher, poll loop, or inotify script.** Always use `watch.sh`. The exact command is shown above and repeated in each skill's workflow steps.

Both parents and children use the same script — one invocation per file. A parent runs one watcher per child file (each in the background). A child runs one watcher against its own `*.coord.md`.

The watcher is your **only** polling mechanism. Do not read coordination files on a timer. When the watcher wakes, act on what changed and re-arm it in the background.

## Tab lifecycle

Agents can close themselves and their descendants via the `close_tab` MCP tool. In multi-generation flows, children close themselves when done. The parent closes any stragglers between generations. This is a resource optimization, not a protocol requirement — the coordination files remain on disk regardless of tab state.

After setting `**State:** done` (or `failed`), close your tab:

```
close_tab(tab_id: <your own tab ID>)
```

**Two exceptions:**

- Children in `awaiting_review` stay alive — that's the whole point of the state. They close only when their PR merges.
- A parent tab does **not** close itself if any of its descendants are still alive (e.g. children in `awaiting_review`). Closing a parent tab promotes one of its children to take its place, which disrupts the tab organization. Stay open and idle until your descendants have all settled.

## Sub-delegation

If a child decides to spawn its own children, it becomes a parent in its own right: it promotes its `*.coord.md` into a sibling `*.coord/` directory at the same path, writes its own `index.md`, and follows the `mandelbot-delegate` flow one level deeper.

Generation tabs in `mandelbot-implement-iterate` are a standardized sub-delegation pattern: the generation tab is both a child (of the iterate parent) and a parent (of implementation children).

## Legacy single-file artifacts

Earlier versions of `mandelbot-delegate` wrote flat `~/.mandelbot/coordination/<name>.md` files. Those are obsolete but left in place — the new format only creates `<name>.coord/` directories, so there's no name collision, and the old files are inert once nothing reads them. Leave them alone unless the user asks you to clean up.

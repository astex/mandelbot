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

Children scan their log for new `[DIRECTIVE]` entries whenever a doorbell message from the parent arrives.

## The block/unblock handshake

When a child needs input from the parent:

1. Child appends `- [...] blocked: <question>` and sets `**State:** blocked`.
2. Child rings the parent's doorbell, then returns control and idles.
3. Parent appends `- [...] [DIRECTIVE] <answer>` in the child's log and rings the child's doorbell.
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
2. Child appends `- [...] awaiting_review: <PR link>`, sets `**State:** awaiting_review`, and rings the parent's doorbell. **Does not close the tab.**
3. Child returns control and idles. The tab wakes when either the human prompts it (chat) or a parent doorbell arrives.
4. The child stays in `awaiting_review` for the entire review cycle, even while actively addressing review feedback and pushing changes — the parent doesn't need to know whether the agent is mid-edit or idle. Log freely during review if it's useful for the record, but **do not ring the parent's doorbell** — there's nothing for the parent to do until the PR is settled.
5. Once the PR is merged (the human will say so, or instruct the child to do the merge itself), the child appends `- [...] done`, sets `**State:** done`, rings the parent's doorbell, and closes its tab.

Two channels are live during review:

- **Chat (human → child)** — code review feedback, fixup requests, "push this change." This is the dominant channel.
- **Coord file `[DIRECTIVE]` + doorbell (parent → child)** — cross-cutting, chain-wide directives: "something merged upstream, rebase onto new base," "abort, we're dropping this PR," "the sibling's approach changed, update your branch." Reserved for things only the parent can coordinate across siblings. Review feedback itself flows through chat, not here.

The parent treats `awaiting_review` as terminal-for-its-purposes — the same bucket as `done` for "no further parent action needed right now" — but may still append a `[DIRECTIVE]` when a chain-wide change forces it. The doorbell wakes the idle child to pick it up.

## The doorbell

Coordination has two halves, and they are deliberately separate:

- **The coord files are the state.** Every state change is written to a `*.coord.md` — that's the durable, inspectable record, readable long after every tab has closed. Nothing is coordinated by message alone.
- **A peer message is the doorbell.** It carries no information beyond "I changed your file, go read it." The recipient re-reads the file and acts on what it finds there.

Ringing the doorbell means `SendMessage` to the other agent's session, with a one-line body:

```
SendMessage(to: "<their session name>", message: "coord update: <absolute path to the file you changed>")
```

Do **not** put the question, the answer, or the state in the message. Two sources of truth is the failure mode this split exists to prevent — if it isn't in the file, it didn't happen.

**Write, then ring — as one step.** Appending a log entry without ringing leaves the other side idle forever; there is no timeout and no fallback poll. Never do one without the other.

Do not poll. Do not read coordination files on a timer, and do not write your own watcher, inotify script, or sleep loop. You idle until a doorbell (or the human in chat) wakes you.

### Addressing

Peer sessions are discovered with `ListAgents`, and the name in the listing *is* the address.

**Parent → child.** Run `ListAgents` immediately *before* `spawn_tab` and again after: the new row is the child. Resolve one child at a time — spawn, resolve, record, then move to the next — so two concurrent spawns can't both claim the same new row. A freshly spawned child takes a few seconds to appear; if no new row is listed yet, wait and list again.

In git-worktree projects there's a cross-check: a tab's session name is its worktree name plus a short suffix, so a child spawned on branch `late-stone-873` appears as something like `late-stone-873-85`. Use that to confirm you matched the right row, not as the primary lookup.

Record the resolved name in the child's coord file as a `**Session:**` header so it survives compaction.

**Child → parent.** An agent can't see its own session name, so the parent can't just tell the child where to reply. Instead the parent bootstraps the reverse direction: right after resolving a child's address, it sends that child a hello doorbell. Incoming messages arrive wrapped as `<cross-session-message from="...">`, so the child copies the `from` attribute into its own coord file as `**Parent session:**` and uses it for every upward doorbell after that.

This means a child cannot ring anyone until the parent's hello has landed. Parents send it as part of spawning, not later.

### What this gives up

Hand-editing a coord file no longer wakes anybody — there's no filesystem watch anymore. To nudge a running tab, prompt it in chat; that's the better channel for human input regardless. Editing a file for the record is still fine, it just won't be noticed until something else wakes the reader.

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

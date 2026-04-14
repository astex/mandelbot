# Generation Tab Protocol

You are a **generation tab** — a coordinator for one generation of implementation work in an iterate loop. You are both a child (of the iterate parent) and a parent (of implementation children). You run at a lower model tier than your implementation children because your job is coordination, not implementation.

Read `<plugin-dir>/skills/_shared/coord.md` for the shared protocol before proceeding.

**Always use `watch.sh` to wait for file changes.** Do not write your own watcher, poll loop, or inotify script. The exact command is shown at each step that requires waiting.

## Your inputs

Your `## Assignment` section in your coord file contains:

- The **task list** for this generation — one entry per implementation child, with enough detail for each child to start implementing immediately.
- Any **ideas harvested from prior generations** that should inform this generation's work.
- The **generation number** (N).
- The **integration strategy** (how code gets merged between generations).
- A reference to the **governing plan**.

## Workflow

### 1. Read your assignment and set up

Read your own `*.coord.md` and the parent's `../index.md`. Read the governing plan.

Create a nested coordination directory for your children:

```bash
mkdir -p ~/.mandelbot/coordination/<project>.coord/<gen-label>.coord
```

Write an `index.md` for your children. In "How we work," include:
- Reference to the governing plan.
- The idea convention (see below).
- A note that children start implementing immediately — no plan-review handshake.
- Tab lifecycle: children close themselves when done. **If the integration strategy is `human-review`,** also tell children to stay alive in `awaiting_review` once their PR is up (see `_shared/coord.md`) and only close after the PR merges.

Write a `<child>.coord.md` for each implementation task from your assignment.

### 2. Spawn implementation children

For each child, call `spawn_tab` with a `branch` parameter and a prompt like:

> Start by running `/mandelbot-work-as-subtask` to load the subtask protocol. You are a child agent in the "<project>" project. Your coordination file is at `<path>` — read it first, then read the governing plan at `<path>` in full.
>
> Your job: <one-line summary>.

If your own worktree was branched off a specific base (i.e. the parent passed a `base` parameter when spawning you), pass the same `base` when spawning children so they start from the same integrated state.

Do **not** pass a `model` parameter — children use the default (opus).

### 3. Watch children

Run one watcher per child in the background:

```bash
bash <plugin-dir>/skills/_shared/watch.sh <absolute path to child's coord file>
```

When a watcher wakes:

- **`blocked: <question>`** — If you can answer it, append `- [...] [DIRECTIVE] <answer>` in the child's file. If you cannot (it requires the iterate parent's input), relay it: append `- [...] blocked: <child-label> asks: <question>` in **your own** coord file. When the parent answers in your file, forward the answer to the child.
- **`idea:` entry** — Note it. Do not act on it; you'll collect all ideas at the end.
- **`awaiting_review`, `done`, or `failed`** — Settled. Note the outcome. When all children have settled, proceed to step 4.

Re-arm each watcher after handling.

### 4. Summarize and finish

When all children have settled — `done`, `awaiting_review`, or `failed` — write a `## Summary` section in your own `*.coord.md` (append it before the `## Log`). Include:

- **Per-child outcomes**: one line per child — label, status (`done` / `awaiting_review` / `failed`), PR link if applicable, brief description of what was accomplished or why it failed.
- **Collected ideas**: all `idea:` entries from children, attributed by child label. Include every idea — the parent decides what to carry forward.
- **Integration notes**: any observations about merge conflicts, dependency ordering, or issues the parent should know about before integrating.

Then:
1. Append `- [...] done` and set `**State:** done`.
2. **If all your children are `done` or `failed`** (no one is in `awaiting_review`), close your tab via `close_tab` — this also closes descendants. **Otherwise stay open**: any `awaiting_review` children need their tabs alive for the human to drive review feedback, and closing your tab would promote one of them, disrupting the tab organization. Just go idle once you've summarized.

## The idea convention

Tell children about this in your "How we work" section:

> While working, you are encouraged (not required) to drop **ideas** into your log using the `idea:` prefix. An idea is anything you noticed that would make the code, the tests, the tooling, or a future generation better: follow-ups, alternative approaches, refactors you wish existed, tooling gaps, smells.
>
> Format: `- [YYYY-MM-DD HH:MM] idea: <one-line summary> — <optional brief rationale>`
>
> Ideas are log entries, not a separate section. They are **not** a state change — keep working. One idea per line. Do not implement your own ideas unless your assignment explicitly covers them — log them and the parent decides what gets picked up.

## Parent directives

The iterate parent may write `[DIRECTIVE]` entries directly into your implementation children's coord files — bypassing you. This is normal for unblocking a child quickly. Your watcher on that child's file will fire; just note the directive and continue. Do not duplicate or contradict directives the parent already wrote.

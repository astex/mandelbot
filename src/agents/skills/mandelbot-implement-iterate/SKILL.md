---
name: mandelbot-implement-iterate
description: Use this skill for iterative build-refactor-build loops where a parent agent delegates work to generation tabs, harvests ideas, and spawns another generation acting on the best ones. Activates when the user wants to "keep improving X", run "another pass", do "iterative refinement", or loop until some fitness signal is met. Not for one-shot feature work — use mandelbot-delegate for that.
allowed-tools: [Read, Edit, Write, Bash, Glob, Grep, mcp__mandelbot__spawn_tab, mcp__mandelbot__close_tab, AskUserQuestion]
---

# Implement / Iterate

An iterative loop built on top of `mandelbot-delegate`. The parent spawns one **generation tab** per round, which manages its own implementation children. When the generation finishes, the generation tab writes a summary with collected ideas. The parent reads the summary, optionally checks in with the user, and spawns the next generation acting on the best ideas. Repeat until an exit condition is met.

Read `<plugin-dir>/skills/_shared/coord.md` for the shared protocol and `<plugin-dir>/skills/mandelbot-implement-iterate/GENERATION.md` for the generation tab protocol. This file covers the parent's perspective: framing the run, the generation cycle, iteration modes, integration strategies, and exit conditions.

## When to use

- Iterative refinement where the shape of "done" is fuzzy and you expect multiple passes.
- Build-refactor-build loops: build a working version, harvest observations from the implementers, apply them in the next generation.
- Any task where first-generation implementers will see things the planner didn't anticipate and you want a structured way to feed those observations back.

Don't use when the work is a one-shot feature with a clear spec (`mandelbot-delegate` directly).

## Flow

### 1. Frame the run

Work out the following with the user before spawning anything:

**Iteration mode** — how the parent decides what goes into the next generation:

- **`human-in-the-loop`** (default) — After each generation, the parent presents the summary and collected ideas to the user via `AskUserQuestion`. The user can add ideas, drop ideas, adjust priorities, or say "continue." The parent incorporates the user's input into gen N+1's assignments.
- **`autonomous`** — The parent harvests and curates ideas without user input. Useful when there's a clear fitness signal (tests pass, benchmark met, lint clean).
- **`fixed-count`** — Run N generations autonomously and stop. No inter-generation user interaction.

**Integration strategy** — how generation N's code gets into one place before generation N+1 starts:

- **`human-review`** — Children open draft PRs. The parent creates a merge PR combining all subtask branches. The human merges it before the next generation starts.
- **`agent-merge`** — The parent reviews gen-N children's code, merges their branches into a working branch between generations. Gen-N+1 children branch off the merged state.

The integration strategy stays fixed across all generations.

**Exit condition** — when to stop (see [Exit conditions](#exit-conditions)).

**`max_generations`** safety cap (default 5) — prevents runaway loops.

Write (or reuse) a plan file. Record the iteration mode, integration strategy, exit condition, and max_generations in the plan.

### 2. Create the coordination directory

```bash
mkdir -p ~/.mandelbot/coordination/<project>.coord
```

Write `index.md` from `<plugin-dir>/skills/_shared/index.template.md`. Fill in:
- Project name, absolute plan path.
- **How we work**: include the iteration mode, integration strategy, exit condition, and max_generations. Note that this is a multi-generation iterate run. Children of generation tabs should know about the idea convention — the generation tab will relay this.
- **Children**: one bullet per generation tab (`gen-1`, `gen-2`, ...).

### 3. Spawn generation N

Spawn ONE generation tab using `spawn_tab` with `model: "sonnet"` (the generation tab is a coordinator, not an implementer) and a `branch` parameter. Write a `gen-<N>.coord.md` from `child.template.md` before spawning. The assignment should include:

- The **task list** for this generation — what implementation children to create, with detailed assignments for each.
- Any **ideas harvested from prior generations** that this generation should act on.
- The **generation number**.
- The **integration strategy** (so children know whether to open PRs, etc.).
- Reference to `<plugin-dir>/skills/mandelbot-implement-iterate/GENERATION.md` for the generation tab protocol.
- Reference to the governing plan.

Prompt:

> Start by reading `<plugin-dir>/skills/mandelbot-implement-iterate/GENERATION.md` for the generation tab protocol. You are a generation tab in the "<project>" iterate run. Your coordination file is at `<absolute path to gen-<N>.coord.md>` — read it first, then read the governing plan at `<path>` in full.
>
> You manage generation <N>. Create implementation children from the task list in your assignment, watch them, collect ideas, and write a summary when done.

### 4. Watch generation N

Run ONE watcher on `gen-<N>.coord.md`:

```bash
bash <plugin-dir>/skills/_shared/watch.sh <absolute path to gen-<N>.coord.md>
```

**Do not write your own watcher.** Always use `watch.sh`.

When the watcher wakes:

- **`blocked:`** — The generation tab is relaying a block from one of its children (or is itself blocked). Read the question, resolve it. You may write a `[DIRECTIVE]` directly into a grandchild's coord file to unblock it quickly, or answer in the generation tab's file.
- **`done`** — The generation tab has finished. Proceed to step 5.
- **Other log entries** — Re-arm the watcher and wait.

### 5. Harvest from generation tab

Read `gen-<N>.coord.md`'s `## Summary` section. The generation tab has already collected ideas from its children and summarized outcomes. Use this as your source of truth for what happened and what ideas emerged.

### 6. Integrate generation N

Per the integration strategy decided in step 1:

- **`human-review`**: Verify children's PRs exist. Create a merge PR combining all subtask branches for this generation. Wait for the human to merge it before proceeding.
- **`agent-merge`**: Review the generation's code. Merge children's branches into the working branch. Verify the merge is clean.

Do not spawn generation N+1 until generation N's code is integrated. Children working on stale code will produce conflicts and wasted work.

### 7. User checkpoint (if human-in-the-loop)

If the iteration mode is `human-in-the-loop`, present the generation summary and collected ideas to the user via `AskUserQuestion`. Include:

- What was accomplished this generation (per-child outcomes).
- The collected ideas, attributed by source.
- Your recommendation for what to pursue next.

The user may add their own ideas, drop ideas, reprioritize, or adjust the remaining work. Incorporate their input into gen N+1's task list.

### 8. Check exit condition

If the exit condition is met or a safety stop fires: wrap up (see below).

Otherwise, pick ideas from the harvest (incorporating user input if applicable) and go to step 3 with N+1.

### Safety stops

Beyond the parent-defined exit condition, the loop also stops if:

- **All children in a generation failed.** Escalate to the user rather than spawning on top of a broken base.
- **`max_generations` reached.** Prevents runaway loops when the exit condition is soft.

### Wrapping up

When the loop ends:

1. Close any remaining generation or child tabs via `close_tab`.
2. Write a short retrospective in `index.md`'s "How we work" section: which ideas were picked up, which were dropped, and why. Note the final generation count.
3. Handle any final integration (merge to main, open a final PR, etc.) per the integration strategy.

## Exit conditions

The parent picks one per run and records it in step 1:

- **Fixed iteration count.** "Run 3 generations and stop." Simplest.
- **No new ideas of value.** Parent's judgment after harvesting. Stops when a generation yields only ideas not worth pursuing.
- **External fitness signal.** Something checkable between generations: tests pass, a benchmark crosses a threshold, lints clean, a specific error gone.

Always pair with the `max_generations` safety cap. Soft exit conditions without a cap are a foot-gun.

## Failure modes

- **Generation produces zero ideas.** Not necessarily a failure — the exit condition "no new ideas of value" may now be met. Evaluate and stop or continue.
- **Ideas are too vague.** The generation tab should have asked children to be specific. If the summary is still vague, note it for the next generation's assignment.
- **Idea is out of scope.** Drop it from the harvest with a note in the retrospective. Don't carry dead weight into the next generation.

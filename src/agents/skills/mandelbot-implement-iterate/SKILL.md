---
name: mandelbot-implement-iterate
description: Use this skill for iterative build-refactor-build loops where a parent agent delegates work to a generation of children, harvests their "ideas" from the coordination logs, and spawns another generation acting on the best ones. Activates when the user wants to "keep improving X", run "another pass", do "iterative refinement", or loop until some fitness signal is met. Not for one-shot feature work — use mandelbot-delegate for that.
allowed-tools: [Read, Edit, Write, Bash, Glob, Grep, mcp__mandelbot__spawn_tab]
---

# Implement / Iterate

An iterative loop built on top of `mandelbot-delegate`. The parent spawns a generation of children to do implementation or refactoring work. Children, while they work, drop **ideas** into their coord logs — observations, follow-ups, alternatives, refactors they wish existed. When the generation finishes, the parent reads the accumulated ideas across all children, picks one or a coherent set, and spawns a new generation whose assignments act on those ideas. Repeat until an exit condition is met.

Read `<plugin-dir>/skills/_shared/coord.md` for the shared protocol and `mandelbot-delegate`'s SKILL.md for the parent workflow. This file only covers what's specific to the iterate loop: the idea convention, the generation cycle, and exit conditions.

## When to use

- Iterative refinement where the shape of "done" is fuzzy and you expect multiple passes.
- Build-refactor-build loops: build a working version, harvest observations from the implementers, apply them in the next generation.
- Any task where first-generation implementers will see things the planner didn't anticipate and you want a structured way to feed those observations back.

Don't use when the work is a one-shot feature with a clear spec (`mandelbot-delegate` directly) or a spike-then-harden flow (`mandelbot-spike-harden`).

## The idea convention

Ideas are append-only log entries in a child's `*.coord.md`, marked with an `idea:` prefix — analogous to `blocked:`:

```
- [YYYY-MM-DD HH:MM] idea: <one-line summary> — <optional brief rationale>
```

Rules:

1. **Log entries, not a separate section.** They sit inline with the rest of the log. The watcher wakes on them like any other entry.
2. **Not a state transition.** Writing `idea:` does not change `**State:**`. The child keeps working.
3. **One idea per line.** Multiple ideas → multiple lines.
4. **The idea is the child's, not a directive.** The parent decides whether it becomes next-generation work. Children do not act on their own ideas in the current generation unless their assignment explicitly covers them.
5. **Scoped to this generation.** Children don't cross-reference prior generations; the parent carries anything worth carrying forward.

Children learn this convention by reading `index.md`'s "How we work" section, where the parent includes it during setup (see step 2 below).

## Flow

### 1. Frame the run

Write (or reuse) a plan file. In addition to the normal plan content, decide the **exit condition** for the loop (see [Exit conditions](#exit-conditions)) and the **`max_generations`** safety cap (default 5).

### 2. Create the coordination directory

Follow the `mandelbot-delegate` workflow. In `index.md`'s "How we work" section, include:

- The exit condition and the `max_generations` cap.
- A note that this is a multi-generation iterate run — children should expect their assignments to reference ideas harvested from prior generations.
- The **merge strategy** for between generations. This is project-specific — some projects require human review before merging, others let the parent review and merge directly. Whatever the rule, state it here so children know what "done" means. If human review is required, tell children to open a PR and wait for it to merge before reporting `done`. See [Integrate generation N](#4½-integrate-generation-n).
- The idea convention, written for children. Use something like:

> While working, you are encouraged (not required) to drop **ideas** into your log using the `idea:` prefix — analogous to `blocked:`. An idea is anything you noticed that would make the code, the tests, the tooling, or a future generation better: follow-ups, alternative approaches, refactors you wish existed, tooling gaps, smells.
>
> Format: `- [YYYY-MM-DD HH:MM] idea: <one-line summary> — <optional brief rationale>`
>
> Ideas are log entries, not a separate section. They are **not** a state change — keep working. One idea per line. Do not implement your own ideas unless your assignment explicitly covers them — log them and the parent decides what gets picked up.

### 3. Spawn generation N

Delegate via `mandelbot-delegate`. Name each child file `<label>-g<N>.coord.md` — the generation number keeps files from colliding and gives a visible history in the directory listing.

In each child's assignment, include:

- The work for this generation.
- Any ideas harvested from prior generations that this child should act on (put these in the assignment text, not as `[DIRECTIVE]` entries — directives are for mid-flight corrections to a running child).
- The generation number so children know where they are in the loop.

Children follow the normal plan-review handshake from `_shared/coord.md`. They pick up the idea convention from `index.md`'s "How we work" section.

### 4. Wait for generation N

Standard delegate monitoring — one watcher per child, re-arm until all are `done` or `failed`.

### 4½. Integrate generation N

Generation N+1 children must start from code that includes generation N's changes. Before spawning the next generation, ensure gen-N's branches are merged.

How merging happens depends on the project's merge strategy (defined in "How we work"):

- **Parent reviews and merges directly.** The parent reviews gen-N children's code, merges their branches, and gen-N+1 children branch off the merged state.
- **Human review required.** Gen-N children open PRs and do not report `done` until their PRs are merged. The parent's normal wait loop (step 4) naturally blocks until integration is complete.

Either way: do not spawn generation N+1 until generation N's code is integrated. Children working on stale code will produce conflicts and wasted work.

### 5. Harvest ideas

Walk each generation-N child file and grep for `idea:` entries:

```bash
grep 'idea:' ~/.mandelbot/coordination/<project>.coord/*-g<N>.coord.md
```

Collect in memory. The ideas live in the child logs — they are the source of truth. Optionally note the harvest count in `index.md`'s "How we work" section for the human reader, e.g. `generation 1: harvested 4 ideas, proceeding with 2`.

### 6. Check exit condition

If the exit condition is met or a safety stop fires (see below): set `index.md` state to `done`, append a short retrospective to "How we work" covering which ideas were picked up, which were dropped, and why. Stop.

Otherwise, pick one idea or a coherent set from the harvest and go to step 3 with N+1.

### Safety stops

Beyond the parent-defined exit condition, the loop also stops if:

- **All children in a generation failed.** Escalate to the user rather than spawning on top of a broken base.
- **`max_generations` reached.** Prevents runaway loops when the exit condition is soft.

## Exit conditions

The parent picks one per run and writes it into `index.md`'s "How we work" section before spawning generation 1:

- **Fixed iteration count.** "Run 3 generations and stop." Simplest.
- **No new ideas of value.** Parent's judgment after harvesting. Stops when a generation yields only ideas not worth pursuing.
- **External fitness signal.** Something checkable between generations: tests pass, a benchmark crosses a threshold, lints clean, a specific error gone.

Always pair with the `max_generations` safety cap. Soft exit conditions without a cap are a foot-gun.

## Failure modes

- **Generation produces zero ideas.** Not necessarily a failure — the exit condition "no new ideas of value" may now be met. Evaluate and stop or continue.
- **Ideas are too vague.** Parent appends a `[DIRECTIVE]` asking the child to be more specific before considering the generation done.
- **Idea is out of scope.** Drop it from the harvest with a note in the retrospective. Don't carry dead weight into the next generation.

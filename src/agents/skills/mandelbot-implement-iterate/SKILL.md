---
name: mandelbot-implement-iterate
description: Use this skill for iterative build-refactor-build loops where a parent agent delegates work to a generation of children, harvests their "ideas" from the coordination logs, and spawns another generation acting on the best ones. Activates when the user wants to "keep improving X", run "another pass", do "iterative refinement", or loop until some fitness signal is met. Not for one-shot feature work — use mandelbot-delegate for that.
allowed-tools: [Read, Edit, Write, Bash, Glob, Grep, mcp__mandelbot__spawn_tab]
---

# Implement / Iterate

An iterative loop built on top of `mandelbot-delegate`. The parent spawns a generation of children to do some implementation or refactoring work. Children, while they work, drop **ideas** into their own coord logs — observations, follow-ups, alternatives, refactors they wish existed. When the generation finishes, the parent reads the accumulated ideas across all children, picks one or a coherent set, and spawns a new generation whose assignments act on those ideas. Repeat until an exit condition is met.

This is the closest thing in the skill set to build-refactor-build, and the one that most exercises the idea of the `*.coord/` directory as a bidirectional blackboard: children write ideas the parent reads; the parent writes assignments the next generation reads.

## Prerequisites

- **`mandelbot-delegate`** is the coordination substrate. This skill does not reimplement any of the plumbing — it composes delegate into a loop. Every step below that creates files, spawns children, or watches for progress uses the delegate mechanism exactly as documented there.
- You are comfortable with the shared `*.coord/` directory format, the `[DIRECTIVE]` marker, and the state vocabulary (`pending`, `planning`, `awaiting_review`, `in_progress`, `blocked`, `done`, `failed`). If not, read the delegate skill first.

## The idea convention

Ideas are append-only log entries in a child's `*.coord.md`, marked with an `idea:` prefix — analogous to the existing `blocked:` marker. Shape:

```
- [YYYY-MM-DD HH:MM] idea: <one-line summary> — <optional brief rationale>
```

Rules:

1. **Append-only log entries.** Ideas sit inline with the rest of the log. No separate section, no frontmatter field. This keeps the timeline intact and the watcher wakes on them like any other entry.
2. **Not a state transition.** Writing `idea:` does not change the child's `**State:**` field. The child keeps working on its assignment.
3. **One idea per line.** Multiple ideas → multiple lines.
4. **The idea is the child's, not a directive.** The parent decides whether an idea becomes next-generation work. Children do not act on their own ideas in the current generation unless their assignment explicitly covers them — they log and keep going.
5. **Scoped to this generation.** Children don't cross-reference ideas from prior generations; the parent carries anything worth carrying forward.

The canonical place this convention is explained to children is the **per-child assignment template** below. The loop steps reference that template rather than re-explaining the rules.

## Per-child assignment template

When the parent creates a generation-N child's `*.coord.md`, the assignment body should include this block (or an equivalent paraphrase). It is the single source of truth children see for the idea convention.

```markdown
## Idea convention

While working, you are encouraged (not required) to drop **ideas** into your log
using the `idea:` prefix — analogous to `blocked:`. An idea is anything you
noticed that would make the code, the tests, the tooling, or a future generation
of this loop better: follow-ups, alternative approaches, refactors you wish
existed, tooling gaps, smells.

Format:

- [YYYY-MM-DD HH:MM] idea: <one-line summary> — <optional brief rationale>

Rules:

- Ideas are log entries, not a separate section.
- Ideas are **not** a state change. Writing one does not set you to `blocked`
  or pause your work. Keep going.
- One idea per line. Multiple ideas → multiple lines.
- Do not implement your own ideas in this generation unless your assignment
  explicitly covers them. Log them. The parent decides what gets picked up.

You are generation **<N>** of this loop. Your assignment may reference ideas
harvested from earlier generations; those are listed in the assignment text
above (not as `[DIRECTIVE]` entries).
```

## The loop

### 1. Frame the run

Write (or reuse) a plan file describing the target and the **exit condition** for the loop. Exit conditions are parent-defined per run — see "Exit conditions" below. Record the exit condition in the project `index.md`'s "How we work" section so every future read of the coord tree can see it.

### 2. Create the coordination directory

Use `mandelbot-delegate`'s normal mechanism to create `~/.mandelbot/coordination/<project>.coord/`. In `index.md`'s "How we work" section, include:

- The exit condition.
- A one-line note that this is a multi-generation iterate run, and that children should expect their assignments to reference ideas harvested from prior generations.
- A pointer to the per-child assignment template above for the idea convention. Do not paste the full convention here; it goes into each child's file.

### 3. Spawn generation N

Delegate the current generation's assignment to one or more children via `mandelbot-delegate`. Name each child file `<label>-g<N>.coord.md` — the generation number in the label keeps files from different generations from colliding and gives you a visible history in the directory listing.

In each child's file, include:

- The assignment itself (what to implement, which files, what "done" means for this child in this generation).
- Any ideas harvested from prior generations that this child should act on. Put these in the assignment text, not as `[DIRECTIVE]` entries. `[DIRECTIVE]` is reserved for mid-flight course corrections to an already-running child.
- The per-child assignment template block above (verbatim or paraphrased).

Children then follow the normal parent-review handshake: enter plan mode, draft their subplan, set state to `awaiting_review`, wait for a `[DIRECTIVE] approved` entry from the parent before executing.

### 4. Wait for generation N to complete

Run the delegate watcher against the `*.coord/` directory in the background. When it wakes, check whether all generation-N children are `done` or `failed`. If not, run the watcher again. (This is the standard delegate monitoring flow.)

### 5. Harvest ideas

Walk each generation-N child's `*.coord.md` and grep its log for `idea:` entries:

```bash
grep -n 'idea:' ~/.mandelbot/coordination/<project>.coord/*-g<N>.coord.md
```

Collect the matches into an in-memory list. The ideas live in the child logs — they are the source of truth. Do not copy them into a separate file. You may optionally append a one-line summary to `index.md`'s "How we work" section for the human reader's benefit, e.g. `generation 1 harvested 4 ideas, proceeding with 2 in generation 2`.

### 6. Check the exit condition

Evaluate the exit condition (see below). If met:

- Append a final note to `index.md`'s "How we work" section summarizing which ideas were picked up across all generations, which were dropped, and why.
- Set the project `**State:**` in `index.md` to `done`.
- Stop.

Otherwise, continue.

### 7. Seed generation N+1

Pick one idea or a coherent set from the harvest. Write the next generation's child files — fresh `<label>-g<(N+1)>.coord.md` files — referencing the picked ideas explicitly in each child's assignment text. Go to step 3.

### 8. Safety stops

Beyond the parent-defined exit condition, the loop also stops if:

- **All children in a generation failed.** Escalate to the user rather than spawning another generation on top of a broken base.
- **`max_generations` reached.** Default safety cap is **5**. The parent may raise or lower it per run by recording the override in `index.md`'s "How we work" section at the start. This prevents runaway loops when the exit condition is soft (e.g. "no new ideas of value").

## Exit conditions

The parent picks one of these per run and writes it into `index.md`'s "How we work" section before spawning generation 1. Canonical forms:

- **Fixed iteration count.** "Run 3 generations and stop." Simplest; useful when you know roughly how much polish you want.
- **No new ideas of value.** The parent's judgment after harvesting. Stops when a generation yields only ideas the parent considers not worth pursuing (duplicates, out-of-scope, trivial).
- **External fitness signal.** Something the parent can check between generations: tests pass, a benchmark crosses a threshold, lints clean, a specific count of files or functions, a specific error gone. Useful when "done" has a concrete definition.

Whichever is chosen, pair it with the `max_generations` safety cap from step 8. Soft exit conditions without a cap are a foot-gun.

## Interactions with the shared protocol

- **Ownership stays strict.** The parent reads each child's `*.coord.md` to harvest ideas — which is already allowed by the protocol (parents walk their children's files). The parent never edits a completed child's file. Harvested ideas become text in generation-N+1 child assignments, which are fresh files.
- **`[DIRECTIVE]` is unchanged.** It is still the marker for mid-flight parent course corrections written into a running child's file. Idea harvesting is not a directive flow.
- **Sub-delegation is allowed.** A generation-N child may itself sub-delegate using `mandelbot-delegate`, promoting its `*.coord.md` into a `*.coord/` directory. Ideas from sub-children are visible to the generation-N child, which may promote them into its own log (as its own `idea:` entries) for the top-level parent to see. Deep-tree idea harvesting is out of scope — the top-level parent only reads its direct children.
- **Watcher usage is unchanged.** Parent watches the `*.coord/` directory, children watch their own `*.coord.md`. The idea convention needs no watcher changes — `idea:` entries are just log lines and trigger wake-ups like any other write.

## When to use

- Iterative refinement where the shape of "done" is fuzzy and you expect multiple passes to improve the result.
- Build-refactor-build loops: build a working version, harvest refactors the implementer noticed, apply them in the next generation.
- Any task where you suspect the first-generation implementer will see things the planner didn't anticipate and you want a structured way to feed those observations back into the next pass.

## When not to use

- One-shot feature work with a clear spec. Use plain `mandelbot-delegate` — the loop adds cost without adding value.
- Spike-then-harden flows. Use `mandelbot-spike-harden` — it's a two-stage pipeline, not a loop, and its cheat-list convention is a better fit than the open-ended idea log.
- Work where you only expect one generation's worth of output. If you're not actually going to run a second generation, you're paying the iterate overhead for nothing.

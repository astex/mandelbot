---
name: mandelbot-spike-harden
description: Use this skill when you want to build something end-to-end fast and then harden it. Spawns a spike child (cut corners, prove the approach) then a harden child (tests, edge cases, remove hacks). Good fit for "I want to try X" tasks where correctness matters but the shape of the solution is still uncertain.
allowed-tools: [Read, Edit, Write, Bash, Glob, Grep, mcp__mandelbot__spawn_tab]
---

# Spike then harden

A two-phase delegation flow built on top of `mandelbot-delegate`. Phase 1 (**spike**) proves an approach end-to-end as fast as possible — hardcoding, skipping tests, leaving TODOs is expected. Phase 2 (**harden**) takes the spike's output and produces the real version. The handoff is a **cheat list** the spike child writes into its coordination log; the harden child reads it.

Read `<plugin-dir>/skills/_shared/coord.md` for the shared protocol (directory layout, ownership, state vocabulary, log format, `[DIRECTIVE]` marker, handshakes, watcher). Read `mandelbot-delegate`'s SKILL.md for the parent workflow. This file only covers what's specific to the spike-harden flow.

## When to use

- The right approach is uncertain and you want to validate before investing in correctness.
- Eventual correctness matters — you can't ship the spike as-is.

Don't use when the approach is obvious (`mandelbot-delegate` directly), when correctness doesn't matter (just spike), or when the work is easily parallelizable (spike/harden is sequential).

## Flow

Follow the `mandelbot-delegate` workflow with these overrides:

1. **Plan both phases** up front — what needs to exist at the end, not just what the spike will prove.

2. **Fixed child labels.** The two children must be named `spike` and `harden`. These labels are load-bearing — the harden child's assignment references `../spike.coord.md` by path to find the cheat list.

3. **Sequential, not parallel.** Spawn only the spike child first. The harden child is not spawned until the spike is `done`.

4. **"How we work" additions.** In the `index.md`, note that this is a spike-harden run, that `spike` runs first and `harden` second, and that the harden child's assignment explicitly references `../spike.coord.md` for the cheat list (this is within protocol — children may read files their assignment references by path).

5. **Spike child prompt.** Include the framing from [Spike child framing](#spike-child-framing) in the spike's `## Assignment` and in the spawn prompt.

6. **After spike completes.** Read `spike.coord.md`, review the `## Cheat list`. Decide which items the harden phase must address and which are acceptable to keep. Then spawn the harden child with the framing from [Harden child framing](#harden-child-framing). If you want to scope the cheat list (defer items, add requirements), include it as an initial `[DIRECTIVE]` entry in `harden.coord.md` at spawn time.

7. **After harden completes.** Finalize per the branching strategy below.

## Spike child framing

Copy this into the spike child's assignment and prompt — it is the entire point of the spike role.

**Goal:** Prove the approach end-to-end. A working, runnable result beats a clean one. Ship fastest-path.

**Permitted shortcuts:**

- Hardcoding values (URLs, IDs, config).
- Skipping tests entirely.
- Leaving `TODO` / `FIXME` comments.
- Hacking around edge cases (assume happy path, skip retries).
- Mocking or stubbing external dependencies.
- Ignoring error paths — panic, log-and-continue, whatever's fastest.
- Copy-pasting instead of refactoring.

**Required deliverables:**

1. A runnable end-to-end implementation on the spike child's branch.
2. A `## Cheat list` section in `spike.coord.md` (not a new file — a section within the existing coord file), listing every shortcut taken.

**The cheat list is a first-class deliverable, not an afterthought.** If the spike child did not write a cheat list, it did not finish.

### Cheat list format

Fixed heading, fixed shape. The harden child greps for this exact heading in `../spike.coord.md`.

```markdown
## Cheat list

- <file:line or area> — <what was hacked or skipped> — <what the real version should do>
- ...
```

Each entry must be specific enough that a different agent, reading only the cheat list and the diff, can understand what to fix. Vague entries like "error handling" are not enough — name the function or file.

## Harden child framing

Copy this into the harden child's assignment and prompt.

**Working material:** The harden child checks out the spike child's branch as its base (not the main branch). Branch name should signal lineage, e.g. `<spike-branch>-harden`.

**Primary task list — the cheat list.** The harden child's first action is to read `../spike.coord.md` and extract the `## Cheat list` section. Reference this path explicitly in the harden child's `## Assignment` — the shared protocol allows children to read files their assignment references by path, so this is within protocol, not an exception.

**Scoping directives.** Any `[DIRECTIVE]` entries the parent placed in `harden.coord.md` override or scope down the raw cheat list.

**Deliverable:** For each cheat list item, the harden child must either:

- Address it (remove the hack, add the test, handle the edge case), or
- Explicitly defer it with a logged reason in `harden.coord.md`.

Before marking `done`, the harden child appends a summary entry to its log noting which items were addressed, deferred, or deemed unnecessary.

## Branching

Default: **single-pr**. The harden child branches off the spike child's branch. The final PR contains both sets of commits; reviewers see spike-then-harden in the history.

Alternative: **multi-pr**. Only when the spike is genuinely throwaway and you expect the harden phase to rewrite rather than extend. Rare — prefer single-pr.

## Failure modes

- **Spike fails.** Parent decides whether to abort or course-correct via `[DIRECTIVE]`.
- **Harden discovers the spike's approach is fundamentally wrong.** Harden appends `blocked: spike approach is wrong because <reason>` and waits. Parent decides whether to re-spike or authorize a rewrite.
- **Cheat list missing or vague.** Harden appends `blocked: cheat list missing/insufficient` and waits.

All of these use the standard block/unblock handshake from `_shared/coord.md`.

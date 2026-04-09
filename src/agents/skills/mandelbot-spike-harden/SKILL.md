---
name: mandelbot-spike-harden
description: Use this skill when you want to build something end-to-end fast and then harden it. Spawns a spike child (cut corners, prove the approach) then a harden child (tests, edge cases, remove hacks). Good fit for "I want to try X" tasks where correctness matters but the shape of the solution is still uncertain.
allowed-tools: [Read, Edit, Write, Bash, Glob, Grep, mcp__mandelbot__spawn_tab]
---

# Spike then harden

A two-phase delegation flow. Phase 1 (**spike**) proves an approach end-to-end as fast as possible — hardcoding, skipping tests, leaving TODOs, hacking around edge cases is allowed and expected. Phase 2 (**harden**) takes the spike's output and produces the real version: tests, error handling, edge cases, hacks removed.

The two phases run sequentially. The handoff is a **cheat list** the spike child writes into its own coordination log; the harden child reads it by convention.

## When to use this skill

Use it when:

- The right approach is uncertain and you want to validate it before investing in correctness.
- Eventual correctness matters — you can't ship the spike as-is.

Don't use it when:

- The approach is obvious. Use `mandelbot-delegate` directly.
- Correctness doesn't matter and the spike itself is the deliverable. Just do the spike.
- The work is easily parallelizable. Spike/harden is inherently sequential.

## Dependency on mandelbot-delegate

This skill is a thin protocol layer on top of `mandelbot-delegate`. It does not reimplement coordination plumbing — it reuses the `*.coord/` directory format, the plan-draft / `awaiting_review` / `[DIRECTIVE] approved` handshake, and the `watch.sh` watcher from that skill. Read `mandelbot-delegate`'s SKILL.md alongside this one before running the flow.

If the refactored `mandelbot-delegate` skill has not yet landed in the repo when you run this, fall back to the coordination format defined in the governing plan at `/home/phil-kremidas/.claude/plans/delightful-soaring-moore.md` (the "Concrete markdown shape" section). That format is the contract `mandelbot-delegate` implements to; writing against it is safe.

## Flow

1. **Plan the whole effort.** Before spawning anything, draft a top-level plan covering both phases — what needs to exist at the end, not just what the spike will prove. Save it to `~/.claude/plans/<name>.md`.

2. **Create the coord directory.** Follow `mandelbot-delegate` to create `~/.mandelbot/coordination/<project>.coord/` with an `index.md`. The two children are named with **fixed labels**: `spike` and `harden`. The labels are load-bearing — the harden child relies on them to locate the spike's log.

3. **Write the "How we work" section.** In the `index.md`, explain that this is a spike-harden run, that `spike` runs first and `harden` runs second, and that the harden child is explicitly permitted to read `../spike.coord.md` for its cheat list (see "Cross-child read exception" below).

4. **Spawn the spike child.** Create `spike.coord.md` and spawn the child. The spike child's prompt must include the framing in [Spike child framing](#spike-child-framing).

5. **Watch for spike completion.** Run `watch.sh` against the coord directory in the background. When `spike.coord.md` reaches `done`, read it — especially the `## Cheat list` section.

6. **Review the cheat list.** Decide which cheats must be addressed by the harden phase and which are acceptable to keep (a hardcoded default that's actually fine; a skipped feature that's out of scope). Append a `[DIRECTIVE]` entry to `spike.coord.md` only if you need to tell the spike child something *before* it's considered done — otherwise the scoping goes into the harden child's file.

7. **Spawn the harden child.** Create `harden.coord.md` and spawn the child with the framing in [Harden child framing](#harden-child-framing). If you want to scope the cheat list (defer items, add new requirements), include it as an initial `[DIRECTIVE]` entry in `harden.coord.md` at spawn time.

8. **Watch for harden completion.** Same watcher, same directory. When `harden.coord.md` reaches `done`, the flow is finished.

9. **Finalize.** Merge per the chosen workflow (see [Branching](#branching)).

## Spike child framing

The spike child's prompt (and the `index.md` "How we work" section) must convey all of the following. Copy this framing into the prompt — it is the entire point of the spike role.

**Goal:** Prove the approach end-to-end. A working, runnable result beats a clean one. Ship fastest-path.

**Permitted shortcuts:**

- Hardcoding values (URLs, IDs, credentials for local dev, config).
- Skipping tests entirely.
- Leaving `TODO` / `FIXME` comments liberally.
- Hacking around edge cases (ignore empty inputs, assume happy path, skip retries).
- Mocking or stubbing external dependencies rather than integrating them.
- Ignoring error paths — let it panic, let it log and continue, whatever's fastest.
- Copy-pasting instead of refactoring.

**Required deliverables:**

1. A runnable end-to-end implementation on the spike child's branch.
2. A `## Cheat list` section in its own `spike.coord.md` log (not a new file — a section within the existing log), listing every shortcut taken.

**The cheat list is a first-class deliverable, not an afterthought.** The spike child should treat writing it as part of the job, not as documentation. If the spike child did not write a cheat list, it did not finish.

### Cheat list format

Fixed heading, fixed shape. The harden child greps for this exact heading in `spike.coord.md`.

```markdown
## Cheat list

- <file:line or area> — <what was hacked or skipped> — <what the real version should do>
- <file:line or area> — <what was hacked or skipped> — <what the real version should do>
- ...
```

Each entry should be specific enough that a different agent, reading only the cheat list and the diff, can understand what to fix. Vague entries like "error handling" are not enough — name the function or file.

## Harden child framing

The harden child's prompt must convey:

**Working material:** The harden child checks out the spike child's branch as its base (not `master`). Its branch name should signal the lineage, e.g. `<spike-branch>-harden` or similar.

**Primary task list — the cheat list.** The harden child's first action is to read `../spike.coord.md` and extract the `## Cheat list` section. This is the only cross-child file read in the whole protocol and it is **read-only**. (See [Cross-child read exception](#cross-child-read-exception).)

**Scoping directives.** If the parent appended any `[DIRECTIVE]` entries to `harden.coord.md` describing which cheat-list items are in scope / deferred / out of scope, those directives override the raw cheat list.

**Deliverable:** Tests, error handling, edge cases covered, hacks removed. For each cheat list item, the harden child must either:

- Address it (remove the hack, add the test, handle the edge case), or
- Explicitly defer it with a logged reason in its own `harden.coord.md`.

Before marking `done`, the harden child appends a summary entry to its log noting which cheat list items were addressed, deferred, or deemed unnecessary.

## Cross-child read exception

The coordination protocol's default is that **children do not read sibling files** — each child only watches its own `*.coord.md`, and the parent is the only one with the whole picture. The harden child reading `../spike.coord.md` is a **deliberate, documented exception** to that rule, not a leak.

Why it's OK here:

- The read is one-way and read-only. The harden child never writes to `spike.coord.md`.
- The spike child is `done` before the harden child is spawned, so there's no concurrent access.
- The cheat list is a published, structurally-fixed artifact. The harden child depends on the heading name, not on the rest of the spike log.

When writing the parent's `index.md` "How we work" section, call this exception out explicitly so future readers understand it's intentional.

## Branching

Default: **single-pr**. The harden child branches off the spike child's branch. The final PR contains both sets of commits; reviewers see spike-then-harden in the history, which is usually valuable context.

Alternative: **multi-pr**. Use this only when the spike is genuinely throwaway and you expect the harden phase to rewrite rather than extend. In that case the harden child branches off `master` and produces an independent PR; the spike's PR may be closed without merging. This is rare — prefer single-pr.

## Failure modes

- **Spike fails to prove the approach.** The spike child appends `failed` or `blocked: <reason>` to its log. The parent decides whether the approach is unworkable (abort, talk to the user) or salvageable with a course correction (`[DIRECTIVE]` with new guidance, spike child retries).

- **Harden child discovers the spike's approach is fundamentally wrong.** The harden child should not silently rewrite — it should append `blocked: spike approach is wrong because <reason>` and wait. The parent decides whether to relaunch the spike phase with different guidance or authorize the harden child to proceed with a rewrite via `[DIRECTIVE]`.

- **Cheat list is missing or vague.** The harden child cannot do its job. It appends `blocked: cheat list missing/insufficient` and waits. The parent either asks the spike child for a better cheat list (append `[DIRECTIVE]` to `spike.coord.md`, re-run) or provides the list directly in a `[DIRECTIVE]` on `harden.coord.md`.

## Verification

To check a spike-harden run worked:

- A `*.coord/` directory exists with `spike.coord.md` and `harden.coord.md`.
- `spike.coord.md` contains a `## Cheat list` section with specific entries.
- `harden.coord.md` shows the harden child read the cheat list and logged the disposition of each item (addressed / deferred / unnecessary).
- The final branch (spike + harden) builds and tests pass.

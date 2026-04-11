---
name: mandelbot-spike-harden
description: Use this skill when you want to build something end-to-end fast and then harden it. Delegates a spike child (cut corners, prove the approach), then the parent reads the summary and proceeds normally — plan, delegate, or implement directly. Good fit for "I want to try X" tasks where correctness matters but the shape is uncertain.
allowed-tools: [Read, Edit, Write, Bash, Glob, Grep, mcp__mandelbot__spawn_tab]
---

# Spike then harden

A delegation flow for uncertain work. A **spike** child proves an approach end-to-end as fast as possible — hardcoding, skipping tests, leaving TODOs is expected. When the spike finishes, the parent reads its summary, decides what needs hardening, and proceeds however makes sense: plan the hardening work, delegate it (likely parallelizable — cheat list items are usually independent), or do it directly if it's small.

Read `<plugin-dir>/skills/_shared/coord.md` for the shared protocol and `mandelbot-delegate`'s SKILL.md for the parent workflow. This file only covers what's specific to the spike phase.

## When to use

- The right approach is uncertain and you want to validate before investing in correctness.
- Eventual correctness matters — you can't ship the spike as-is.

Don't use when the approach is obvious (`mandelbot-delegate` directly) or when correctness doesn't matter (just spike, no hardening needed).

## Flow

### Phase 1: Spike

Follow the `mandelbot-delegate` workflow with one child labeled `spike`. In the `index.md` "How we work" section, describe:

- This is a spike-harden run. The spike child's job is to prove the approach, not to produce shippable code.
- How the prototype will be handed off: the spike child writes a `## Summary` section in its `spike.coord.md` when done, listing what it built, what it skipped, and what the real version needs. The parent will use this to plan the hardening work.

Include the [spike child framing](#spike-child-framing) in the spike's `## Assignment` and spawn prompt.

### Phase 2: Harden

When the spike child reports `done`, read `spike.coord.md` — especially the `## Summary`. You now have full context: the spike's code on its branch, the summary of shortcuts taken, and your own observations from watching the spike work. Proceed as you normally would on any project:

- If the hardening work is parallelizable (it usually is — summary items tend to be independent), plan it and delegate via `mandelbot-delegate`.
- If it's small enough to do directly, just do it.
- If the spike proved the approach is wrong, escalate.

The spike child's branch is your starting point for hardening work. Children you delegate to should branch off it.

## Spike child framing

Copy this into the spike child's assignment and prompt — it is the entire point of the spike role.

**Goal:** Prove the approach end-to-end. A working, runnable result beats a clean one. Take the fastest path to "it works on my machine." Do not merge, deploy, publish, or otherwise ship your changes — the spike is a prototype, not a release.

**Permitted shortcuts:**

- Hardcoding values (URLs, IDs, config).
- Skipping tests entirely.
- Leaving `TODO` / `FIXME` comments.
- Hacking around edge cases (assume happy path, skip retries).
- Mocking or stubbing dependencies that are not on the critical path.
- Ignoring error paths — panic, log-and-continue, whatever's fastest.
- Copy-pasting instead of refactoring.

**Required deliverables:**

1. A runnable end-to-end implementation. If you're in a git repo, the spike child pushes its branch before marking `done` — the branch name is the handoff artifact the parent and any subsequent children use as their starting point.
2. A `## Summary` section in `spike.coord.md` when done — not a new file, a section within the existing coord file.

**The summary is a first-class deliverable, not an afterthought.** It is the handoff to the parent for the hardening phase. If the spike child did not write a summary, it did not finish.

### Summary format

Fixed heading. The parent reads this section to plan hardening.

```markdown
## Summary

What was built: <brief description of what works end-to-end>

What was skipped or hacked:
- <file:line or area> — <what was hacked or skipped>
- ...
```

Each entry must be specific enough that someone reading only the summary and the diff can understand what to fix. Vague entries like "error handling" are not enough — name the function or file.

## Failure modes

- **Spike fails.** Parent decides whether to abort or course-correct via `[DIRECTIVE]`. Uses the standard block/unblock handshake from `_shared/coord.md`.
- **Spike proves the approach is wrong.** Escalate — the spike did its job by ruling the approach out cheaply.
- **Summary missing or vague.** Parent appends a `[DIRECTIVE]` asking for a better summary before considering the spike done.

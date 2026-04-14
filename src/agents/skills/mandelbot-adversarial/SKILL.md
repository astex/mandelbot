---
name: mandelbot-adversarial
description: Use this skill when correctness is adversarial — a builder agent writes code while a breaker agent writes tests trying to break it, looping until the breaker finds nothing or a round cap is hit. Good fit for algorithm-heavy, parser, or security-adjacent work where "what could go wrong" is the real risk and a single implementer is likely to miss cases.
allowed-tools: [Read, Edit, Write, Bash, Glob, Grep, mcp__mandelbot__spawn_tab, mcp__mandelbot__close_tab]
---

# Adversarial loop

A two-agent game built on top of `mandelbot-delegate`. A **builder** child writes code. A **breaker** child writes tests trying to break it. The parent alternates rounds until the breaker reports it found nothing (fixpoint) or a round cap is hit.

Read `<plugin-dir>/skills/_shared/coord.md` for the shared protocol and `mandelbot-delegate`'s SKILL.md for the parent workflow. This file covers only what's specific to the adversarial loop.

## When to use

- The work has adversarial surface area: edge cases, malformed input, concurrency, invariants under load. A single implementer will miss cases a second set of eyes would catch.
- Eventual correctness is the point, not speed. Adversarial rounds are not cheap.
- You can state the task precisely enough that a breaker knows what "break it" means. Vague tasks produce vague breaking.

Don't use when the work is straightforward CRUD, UI polish, or anything where "correctness" is mostly a function of taste rather than behaviour. Don't use when a single test suite written up front would catch everything — just write the tests.

## Knobs

- **task** — the task description (required). Must be concrete enough that both agents know when the code meets the spec.
- **round_cap** — max rounds before the parent declares a partial win (default 3). One round = builder runs, then breaker runs.

That's it. Everything else is defaulted.

## Flow

### 1. Frame the run

Create a `*.coord/` directory as in `mandelbot-delegate`. Write `index.md` with:

- The task, verbatim.
- The round cap.
- A `## Exchange` section, initially empty. This is the per-round handoff log — the parent appends to it after each child reports. Both children read `../index.md` by default, so this is their shared record of what happened in prior rounds without reading each other's coord files.

List two children in the index: `builder` and `breaker`. Children are respawned each round with round-indexed labels (`builder-1`, `breaker-1`, `builder-2`, ...) — update the children list as you spawn.

### 2. Round N

One round is two sequential children: builder first, then breaker. Do not spawn them in parallel — the breaker needs the builder's code.

**Builder phase.** Write `builder-<N>.coord.md`. The assignment includes:

- The task (verbatim from `index.md`).
- **For N > 1:** the failing tests the breaker produced in round N-1, referenced by branch name and file paths. The builder's job this round is to make those tests pass without regressing prior rounds.
- The base branch: for N=1, master; for N>1, the breaker's branch from round N-1 (so the failing tests are already in the worktree).
- Instruction to push its branch and write a `## Handoff` section in its coord file on completion. Format below.

Spawn the builder with `base: <base branch>` and wait via `watch.sh`.

**Breaker phase.** When the builder reports `done`, read its `## Handoff`. Append a `### Round N — builder` entry to `index.md`'s `## Exchange` section with the builder's branch name and one-line summary.

Then write `breaker-<N>.coord.md`. The assignment includes:

- The task (verbatim).
- The builder's branch name, used as the base — the breaker's worktree is created on a fresh branch off it, so the builder's code is already in the tree.
- Instruction to write failing tests — not to patch the code. The breaker produces tests, the builder produces code. Keep the roles separate.
- Instruction to write a `## Verdict` section in its coord file on completion. Format below.
- The base branch: the builder's branch from this round.

Spawn the breaker with `base: <builder's branch>` and wait.

### 3. Evaluate

When the breaker reports `done`, read its `## Verdict`. Append a `### Round N — breaker` entry to `index.md`'s `## Exchange`.

- **Verdict `clean`** — fixpoint. Go to step 4 with outcome "clean."
- **Verdict `failing`** — if N < round_cap, increment N and loop to step 2. If N == round_cap, go to step 4 with outcome "partial."
- **Verdict `failed`** (the breaker itself couldn't complete) — escalate; treat as a broken round, not a clean one.

### 4. Declare outcome

- **Clean.** The final builder's branch is the shippable artifact. Report the branch, the number of rounds, and the categories the breaker enumerated as attempted-and-clean.
- **Partial.** Round cap hit with failing tests still outstanding. Surface the final builder's branch, list the still-failing tests, and hand off to the user — the code is closer than it started but not done. Do not pretend otherwise.

Close any remaining child tabs.

## Builder's role

Receives: the task, and (from round 2 onward) a set of failing tests committed on the base branch.

Produces: code that implements the task and makes all prior-round tests pass. Pushes its branch. Does not modify tests written by the breaker — it makes them pass by changing production code. If a test is wrong (tests an invariant the task does not require), the builder blocks with `- [...] blocked: breaker test X contradicts spec because ...` and lets the parent adjudicate.

`## Handoff` format in the builder's coord file:

```markdown
## Handoff

Branch: <branch name>
What I built: <one or two lines>
Tests I added (if any): <file paths>
Known gaps: <anything the builder is aware of but didn't address — e.g. "does not handle UTF-16 input, task didn't require it">
```

## Breaker's role

Receives: the task, the builder's branch as the worktree base (mandelbot creates a fresh `breaker-N` branch off it, so the builder's code is already present — no dual checkout).

Produces: failing tests, committed on its own branch (branched from the builder's). Does not modify the builder's code. If the breaker cannot find anything to break after genuinely trying across the categories relevant to the task, it reports `clean` — with enumerated categories so the parent can judge the honesty of the attempt.

`## Verdict` format in the breaker's coord file:

```markdown
## Verdict

Result: failing | clean
Branch: <branch name>

Categories attempted:
- <category>: <failing | clean> — <what I tried>
- ...

Failing tests (if any):
- <test path::name>: <one-line description of what it demonstrates>
- ...
```

Categories are task-dependent — for a parser: malformed input, unicode, size limits, nesting depth; for a cache: eviction, concurrency, TTL boundaries; etc. The breaker enumerates what's relevant. A `clean` verdict with zero categories is a `failed` round in disguise — the parent should treat it as such.

## Exchange: how the two agents see each other's work

The adversarial loop needs a shared handoff record without violating the rule that children don't read each other's coord files. Three mechanisms together cover it:

1. **Git branches are the primary exchange artifact.** The builder's code and the breaker's tests live on branches. Each child's next incarnation checks out the relevant branch via the `base` parameter. Code is the ground truth; coord entries are the summary.
2. **`index.md`'s `## Exchange` section is the cross-round log.** The parent appends a round-by-round summary after each child reports (builder branch + one-liner, breaker verdict + failing test list). Both children read `../index.md` by default protocol, so this is how round N's agents learn what happened in rounds 1..N-1 without touching each other's coord files.
3. **Per-child `## Handoff` / `## Verdict` sections** live in each child's own coord file. Only the parent reads these; it relays the relevant bits into the next child's assignment and into `## Exchange`.

This keeps the protocol extension to zero new files — `index.md`'s role is already "shared parent-owned context," and a `## Exchange` section is a natural fit.

## Fixpoint

Fixpoint = breaker reports `clean` with a non-empty category list. The parent treats this as terminal success. The parent should sanity-check the category list against the task before declaring victory: a parser task with no "malformed input" category is a suspect `clean` and warrants a `[DIRECTIVE]` asking the breaker to try harder.

## Round cap

Default 3. When hit with an outstanding `failing` verdict, the outcome is `partial`: the final builder branch is surfaced, but the run did not reach fixpoint. The parent must report this honestly — do not paper over a partial as clean. A partial is still usually more correct than a single-shot implementation; the value is in the rounds that did happen, not in the one that didn't.

## Composability

The builder or breaker role can itself be a pipeline, tournament, or delegate run — spawn `mandelbot-pipeline`, `mandelbot-tournament`, or `mandelbot-delegate` from inside the builder or breaker tab. The adversarial protocol only cares that each role eventually pushes a branch and writes its `## Handoff` / `## Verdict`. How the role arrives at that output is opaque from the outside.

## Failure modes

- **Breaker patches the code instead of writing tests.** Parent appends a `[DIRECTIVE]` reminding the breaker that its role is tests, not fixes, and asks it to revert and try again.
- **Builder modifies breaker tests to make them pass.** Same — `[DIRECTIVE]` to revert and fix the production code instead. If the builder believes the test is genuinely wrong, it should have blocked, not rewritten.
- **Breaker reports `clean` with no categories.** Treat as a failed round. Respawn with a `[DIRECTIVE]` demanding enumerated categories, or escalate.
- **Builder can't make the tests pass.** Block with `blocked: test X appears impossible because ...`. Parent adjudicates — sometimes the task is under-specified and the tests reveal that, which is a valuable outcome.
- **Endless "almost clean" rounds.** The round cap catches this. Honour it.
- **Task is too vague.** Both agents will flail. If the first round's verdict is shallow or the builder blocks asking for clarification, fix the task description in `index.md` before respawning. Do not keep rounds going on a bad spec.

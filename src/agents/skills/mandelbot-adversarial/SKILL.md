---
name: mandelbot-adversarial
description: Use this skill when correctness is adversarial — a persistent builder agent writes code while fresh breaker agents each round probe it and report what's broken, looping until the breaker reports clean or a round cap is hit. Good fit for algorithm-heavy, parser, or security-adjacent work where "what could go wrong" is the real risk and a single implementer is likely to miss cases.
allowed-tools: [Read, Edit, Write, Bash, Glob, Grep, mcp__mandelbot__spawn_tab, mcp__mandelbot__close_tab]
---

# Adversarial loop

A two-role game built on top of `mandelbot-delegate`. A single **builder** child, persistent across rounds, writes code on one branch. Each round, a fresh ephemeral **breaker** child detaches HEAD at the builder's current tip, probes the code, and reports what's broken in prose. The builder reads the report, writes tests that capture the failures, fixes the code, and blocks again. One branch accumulates the whole run and is the PR.

Read `<plugin-dir>/skills/_shared/coord.md` for the shared protocol and `mandelbot-delegate`'s SKILL.md for the parent workflow. This file covers only what's specific to the adversarial loop.

## When to use

- The work has adversarial surface area: edge cases, malformed input, concurrency, invariants under load. A single implementer will miss cases a second set of eyes would catch.
- Eventual correctness is the point, not speed. Adversarial rounds are not cheap.
- You can state the task precisely enough that a breaker knows what "break it" means. Vague tasks produce vague breaking.

Don't use when the work is straightforward CRUD, UI polish, or anything where "correctness" is mostly a function of taste rather than behaviour. Don't use when a single test suite written up front would catch everything — just write the tests.

## Knobs

- **task** — the task description (required). Must be concrete enough that both roles know when the code meets the spec.
- **round_cap** — max rounds before the parent declares a partial win (default 3). One round = builder builds or fixes, then a breaker probes.

That's it. Everything else is defaulted.

## Flow

### 1. Frame the run

Create a `*.coord/` directory as in `mandelbot-delegate`. Write `index.md` with:

- The task, verbatim.
- The round cap.
- A short note in `## How we work` explaining the two roles: one persistent `builder`, and `breaker-1`, `breaker-2`, ... spawned one per round. The parent relays all cross-role context through the children's logs — there is no shared doc. See "Exchange" below.

Spawn the builder once, up front, on a fresh branch off master. The builder tab stays alive for the whole run; the parent drives it via the block/unblock handshake rather than respawning.

### 2. Round N

One round = one builder action, then one breaker probe. The builder is the same tab every round; the breaker is a new tab every round.

**Builder phase.**

- **Round 1:** the builder implements the task from scratch on its branch, pushes, writes a `## Handoff — Round 1` section to its coord file, and blocks: `- [...] blocked: round 1 complete, awaiting breaker`.
- **Round N > 1:** the builder is already blocked from round N-1. The parent appends a `[DIRECTIVE]` with the breaker's verdict — the list of reported failures with inputs, expected, actual. The builder unblocks, writes a regression test for each reported failure, confirms each fails against the current code, fixes the production code, commits (tests and fix — one commit or two, builder's call), pushes, writes `## Handoff — Round N`, and blocks again.

**Breaker phase.** When the builder blocks, spawn a fresh breaker tab (`breaker-<N>`) with its worktree based on the builder's current branch tip. The breaker's assignment in its coord file includes, for N > 1, a prior-round summary the parent has compiled from earlier builder handoffs and breaker verdicts — categories already probed, what came back `clean`, what changed since. For N = 1, no prior context is needed.

The breaker's first step is to detach HEAD so nothing it does leaves a persistent branch behind:

```bash
git checkout --detach HEAD
```

It probes the builder's code — runs the existing tests, feeds adversarial inputs to exposed APIs, reads the source for invariant violations, writes throwaway scratch scripts in the worktree to experiment. None of this is handed over; it's just how the breaker investigates.

Output is its `## Verdict` section, in prose, detailed enough that the builder can reproduce each failure without asking. It sets `**State:** done` and closes its tab.

### 3. Evaluate

Read the breaker's `## Verdict`.

- **Verdict `clean`** — fixpoint. Go to step 4 with outcome "clean". Append a final `[DIRECTIVE] done, close when ready` to the builder and let it finish.
- **Verdict `failing`** and N < round_cap — append `[DIRECTIVE]` to the builder with the failure list (inputs, expected, actual) and instructions to write regression tests, fix, and push. Loop to step 2 with N+1.
- **Verdict `failing`** and N == round_cap — go to step 4 with outcome "partial".
- **Verdict `failed`** (the breaker itself couldn't complete) — escalate; treat as a broken round.

### 4. Declare outcome

The builder's branch is the only artifact either way. One branch, one PR.

- **Clean.** Report the branch, the round count, and the categories the breaker enumerated as attempted-and-clean.
- **Partial.** Surface the branch plus the still-outstanding failures from the last verdict (not yet fixed). Hand off to the user — the code is closer than it started but not done. Do not pretend otherwise.

Close any remaining child tabs.

## Builder's role

Persistent across rounds. Receives the task up front; on round 1, implements from scratch on its branch. On round N > 1, each round begins when the parent appends a `[DIRECTIVE]` listing the breaker's reported failures. The builder:

1. For each reported failure, writes a regression test (unit, integration, or property — whatever matches the existing suite's style) that captures the described input and expected behaviour.
2. Runs the new tests to confirm they fail against current code. If a test passes unexpectedly, something in the report didn't reproduce — block and let the parent adjudicate.
3. Fixes the production code until all new tests pass and the existing suite still passes.
4. Commits and pushes.
5. Appends a fresh `## Handoff — Round N` section to its coord file.
6. Blocks with `- [...] blocked: round N complete, awaiting breaker`.

If a reported failure contradicts the spec (tests an invariant the task does not require), block with `blocked: breaker-<N> failure X contradicts spec because ...` and let the parent adjudicate — do not silently drop it.

`## Handoff — Round N` format, appended to the builder's coord file (one section per round):

```markdown
## Handoff — Round N

What I changed: <one or two lines>
Known gaps: <anything the builder is aware of but didn't address — e.g. "does not handle UTF-16 input, task didn't require it">
```

The branch name is set once at spawn and recorded in `index.md`. It does not change across rounds.

## Breaker's role

Ephemeral — one breaker tab per round. Spawned with its worktree based on the builder's current branch tip. First step: `git checkout --detach HEAD` so nothing the breaker does leaves a committed branch behind.

Produces: a prose `## Verdict` section in its coord file. Does not write tests into the suite, does not modify the builder's code, does not commit, does not push. The worktree is a scratchpad for probing — run existing tests, poke at APIs from a REPL or ad-hoc script, read the source, fuzz inputs. Throwaway scripts can live in the worktree for the duration of the run; they go away with the tab.

If the breaker cannot find anything to break after genuinely trying across the categories relevant to the task, it reports `clean` — with enumerated categories so the parent can judge the honesty of the attempt.

`## Verdict` format in the breaker's coord file:

```markdown
## Verdict

Result: failing | clean

Categories attempted:
- <category>: <failing | clean> — <what I tried>
- ...

Failures (if any):
- <short label>
  Category: <category>
  Input: <concrete input — literal bytes, structured value, or precisely described sequence>
  Expected: <what the task/spec says should happen>
  Actual: <what the code actually does — error message, returned value, hang, crash>
  Reproducer: <one-line instruction, e.g. "parse('<<<<') from an empty registry">
- ...
```

Categories are task-dependent — for a parser: malformed input, unicode, size limits, nesting depth; for a cache: eviction, concurrency, TTL boundaries; etc. The breaker enumerates what's relevant. A `clean` verdict with zero categories is a `failed` round in disguise — the parent should treat it as such.

Each failure must be concrete enough for the builder to reproduce without follow-up questions. "Something breaks with big inputs" is not a failure; "parse() returns `Ok` on a 1MB string of `<` when the spec says it should reject inputs over 64KB" is.

## Exchange: how the two roles see each other's work

Children never read each other's coord files, and there is no shared doc. The parent is the relay.

1. **The builder's branch is the substrate.** The breaker's worktree sits on the builder's current tip in detached HEAD — everything it sees of the builder is the working tree at that commit. Code is the ground truth.
2. **Verdicts are prose, not code.** The breaker hands back a description of what's broken; the builder writes the regression tests itself. Only one branch exists at the end of the run, and it contains only code the builder wrote.
3. **The parent relays context through the children's logs.** When spawning `breaker-<N>` for N > 1, the parent composes a prior-round summary into the breaker's initial coord-file assignment (earlier verdicts, categories already probed, what's changed since). When handing a new verdict back to the builder, the parent appends a `[DIRECTIVE]` into the builder's log with the failure list. Each child only ever reads its own coord file and `../index.md`.
4. **Per-child `## Handoff` / `## Verdict` sections** live in each child's own coord file for the parent to read and compose into the next relay.

## Fixpoint

Fixpoint = a breaker reports `clean` with a non-empty, on-topic category list. The parent treats this as terminal success. Sanity-check the category list against the task before declaring victory: a parser task with no "malformed input" category is a suspect `clean` and warrants a `[DIRECTIVE]` asking the breaker to try harder, or respawning with a broader category hint.

## Round cap

Default 3. When hit with an outstanding `failing` verdict, the outcome is `partial`: the builder's branch is surfaced, but the run did not reach fixpoint. Report honestly — do not paper over a partial as clean. A partial is still usually more correct than a single-shot implementation; the value is in the rounds that did happen, not in the one that didn't.

## Composability

The builder role can itself be a pipeline, tournament, or delegate run — spawn one of those from inside the builder tab. The protocol only cares that a single branch accumulates all the builder's commits and that the builder writes `## Handoff — Round N` per round. Breakers are short-lived and detached; if a breaker needs to sub-delegate probing work, the sub-tasks must all finish and their findings fold into the single `## Verdict` before the breaker closes.

## Failure modes

- **Breaker writes tests into the suite or hands over a patch.** `[DIRECTIVE]` reminding the breaker that its output is a prose verdict, nothing else. If it's already closed, discard what it left behind and respawn.
- **Breaker's failure description isn't reproducible** (vague input, missing Actual, hand-wavy category). Builder blocks with `blocked: breaker-<N> failure X not reproducible because ...`. Parent either relays back for clarification or respawns the breaker.
- **Builder fixes the bug without adding a regression test.** `[DIRECTIVE]` to add the missing test before moving on. The regression tests are the durable artifact of the loop.
- **Breaker reports `clean` with no categories.** Treat as a failed round. Respawn with a `[DIRECTIVE]` demanding enumerated categories, or escalate.
- **Builder can't make a test pass.** Block with `blocked: failure X appears impossible because ...`. Parent adjudicates — sometimes the task is under-specified and the report reveals that, which is a valuable outcome.
- **Endless "almost clean" rounds.** The round cap catches this. Honour it.
- **Task is too vague.** Both roles will flail. If round 1's verdict is shallow or the builder blocks asking for clarification, fix the task in `index.md` before continuing. Do not keep rounds going on a bad spec.

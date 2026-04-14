---
name: mandelbot-adversarial
description: Use this skill when correctness is adversarial — a persistent builder agent writes code while fresh breaker agents each round write tests trying to break it, looping until the breaker reports clean or a round cap is hit. Good fit for algorithm-heavy, parser, or security-adjacent work where "what could go wrong" is the real risk and a single implementer is likely to miss cases.
allowed-tools: [Read, Edit, Write, Bash, Glob, Grep, mcp__mandelbot__spawn_tab, mcp__mandelbot__close_tab]
---

# Adversarial loop

A two-role game built on top of `mandelbot-delegate`. A single **builder** child, persistent across rounds, writes code on one branch. Each round, a fresh ephemeral **breaker** child detaches HEAD at the builder's current tip, writes failing tests, and emits them as a patch. The builder applies the patch, makes the tests pass, and blocks again. One branch accumulates the whole run and is the PR.

Read `<plugin-dir>/skills/_shared/coord.md` for the shared protocol and `mandelbot-delegate`'s SKILL.md for the parent workflow. This file covers only what's specific to the adversarial loop.

## When to use

- The work has adversarial surface area: edge cases, malformed input, concurrency, invariants under load. A single implementer will miss cases a second set of eyes would catch.
- Eventual correctness is the point, not speed. Adversarial rounds are not cheap.
- You can state the task precisely enough that a breaker knows what "break it" means. Vague tasks produce vague breaking.

Don't use when the work is straightforward CRUD, UI polish, or anything where "correctness" is mostly a function of taste rather than behaviour. Don't use when a single test suite written up front would catch everything — just write the tests.

## Knobs

- **task** — the task description (required). Must be concrete enough that both roles know when the code meets the spec.
- **round_cap** — max rounds before the parent declares a partial win (default 3). One round = builder builds or patches, then a breaker probes.

That's it. Everything else is defaulted.

## Flow

### 1. Frame the run

Create a `*.coord/` directory as in `mandelbot-delegate`. Write `index.md` with:

- The task, verbatim.
- The round cap.
- A `## Exchange` section, initially empty. The parent appends a one-line summary per round so future breakers can see prior-round history without reading each other's coord files.

List one persistent child in `index.md` — `builder` — plus a note that per-round breakers (`breaker-1`, `breaker-2`, ...) are spawned and closed round by round. Update the children list as breakers come and go.

Spawn the builder once, up front, on a fresh branch off master. The builder tab stays alive for the whole run; the parent drives it via the block/unblock handshake rather than respawning.

### 2. Round N

One round = one builder action, then one breaker probe. The builder is the same tab every round; the breaker is a new tab every round.

**Builder phase.**

- **Round 1:** the builder implements the task from scratch on its branch, pushes, writes a `## Handoff — Round 1` section to its coord file, and blocks: `- [...] blocked: round 1 complete, awaiting breaker`.
- **Round N > 1:** the builder is already blocked from round N-1. The parent appends a `[DIRECTIVE]` naming `<coord-dir>/breaker-<N-1>.patch` with instructions to apply it, make the tests pass, commit, push, write `## Handoff — Round N`, and re-block. The builder unblocks, does the work, and blocks again.

**Breaker phase.** When the builder blocks, spawn a fresh breaker tab (`breaker-<N>`) with its worktree based on the builder's current branch tip. The breaker's first step is to detach HEAD so nothing it does leaves a persistent branch behind:

```bash
git checkout --detach HEAD
```

It writes failing test files, runs them to confirm they fail, then captures them as a patch in the coord directory (not a commit, not a push):

```bash
git diff -- <test paths> > <coord-dir>/breaker-<N>.patch
```

It writes `## Verdict` to its coord file referencing the patch path, sets `**State:** done`, and closes its tab. The parent deletes any ephemeral branch the worktree was spawned on.

### 3. Evaluate

Read the breaker's `## Verdict`. Append `### Round N — breaker` to `index.md`'s `## Exchange` with the verdict, patch path (if any), and one-line summary.

- **Verdict `clean`** — fixpoint. Go to step 4 with outcome "clean". Append a final `[DIRECTIVE] done, close when ready` to the builder and let it finish.
- **Verdict `failing`** and N < round_cap — append `[DIRECTIVE] apply <coord-dir>/breaker-<N>.patch, make it pass, push` to the builder, loop to step 2 with N+1.
- **Verdict `failing`** and N == round_cap — go to step 4 with outcome "partial".
- **Verdict `failed`** (the breaker itself couldn't complete) — escalate; treat as a broken round.

### 4. Declare outcome

The builder's branch is the only artifact either way. One branch, one PR.

- **Clean.** Report the branch, the round count, and the categories the breaker enumerated as attempted-and-clean.
- **Partial.** Surface the branch, plus the still-failing patch (not applied) or tests (applied and still failing, depending on where round_cap hit). Hand off to the user — the code is closer than it started but not done. Do not pretend otherwise.

Close any remaining child tabs.

## Builder's role

Persistent across rounds. Receives the task up front; on round 1, implements from scratch on its branch. On round N > 1, each round begins when the parent appends a `[DIRECTIVE]` naming a `breaker-<N-1>.patch` in the coord directory. The builder:

1. `git apply <coord-dir>/breaker-<N-1>.patch` to pull the new failing tests into the worktree.
2. Runs them to confirm they fail against current code.
3. Fixes the production code.
4. Commits (tests and fix — one commit or two, builder's call) and pushes.
5. Appends a fresh `## Handoff — Round N` section to its coord file.
6. Blocks with `- [...] blocked: round N complete, awaiting breaker`.

Does not modify tests once applied. If a breaker test is genuinely wrong (contradicts the spec), block with `blocked: breaker-<N>.patch test X contradicts spec because ...` and let the parent adjudicate — do not unilaterally delete or rewrite it.

`## Handoff — Round N` format, appended to the builder's coord file (one section per round):

```markdown
## Handoff — Round N

What I changed: <one or two lines>
Known gaps: <anything the builder is aware of but didn't address — e.g. "does not handle UTF-16 input, task didn't require it">
```

The branch name is set once at spawn and recorded in `index.md`. It does not change across rounds.

## Breaker's role

Ephemeral — one breaker tab per round. Spawned with its worktree based on the builder's current branch tip. First step: `git checkout --detach HEAD` so nothing the breaker does leaves a committed branch behind.

Produces: failing test files, captured as a patch at `<coord-dir>/breaker-<N>.patch`. Does not modify the builder's code. Does not commit. Does not push.

If the breaker cannot find anything to break after genuinely trying across the categories relevant to the task, it reports `clean` — with enumerated categories so the parent can judge the honesty of the attempt.

`## Verdict` format in the breaker's coord file:

```markdown
## Verdict

Result: failing | clean
Patch: <coord-dir>/breaker-<N>.patch    # only if Result: failing

Categories attempted:
- <category>: <failing | clean> — <what I tried>
- ...

Failing tests (if any):
- <test path::name>: <one-line description of what it demonstrates>
- ...
```

Categories are task-dependent — for a parser: malformed input, unicode, size limits, nesting depth; for a cache: eviction, concurrency, TTL boundaries; etc. The breaker enumerates what's relevant. A `clean` verdict with zero categories is a `failed` round in disguise — the parent should treat it as such.

## Exchange: how the two roles see each other's work

Children never read each other's coord files. What they do share:

1. **The builder's branch is the substrate.** The breaker's worktree sits on the builder's current tip in detached HEAD — everything it sees of the builder is the working tree at that commit. Code is the ground truth.
2. **Patches are the test handoff.** The breaker's failing tests come back to the builder as a patch file in the coord directory, not via a branch. The builder applies it into its own branch as its own commit. Only one branch exists at the end of the run.
3. **`index.md`'s `## Exchange` section is the cross-round log.** The parent appends a round-by-round summary. Each new breaker reads `../index.md` and picks up prior-round context without touching other children's files.
4. **Per-child `## Handoff` / `## Verdict` sections** live in each child's own coord file for the parent to read and relay.

## Fixpoint

Fixpoint = a breaker reports `clean` with a non-empty, on-topic category list. The parent treats this as terminal success. Sanity-check the category list against the task before declaring victory: a parser task with no "malformed input" category is a suspect `clean` and warrants a `[DIRECTIVE]` asking the breaker to try harder, or respawning with a broader category hint.

## Round cap

Default 3. When hit with an outstanding `failing` verdict, the outcome is `partial`: the builder's branch is surfaced, but the run did not reach fixpoint. Report honestly — do not paper over a partial as clean. A partial is still usually more correct than a single-shot implementation; the value is in the rounds that did happen, not in the one that didn't.

## Composability

The builder role can itself be a pipeline, tournament, or delegate run — spawn one of those from inside the builder tab. The protocol only cares that a single branch accumulates all the builder's commits and that the builder writes `## Handoff — Round N` per round. Breakers are short-lived and detached; if a breaker needs to sub-delegate, the sub-tasks must all finish and their findings fold into the single patch before the breaker closes.

## Failure modes

- **Breaker patches the code instead of writing tests.** `[DIRECTIVE]` reminding the breaker that its output is a patch of tests, nothing else. If it's already closed, discard the patch and respawn.
- **Builder modifies breaker tests to make them pass.** `[DIRECTIVE]` to revert and fix production code. If the test is genuinely wrong, the builder should have blocked, not rewritten.
- **Breaker reports `clean` with no categories.** Treat as a failed round. Respawn with a `[DIRECTIVE]` demanding enumerated categories, or escalate.
- **Builder can't make the tests pass.** Block with `blocked: test X appears impossible because ...`. Parent adjudicates — sometimes the task is under-specified and the tests reveal that, which is a valuable outcome.
- **Patch fails to apply.** The builder's tree has drifted from what the breaker saw (shouldn't happen if the breaker detached from the current tip, but if it does). Builder blocks; parent decides whether to regenerate the patch, rebase, or have the breaker redo the round.
- **Endless "almost clean" rounds.** The round cap catches this. Honour it.
- **Task is too vague.** Both roles will flail. If round 1's verdict is shallow or the builder blocks asking for clarification, fix the task in `index.md` before continuing. Do not keep rounds going on a bad spec.

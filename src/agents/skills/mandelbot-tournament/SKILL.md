---
name: mandelbot-tournament
description: Use this skill when you want multiple attempts at the same task and will pick the best one. Spawns N contestant agents in parallel, then a judge agent scores their outputs against user-supplied criteria and picks a winner. Good fit when the right approach is unclear or when model/prompt variation might change the outcome.
allowed-tools: [Read, Edit, Write, Bash, Glob, Grep, mcp__mandelbot__spawn_tab, mcp__mandelbot__close_tab, AskUserQuestion]
---

# Tournament

A parallel-and-select delegation flow. The parent spawns N **contestant** children on the same task — varied by model, prompt, or approach — and then spawns a **judge** child that scores them against user-supplied criteria and picks a winner. The winner's branch is surfaced to the user; losers' branches stay pushed but unused.

**This project uses git-based VCS isolation.** Each contestant runs in its own worktree on its own branch. Contestants do not share code or see each other during the run.

Read `<plugin-dir>/skills/_shared/coord.md` for the shared protocol and `mandelbot-delegate`'s SKILL.md for the parent workflow. This file covers what's specific to tournaments: framing the run, varying contestants, the judge role, and surfacing the outcome.

## When to use

- The right approach is unclear and you'd rather run several in parallel than pick one up front.
- The task is small enough that N full attempts is cheaper than the cost of picking wrong.
- You can name concrete criteria the judge can score against.

Don't use when there's a clear single approach (`mandelbot-delegate` directly), when contestants would trip over each other (the task mutates shared infra, not code), or when you have no way to tell a good result from a bad one — without criteria the judge can't do its job.

## Flow

### 1. Frame the run

Work out with the user:

- **The task.** Same description every contestant gets.
- **Judging criteria.** Concrete, scorable properties — "passes the existing test suite," "compiles without warnings," "fewer than 200 lines of diff," "doesn't touch module X." Vague criteria ("is clean," "feels right") produce vague verdicts. Aim for 3–5 criteria the judge can check by reading code and running commands.
- **N** (number of contestants, default 3). Odd numbers are fine; the judge picks one winner regardless.
- **Variation strategy** — how contestants differ from each other. Pick one:
  - **model** — same prompt, different models (e.g. Opus, Sonnet, Haiku).
  - **prompt** — same model, different framing of the task (e.g. "minimize diff," "prioritize test coverage," "use pattern X").
  - **approach** — same model and prompt, different high-level strategies named up front (e.g. "refactor in place," "add a new module," "extend the existing abstraction").

  Default: **prompt** variation, using three framings the parent picks based on the task. If nothing distinguishes the contestants, you're running the same attempt N times — pick a variation axis or don't use this skill.

### 2. Create the coordination directory

```bash
mkdir -p ~/.mandelbot/coordination/<project>.coord
```

Write `index.md` from `<plugin-dir>/skills/_shared/index.template.md`. In "How we work":

- Note that this is a tournament run.
- State the task, the judging criteria, the variation strategy, and N.
- Include the [contestant framing](#contestant-framing) so contestants know to write a `## Summary`.
- List the children: `contestant-1` … `contestant-N`, plus `judge` (spawned later, but named in advance so the structure is visible).

Write `contestant-<i>.coord.md` for each contestant from `child.template.md`. Each gets the same `## Assignment` task text, plus the variation applied to that contestant (different model noted in the spawn call, different prompt framing in the assignment, or different named approach in the assignment).

Do not write the judge's coord file yet — the judge is spawned after contestants settle and its assignment depends on their outputs.

### 3. Spawn contestants in parallel

Spawn all N contestants in a single round via `spawn_tab`, each with its own `branch` parameter and a prompt in the standard subtask shape:

> Start by running `/mandelbot-work-as-subtask` to load the subtask protocol. You are a contestant agent in the "<project>" tournament. Your coordination file is at `<absolute path to contestant-<i>.coord.md>` — read it first, then read the governing plan at `<path>` in full.
>
> You are contestant <i> of <N>. Your job: <one-line task summary>. Write a `## Summary` section in your coord file before marking `done` — the judge will read it.

If the variation strategy is `model`, pass the model via `spawn_tab`'s model parameter.

### 4. Watch contestants

Run one `watch.sh` per contestant in the background (see `mandelbot-delegate` for the pattern). Handle blocks via `[DIRECTIVE]` as usual.

**Keep contestants isolated.** Do not share information from one contestant's coord file into another's. Answer blocks using only what the parent and the governing plan provide. Cross-pollination defeats the point — you want independent attempts.

Wait until every contestant has reached `done` or `failed`.

### 5. Spawn the judge

Write `judge.coord.md` from `child.template.md`. The `## Assignment` should include:

- The task (copied from the contestants' assignment).
- The judging criteria.
- Absolute paths to every contestant's `*.coord.md` (the judge will read each contestant's `## Summary`).
- Branch names of every contestant (the judge will check them out to read the code).
- The [judge framing](#judge-framing).

Spawn the judge via `spawn_tab` on its own branch with a prompt in the standard subtask shape. The judge runs alone — no parallelism at this stage.

### 6. Read the verdict and surface the outcome

When the judge reports `done`, read `judge.coord.md`'s `## Verdict`. It names the winner, scores each contestant against the criteria, and explains the pick.

Surface to the user:

- Winner's branch name and a one-paragraph summary of why it won.
- Per-contestant score table from the verdict.
- A link to the verdict in `judge.coord.md` for details.

Ask the user via `AskUserQuestion` whether to open a PR from the winning branch, pick a different contestant, or discard the run. Losers' branches stay pushed (they're artifacts of the run) but nothing is merged from them.

### 7. Wrap up

Close any still-open child tabs via `close_tab`. If the user asked for a PR, open it from the winning branch with the verdict's rationale in the PR description.

## Contestant framing

Copy this into each contestant's assignment.

**Goal:** Produce a working, shippable attempt at the task. You are one of several independent contestants; a judge will compare your result against the others against fixed criteria. You will not see the others' work.

**Required deliverables:**

1. Your implementation, committed and pushed on your branch. The branch is the artifact the judge reads.
2. A `## Summary` section in your coord file before you mark `done`. The judge reads this before diving into the code.

### Summary format

```markdown
## Summary

Approach: <1–3 sentences on the strategy you took>

What you built: <what works, at a glance>

Tradeoffs: <what you optimized for and what you gave up>

How to verify: <commands or paths that let the judge check your claims — tests to run, files to diff, etc.>
```

Keep it tight. The judge uses this as a map into your branch, not as the evaluation itself.

## Judge framing

Copy this into the judge's assignment.

**Goal:** Score each contestant against the fixed criteria and pick a winner. Be specific, be consistent, and prefer evidence from the code over impressions from the summaries.

**What you read:**

1. Each contestant's `## Summary` in their `*.coord.md` (paths provided in your assignment).
2. Each contestant's branch — check out the branch, read the diff, run tests or tooling as the criteria require.

**What you do:**

- Score every contestant on every criterion. Use a consistent scale (e.g. 1–5 or pass/fail) and apply it the same way to each contestant.
- Note each score's evidence — a file, a test result, a line of diff. A score without evidence is not a score.
- Pick one winner. Ties go to the contestant with the clearest evidence; if that's still ambiguous, break ties on diff size (smaller wins).

**What you write:**

Before marking `done`, write a `## Verdict` section in `judge.coord.md`:

```markdown
## Verdict

Winner: contestant-<i> (branch: <branch-name>)

Why: <1–3 sentences naming the criteria that decided it>

Scores:

| Contestant | <criterion 1> | <criterion 2> | ... | Total |
| ---------- | ------------- | ------------- | --- | ----- |
| contestant-1 | <score + brief evidence> | ... | ... | ... |
| ...          | ...                      | ... | ... | ... |

Notes: <anything the parent should know — disqualifications, near-ties, criteria that didn't discriminate>
```

You do not open PRs, merge branches, or modify contestants' code. Your output is the verdict.

## Failure modes

- **Some contestants fail.** The run still has a winner as long as at least one contestant reaches `done`. If every contestant failed, escalate to the user — the task framing or criteria are probably wrong.
- **Judge can't pick a winner.** The criteria didn't discriminate. The judge writes the verdict anyway with notes on what went wrong; the parent surfaces this to the user and asks for sharper criteria or a rerun.
- **All contestants produce the same solution.** The variation strategy was too weak — usually happens with `prompt` variation where the framings were too similar. Note this to the user and suggest a stronger axis (different models, or distinct named approaches) if they want to rerun.
- **Judge is biased by summary quality.** A contestant with a well-written summary can out-market a contestant with better code. The judge framing tells the judge to prefer code evidence, but watch for this when reading the verdict — if the rationale leans on summary prose rather than diffs or test results, push back via `[DIRECTIVE]`.

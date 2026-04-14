---
name: mandelbot-pipeline
description: Use this skill for sequential multi-stage work where each stage's output becomes the next stage's input — a Unix pipe for agents. Stages like `spec | planner | 3× implementer | reviewer`. Activates when the user describes work as a chain of phases that must happen in order, each consuming the prior one's result.
allowed-tools: [Read, Edit, Write, Bash, Glob, Grep, mcp__mandelbot__spawn_tab, mcp__mandelbot__close_tab]
---

# Pipeline

A sequential composition built on top of `mandelbot-delegate`. The parent defines an ordered list of **stages**. Each stage runs as its own child agent, writes a `## Output` section in its coord file, and the parent pipes that output forward as context for the next stage's assignment. A stage can fan out to N parallel agents (`3× implementer`); the parent collects their outputs before advancing.

**This project uses git-based VCS isolation.** Each stage agent runs in its own worktree. Later stages branch off the prior stage's branch so code changes accumulate down the pipe.

Read `<plugin-dir>/skills/_shared/coord.md` for the shared protocol and `mandelbot-delegate`'s SKILL.md for the parent workflow. This file covers what is specific to the pipeline flow: stage definition, the output handoff, fan-out, and failure handling.

## When to use

- The work decomposes into an ordered chain of phases where each phase needs the prior one's output.
- The shape of each phase is reasonably clear up front (you can write a role description for it).
- You want intermediate artifacts — spec, plan, implementation, review — kept as distinct, inspectable handoffs rather than collapsed into one agent.

Don't use when:

- The work is parallelizable with no ordering — use `mandelbot-delegate` directly.
- You want N independent attempts at the same thing — use `mandelbot-tournament`.
- The shape is uncertain and you want to prove the approach first — use `mandelbot-spike-harden`.

## Flow

### 1. Define the pipeline

Write the stage list before spawning anything. Each stage has:

- **Label** — short identifier (`spec`, `planner`, `impl`, `reviewer`). Labels must be unique across the pipeline.
- **Role description** — what this stage does, what it reads, what it produces. This becomes the stage's assignment.
- **Fan-out count** (optional, default 1) — N parallel agents for this stage.

Record the pipeline in the plan file. A minimal pipeline:

```
1. spec       — turn the user's request into a concrete spec.
2. planner    — read spec, produce an implementation plan.
3. impl (×3)  — read plan, implement; three parallel attempts.
4. reviewer   — read plan + all three impl outputs, pick the best and note issues.
```

### 2. Create the coordination directory

```bash
mkdir -p ~/.mandelbot/coordination/<project>.coord
```

Write `index.md` from `<plugin-dir>/skills/_shared/index.template.md`. In "How we work," include:

- This is a pipeline run. Stages run sequentially. Each stage writes a `## Output` section in its coord file before marking `done` (see [Output format](#output-format)).
- The stage list and order.
- File ownership: each stage owns its own branch and commits on top of the prior stage's branch. Stages do not edit each other's coord files.

In the "Children" list, enumerate every stage agent you'll spawn — including each fan-out replica. Naming convention: `<N>-<label>` for singleton stages, `<N>-<label>-<i>` for the i-th replica of a fan-out stage (1-indexed). Example:

```
- **1-spec** (spec) — [link](./1-spec.coord.md)
- **2-planner** (planner) — [link](./2-planner.coord.md)
- **3-impl-1** (impl) — [link](./3-impl-1.coord.md)
- **3-impl-2** (impl) — [link](./3-impl-2.coord.md)
- **3-impl-3** (impl) — [link](./3-impl-3.coord.md)
- **4-reviewer** (reviewer) — [link](./4-reviewer.coord.md)
```

You do not have to create every stage's `<label>.coord.md` up front — only the first stage. Later stages' coord files are written by the parent right before spawning them, because their assignments include the prior stage's output.

### 3. Run stage N

For each stage, in order:

1. **Write the stage's `<N>-<label>.coord.md`** from `child.template.md`. The `## Assignment` section must contain the stage's role description, the prior stage's output inlined verbatim under a clear header (see [Injecting prior output](#injecting-prior-output)), and an explicit instruction to write a `## Output` section in this coord file before marking `done` (see [Output format](#output-format)). Stage agents follow the standard subtask protocol — they don't know they're in a pipeline, so the output contract must live in their assignment.
2. **Spawn the stage's agent** via `spawn_tab` with a `branch` parameter and a prompt instructing it to run `/mandelbot-work-as-subtask` first (same shape as `mandelbot-delegate`). Pass `base: "<prior stage's branch>"` so the new worktree stacks on top of the prior stage's commits. The first stage branches off your default base.
3. **Watch the stage's coord file** using `watch.sh` in the background. Handle any `blocked:` entries with `[DIRECTIVE]` as usual.
4. **When the stage marks `done`**, extract its `## Output` section (see [Extracting output](#extracting-output)) and hold it for stage N+1.

For **fan-out stages** (count > 1), write one coord file per replica (`<N>-<label>-<i>.coord.md`), spawn all replicas in parallel (each on its own branch, all branching off the prior stage's branch), watch all of them, and wait until every replica has settled before advancing. See [Fan-out](#fan-out) for how their outputs combine.

### 4. Finalize

When the last stage is `done`:

- The final stage's branch is the pipeline's output branch. Merge, PR, or hand it off per whatever the project's integration strategy is.
- Close any remaining stage tabs via `close_tab`.

## Output format

Every stage agent must end its work by writing a `## Output` section into its own coord file (append it before `## Log`) and **then** marking `done`. Fixed heading, no variants — the parent greps for `## Output` to extract it.

```markdown
## Output

<the stage's deliverable as prose, structured however makes sense for this stage: spec text, plan bullets, a diff summary, a review verdict, etc.>
```

The output is the contract between stages. It must be self-contained — the next stage should be able to do its job by reading only the role description and this output. Do not rely on the next stage reading the prior stage's branch or log (code changes flow through git via branch stacking; reasoning and decisions flow through the `## Output` section).

**The output is a first-class deliverable, not an afterthought.** If a stage marks `done` without an `## Output` section, the parent treats that as a failure and re-opens it via `[DIRECTIVE]`.

## Injecting prior output

When writing stage N's coord file, inline the prior stage's output under a clear header in the `## Assignment`. For singleton prior stages:

```markdown
## Assignment

<role description for this stage>

### Input: output from `<prior-label>`

<prior stage's `## Output` section, inlined verbatim>
```

For fan-out prior stages, inline each replica's output under its own sub-header:

```markdown
### Input: outputs from `<prior-label>` (3 replicas)

#### From `<prior-label>-1`

<replica 1's output>

#### From `<prior-label>-2`

<replica 2's output>

#### From `<prior-label>-3`

<replica 3's output>
```

Inlining verbatim is intentional. The stage agent reads only its own coord file, so the output must live there — not referenced by path.

## Extracting output

When a stage marks `done`, read its coord file and extract everything under the `## Output` heading up to the next `##` heading (or end-of-file). That substring is what you inline into the next stage's assignment. Do not edit or summarize it in transit — the stage agent chose its phrasing deliberately.

## Fan-out

A fan-out stage spawns N parallel agents that all receive the same assignment (same role description, same prior output). Each runs on its own branch, all branched off the prior stage's branch.

When every replica is `done`, collect every replica's `## Output` and inline all of them into the next stage's assignment under per-replica sub-headers (see [Injecting prior output](#injecting-prior-output)).

**Default merge strategy: concatenation.** The parent does not synthesize fan-out outputs itself. The next stage sees all replicas side-by-side and is responsible for reconciling them. This keeps the pipeline transparent: no hidden synthesis, and the reconciling logic lives in an explicit stage the user can reason about.

**If you want synthesis**, add it as its own stage. A common shape: `... | 3× impl | reviewer | ...` — the reviewer stage reads all three implementations and picks, merges, or critiques. Don't build a smart merger into the parent; compose one into the pipeline.

**Branch handling for fan-out.** Each replica's branch contains its own code changes. The next stage can only branch off one of them. Default: branch off the first successful replica and let that stage's agent decide what to do with the others (cherry-pick, ignore, etc.) based on the outputs it receives. If the fan-out is followed by a reviewer whose job is to pick a winner, the reviewer's output can name the winning replica's branch, and the stage after that branches off the named branch. Record this convention in the reviewer's role description.

## Failure modes

- **Stage fails.** Default: abort the pipeline and escalate. The prior stages' outputs and branches are preserved — the user can resume by rewriting the failed stage's role description and re-running from there. Do not silently skip a failed stage; later stages would have no input.
- **Stage marks `done` without `## Output`.** Append `- [...] [DIRECTIVE] please write the required ## Output section before closing` in the stage's coord file. Re-arm the watcher.
- **Stage's output is empty or obviously wrong.** Append a `[DIRECTIVE]` asking for a better output. If the stage insists its output is correct, escalate to the user before advancing.
- **Fan-out partial failure.** If some replicas failed and some succeeded, pass the successful ones forward with a note in the next stage's assignment (under a `### Note` sub-header) listing which replicas failed and why. If all replicas failed, treat as full stage failure and abort.
- **Stage is blocked.** Use the standard block/unblock handshake from `_shared/coord.md`. The pipeline does not advance until the stage is unblocked and completes.

## Composition

Pipeline stages compose with other loop skills. A stage's role description can instruct the stage agent to itself run `mandelbot-tournament`, `mandelbot-adversarial`, or another `mandelbot-pipeline` — the stage becomes a sub-pipeline and its `## Output` is the composed result. Likewise, a pipeline can be one contestant in a tournament, or one side of an adversarial pair. Keep each skill's own protocol intact; compose by nesting, not by mixing.

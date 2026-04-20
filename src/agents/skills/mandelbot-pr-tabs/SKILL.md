---
name: mandelbot-pr-tabs
description: Use this skill from a project or task tab to map the user's open PRs onto a tree of tabs — one tab per conceptual group of authored PRs (with per-PR leaf tabs), plus a separate subtree for PRs awaiting the user's review. Good for starting the day, triaging in-flight work, or catching up after being away.
allowed-tools: [Bash, Read, mcp__mandelbot__spawn_tab]
---

# PRs to tabs

Survey the user's open GitHub PRs and spawn a tab tree that mirrors how those PRs cluster conceptually. Each leaf tab is pre-loaded on a single PR and waiting for the user; each group tab is a coordinator that recursively spawns its own children.

## When to use

Run this from a **project tab** or **task tab** in a git repo that has `gh` installed and authenticated. You need to be inside a worktree so spawned tabs inherit the project's checkout and can `gh pr checkout` cleanly.

Not for one-off status checks ("what PRs do I have?") — use `gh pr list` directly for those. Use this skill when the user wants a *workspace* built out, not a report.

## Dependencies

- `gh` (GitHub CLI, authenticated — `gh auth status` must succeed)
- `jq`

If either is missing, tell the user and stop.

## Workflow

### 1. Gather the PR lists

Two independent queries. Run them in parallel.

**Authored by the user** (in the current repo):

```bash
gh pr list --author @me --state open \
  --json number,title,url,headRefName,baseRefName,isDraft,body \
  --limit 100
```

**Awaiting review from the user** (in the current repo):

```bash
gh pr list --search "review-requested:@me" --state open \
  --json number,title,url,author --limit 100
```

Both queries are scoped to the current repo — `gh pr list` defaults to the repo of the current working directory. Do not cross repo boundaries; the spawned tabs inherit this worktree and can only `gh pr checkout` PRs from this repo.

If either list is empty, skip the corresponding section below — don't spawn empty coordinator tabs.

### 2. Group the authored PRs

Cluster the authored PRs into conceptual groups. Use these signals, in rough order of confidence:

1. **Stacked / chained PRs** — if PR B's `baseRefName` equals PR A's `headRefName`, they are on the same stack and **must go in the same group**. Follow chains transitively.
2. **Shared topic** — PRs whose titles, branches, or bodies reference the same feature, bug, or subsystem (e.g. "auth refactor", "billing v2", same issue number). Use judgment; titles alone are usually enough.
3. **Everything else** — singleton PRs that don't obviously belong with anything else each become their own group.

**Subgrouping.** If a single group has more than ~5 PRs, or there are more than ~5 top-level groups, introduce a second level: split the big group into sub-topics, or bucket singletons into a "misc" group. The goal is that no tab has more than a handful of direct children.

Pick a short label (a few words) for each group — it becomes a tab title.

### 3. Spawn a tab per top-level authored group

For each top-level group, call `spawn_tab` with a prompt that tells the child to recursively fan out. Pass the PR list inline so the child doesn't need to re-query.

Prompt template for a group tab:

> You are a coordinator tab for the "<group label>" group of PRs. Your job is to spawn one child tab per PR (or per subgroup, if the list below is itself grouped) and then wait for the user.
>
> PRs in this group:
> - #<number> — <title> — <url>
> - ...
>
> For each PR, call `spawn_tab` with a prompt like:
>
> > A PR needs your attention: <url> — "<title>". Run `gh pr checkout <number>` to bring it into this worktree, then read the PR description with `gh pr view <number>` and the diff with `gh pr diff <number>`. Do not take any action on the PR. Once you've loaded the context, set an informative tab title and wait for the user.
>
> If the list above is subgrouped, spawn one child per subgroup instead, and give that child the same recursive instructions with its own sub-list.
>
> After spawning your children, set your tab title to "<group label>" and wait for the user — do not close yourself.

Use the group label as the spawned tab's initial focus. Do not pre-check-out anything in the coordinator itself; leave the worktree untouched for children.

### 4. Spawn the review-requested subtree

If the review-requested list is non-empty, spawn **one** task tab that will fan out per PR and also run the git monitor. Prompt template:

> You are the "PRs awaiting my review" coordinator. Do two things:
>
> 1. For each PR below, call `spawn_tab` with a prompt that tells the child to `gh pr checkout <number>`, read the PR description and diff, set an informative tab title, and wait for the user without taking action.
>
>    PRs awaiting review:
>    - #<number> — <title> (by @<author>) — <url>
>    - ...
>
> 2. Once all child tabs are spawned, invoke `/mandelbot-git-monitor` so new review requests in this repo surface as toasts.
>
> Set your tab title to "Reviews" and stay alive.

Do **not** group the review-requested PRs — the user wants one tab per PR plus the monitor.

### 5. Report back

Once all top-level tabs are spawned, give the user a one-line summary: how many authored groups, how many total authored PRs, how many review-requested PRs, and whether the monitor was kicked off. Then stop — don't wait or poll. The user will navigate into whichever tab they want to work on.

## Notes

- Do not `gh pr checkout` in the tab running this skill — you'd dirty the current worktree and block the children.
- If `gh search prs` fails (network, rate limit), spawn the authored tree anyway and tell the user that the review list was skipped.
- Re-running the skill later will happily create duplicate tabs. If the user wants idempotency, they should close the old tabs first; don't try to reconcile.

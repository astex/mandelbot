---
name: mandelbot-git-monitor
description: Use this skill from a project tab when the user wants to be notified about GitHub PR activity in this project's repo — a PR newly requests their review, or a PR they authored gets a review decision change. Polls `gh` in the background and raises one toast per affected PR via the `notify` MCP tool; the Open button spawns a task-tab child in this project's worktree.
allowed-tools: [Bash, BashOutput, KillBash, Read, mcp__mandelbot__notify]
---

# Git PR monitor

Run the bundled `watch-prs.sh` script in the background, scoped to this project's GitHub repo. It blocks until at least one PR event concerning the user lands in that repo, prints the events to stdout, and exits. For each event, raise a toast via the `notify` MCP tool with a prompt that tells the spawned child task tab to check out the PR; then re-arm the watcher and go back to waiting.

## When to use

Use this from a **project tab**. The watcher is scoped to the project's repo so that when the user clicks the toast's Open button, the resulting child is a task tab inheriting the project's worktree and can `gh pr checkout` cleanly. Task tabs should not run this (they're already scoped to a single PR); the home tab cannot (its toasts can't spawn children).

Don't use this for one-shot status checks ("what PRs are waiting on me?") — use `gh` directly for those. This skill is the background-watcher loop.

## Dependencies

- `gh` (GitHub CLI, authenticated — `gh auth status` must succeed)
- `jq`

The watcher exits 2 on startup if either is missing, if gh isn't logged in, or if the `<owner/repo>` argument is malformed. Surface that to the user; do not loop.

## Workflow

### 1. Start the watcher in the background

Invoke with `run_in_background: true`:

```bash
bash <plugin-dir>/skills/mandelbot-git-monitor/watch-prs.sh
```

The script resolves the GitHub repo from the current working directory. It watches that repo and that repo only — that's deliberate, so the toast's Open button always spawns a child in a worktree where `gh pr checkout` works.

State is cached per-repo at `~/.mandelbot/git-monitor/<owner>-<repo>.state.json` across runs. On the very first run for a given repo it records a baseline snapshot and treats everything currently open as already seen, so the user is not flooded with pre-existing review requests.

Polling interval defaults to 60 s and can be overridden by setting `POLL_INTERVAL=<seconds>` before the `bash` invocation.

### 2. Wait for it to exit

The script exits 0 as soon as it finds at least one change, with one line per change on stdout in TSV form:

```
<kind>\t<pr url>\t<pr title>\t<detail>
```

`<kind>` is either `review_requested` (a PR newly needs your review) or `status_changed` (a PR you authored changed review decision, e.g. `APPROVED` → `CHANGES_REQUESTED`). `<detail>` carries the human-readable specifics.

Exit 2 means a setup problem (dependencies, auth, cwd isn't a GitHub repo). Exit 3 means the API has been unreachable for multiple consecutive polls. In both cases tell the user and stop — do not blindly relaunch.

### 3. Re-arm the watcher immediately

As soon as the watcher exits, capture its stdout, then **relaunch the watcher in the background before doing anything else**. The persisted state file means the new invocation picks up exactly where the old one stopped — anything already reported won't re-fire, and anything that lands while you're dispatching notifications gets caught by the fresh loop instead of slipping through the gap.

### 4. Notify once per line

Now, for each TSV line captured from the previous run, call `mcp__mandelbot__notify` exactly once with:

- **message** — a short headline, e.g. `Review requested: <title>` or `<title>: <detail>`.
- **prompt** — an instruction to the child task tab that opens when the user clicks the toast's Open button. The child spawns in this project's worktree, so it can `gh pr checkout` the PR directly. Example:

    > A PR needs your attention in this project: <url> — "<title>" (<detail>). Run `gh pr checkout <number>` to bring it into this worktree, read through the diff, and then wait for the user. Do not take any action on the PR until they direct you.

Include the URL verbatim so the child tab can find the PR without guesswork.

Once notifications are dispatched, go back to step 2 and wait for the next exit. Keep this going until the user says to stop or closes the tab.

## Notes

The script makes one GraphQL request per poll — very light. Do not wrap it in your own polling or re-invoke it while an instance is already running in the background.

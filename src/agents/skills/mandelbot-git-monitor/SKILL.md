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

### 1. Resolve the repo

Inside the project's working directory, determine `<owner/repo>` for the watcher:

```bash
gh repo view --json nameWithOwner --jq .nameWithOwner
```

If that fails (the project isn't a GitHub repo, or the user hasn't set a remote), surface the error and stop.

### 2. Start the watcher in the background

Invoke with `run_in_background: true`:

```bash
bash <plugin-dir>/skills/mandelbot-git-monitor/watch-prs.sh <owner/repo>
```

State is cached per-repo at `~/.mandelbot/git-monitor/<owner>-<repo>.state.json` across runs. On the very first run for a given repo it records a baseline snapshot and treats everything currently open as already seen, so the user is not flooded with pre-existing review requests.

Polling interval defaults to 60 s and can be overridden by setting `POLL_INTERVAL=<seconds>` before the `bash` invocation.

### 3. Wait for it to exit

The script exits 0 as soon as it finds at least one change, with one line per change on stdout in TSV form:

```
<kind>\t<pr url>\t<pr title>\t<detail>
```

`<kind>` is either `review_requested` (a PR newly needs your review) or `status_changed` (a PR you authored changed review decision, e.g. `APPROVED` → `CHANGES_REQUESTED`). `<detail>` carries the human-readable specifics.

Exit 2 means a setup problem (dependencies, auth, bad repo arg). Exit 3 means the API has been unreachable for multiple consecutive polls. In both cases tell the user and stop — do not blindly relaunch.

### 4. Notify once per line

For each TSV line, call `mcp__mandelbot__notify` exactly once with:

- **message** — a short headline, e.g. `Review requested: <title>` or `<title>: <detail>`.
- **prompt** — an instruction to the child task tab that opens when the user clicks the toast's Open button. The child spawns in this project's worktree, so it can `gh pr checkout` the PR directly. Example:

    > A PR needs your attention in this project: <url> — "<title>" (<detail>). Run `gh pr checkout <number>` to bring it into this worktree, read through the diff, and then wait for the user. Do not take any action on the PR until they direct you.

Include the URL verbatim so the child tab can find the PR without guesswork.

### 5. Requeue and keep waiting

After all notifications are dispatched, start the watcher again in the background (same command, same `<owner/repo>`) and return to step 3. The per-repo state file guarantees no event is reported twice across restarts.

Keep this going until the user says to stop or closes the project tab.

## Notes

- One watcher per project tab. If the user wants monitoring for another repo, they should start the skill from that repo's project tab — the state files don't collide because they're keyed by repo.
- The script makes one GraphQL request per poll — very light. Do not wrap it in your own polling or re-invoke it while an instance is already running in the background.

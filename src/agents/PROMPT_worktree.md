<system-reminder>
CRITICAL — YOU ARE RUNNING IN A GIT WORKTREE. DO NOT FORGET THIS.

Your current working directory is a git worktree created specifically for this task — it is NOT the main project checkout. The worktree lives on its own branch, isolated from the main repo and from sibling tabs. This remains true for the entire session, including after any context compaction.

What this means — re-read this if you get confused about where you are:
- DO NOT `cd` out of the worktree to "get back to the real repo." You are already in the right place. The worktree IS where your work lives — commit, push, and open PRs from here.
- DO NOT assume the main branch checkout reflects your changes, or vice versa. Edits here do not appear in other worktrees until committed and merged.
- `git switch` is fine within this worktree. But if you try to switch to a branch that a sibling worktree already has checked out, git will refuse. In that case, overlay the files instead: `git restore -s <ref> -- <paths>` (or `git checkout <ref> -- <paths>`) updates the working tree without moving HEAD — useful for bisecting, reproducing a bug, or comparing behavior. Reset with `git restore -s HEAD -- <paths>` when done.
- If you are ever unsure, run `git rev-parse --show-toplevel` and `git worktree list` to confirm — but the answer is always: you are in a worktree, stay in it.
</system-reminder>

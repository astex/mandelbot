---
name: mandelbot-plan-review
description: Use mandelbot's plan-review overlay to exit plan mode. Replaces ExitPlanMode inside mandelbot tabs.
---

# mandelbot-plan-review

Inside mandelbot, plan mode is exited via the `mandelbot__review_plan` MCP tool, not the built-in `ExitPlanMode`. The user reviews the plan in a markdown overlay and accepts, rejects, or sends structured feedback.

## Protocol

1. While in plan mode, write your plan to a file under `~/.claude/plans/` (or wherever your plan-mode workflow puts it).
2. Immediately call `mandelbot__set_plan` with the absolute path. Mandelbot reads the file directly each time the overlay opens, so the path is all it needs.
3. When you are ready to exit plan mode, call `mandelbot__review_plan` (no arguments). This blocks until the user responds. The tool result will be one of:
   - **accept** — The user approved. Exit plan mode and start implementing.
   - **reject** — The user rejected. Return to the prompt and discuss before proposing a new plan.
   - **feedback** — The user attached structured comments. Treat as a denial: read the comments, address them, update the plan file, and re-call `mandelbot__review_plan`.

## Rules

- **Never** call `ExitPlanMode` inside mandelbot. It is left in the tool list for compatibility but must not be used here.
- Always call `mandelbot__set_plan` *before* `mandelbot__review_plan`. Without a registered plan path the overlay has nothing to render.
- If you rewrite the plan file, you do not need to call `set_plan` again unless the path changed — mandelbot re-reads the file when the overlay opens.

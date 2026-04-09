<system-reminder>
This is a project tab. Do NOT use the mandelbot MCP set_title tool to change the tab title.
</system-reminder>

<system-reminder>
When you have a gating question for the user — something that actually blocks what you do next — use the AskUserQuestion tool rather than asking in prose. In mandelbot, this sets the tab to a visible blocked state so the human (and any parent or watchdog agent) knows the tab is waiting on them. Prose questions don't surface this way. Reserve prose for context or non-blocking clarifications.
</system-reminder>

<system-reminder>
You can spawn task agents within this project using the mandelbot MCP spawn_tab tool (no arguments needed). The tool returns the new tab's ID. Note: spawn_tab creates a new visible tab (a full Claude Code session). The built-in Agent tool is different — it runs a lightweight subagent within your session. Use whichever fits the task.
</system-reminder>

<system-reminder>
CRITICAL: Before or alongside your first response, use the mandelbot MCP set_title tool to set a short tab title (a few words) describing the task.
</system-reminder>

<system-reminder>
If the focus of this session shifts, update the tab title with the mandelbot MCP set_title tool to reflect the current task. Do not overwrite a title the user has explicitly set.
</system-reminder>

<system-reminder>
When you have a gating question for the user — something that actually blocks what you do next — use the AskUserQuestion tool rather than asking in prose. In mandelbot, this sets the tab to a visible blocked state so the human (and any parent or watchdog agent) knows the tab is waiting on them. Prose questions don't surface this way. Reserve prose for context or non-blocking clarifications.
</system-reminder>

<system-reminder>
You can spawn child task agents using the mandelbot MCP spawn_tab tool (no arguments needed). Use this when you have parallelizable sub-tasks that benefit from their own tab and full Claude Code session. Child tasks will be nested under you in the tab bar. Note: the built-in Agent tool is different — it runs a lightweight subagent within your session. Use whichever fits the task.

When delegating parallel work to child agents, use the mandelbot-delegate skill to coordinate them via a shared status file.
</system-reminder>

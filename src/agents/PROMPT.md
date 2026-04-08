<system-reminder>
CRITICAL: Before or alongside your first response, use the mandelbot MCP set_title tool to set a short tab title (a few words) describing the task.
</system-reminder>

<system-reminder>
If the focus of this session shifts, update the tab title with the mandelbot MCP set_title tool to reflect the current task. Do not overwrite a title the user has explicitly set.
</system-reminder>

<system-reminder>
You can spawn child task agents using the mandelbot MCP spawn_tab tool (no arguments needed). Use this when you have parallelizable sub-tasks that benefit from their own tab and full Claude Code session. Child tasks will be nested under you in the tab bar. Note: the built-in Agent tool is different — it runs a lightweight subagent within your session. Use whichever fits the task.

When delegating parallel work to child agents, use the mandelbot-delegate skill to coordinate them via a shared status file.
</system-reminder>

<system-reminder>
When writing or updating a plan file in plan mode, immediately call the `mandelbot__set_plan` MCP tool with the absolute path to the plan file. Mandelbot reads the file directly when displaying the plan, so it must know where to find it.
</system-reminder>

<system-reminder>
Never call `ExitPlanMode` inside mandelbot. To exit plan mode, call `mandelbot__review_plan` instead — this opens the plan-review overlay and waits for the user's decision. Ensure you have already called `mandelbot__set_plan` with the current plan file path before calling `mandelbot__review_plan`.
</system-reminder>

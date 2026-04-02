<system-reminder>
CRITICAL: Before or alongside your first response, use the mandelbot MCP set_title tool to set a short tab title (a few words) describing the task.
</system-reminder>

<system-reminder>
If the focus of this session shifts, update the tab title with the mandelbot MCP set_title tool to reflect the current task. Do not overwrite a title the user has explicitly set.
</system-reminder>

<system-reminder>
You can spawn child task agents using the mandelbot MCP spawn_tab tool (no arguments needed). Use this when you have parallelizable sub-tasks that benefit from their own tab and full Claude Code session. Child tasks will be nested under you in the tab bar. Note: the built-in Agent tool is different — it runs a lightweight subagent within your session. Use whichever fits the task.

When spawning a tab in response to a user request, use the focus_tab tool afterward to focus the new tab so the user lands there.

When delegating parallel work to child agents, use the mandelbot-delegate skill to coordinate them via a shared status file.
</system-reminder>

<system-reminder>
You can use the mandelbot MCP get_tab_tree tool to see all tabs (IDs, titles, ranks, statuses, and which is active). It also returns your own tab ID. Use the focus_tab tool to focus a tab by ID — this only works if you are the currently active tab (it no-ops otherwise to prevent stealing focus).

If the user asks you to do something outside your scope (e.g., working in the repo root instead of your worktree, or something that belongs in a parent/project tab), explain briefly why you can't do it here and use focus_tab to redirect them to the appropriate tab. Use get_tab_tree to find the right tab if needed.
</system-reminder>

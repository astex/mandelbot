<system-reminder>
This is a project tab. Do NOT use the mandelbot MCP set_title tool to change the tab title.
</system-reminder>

<system-reminder>
You can spawn task agents within this project using the mandelbot MCP spawn_tab tool (no arguments needed). The tool returns the new tab's ID. Note: spawn_tab creates a new visible tab (a full Claude Code session). The built-in Agent tool is different — it runs a lightweight subagent within your session. Use whichever fits the task.

When spawning a tab in response to a user request, use the focus_tab tool afterward to focus the new tab so the user lands there.
</system-reminder>

<system-reminder>
You can use the mandelbot MCP get_tab_tree tool to see all tabs (IDs, titles, ranks, statuses, and which is active). It also returns your own tab ID. Use the focus_tab tool to focus a tab by ID — this only works if you are the currently active tab (it no-ops otherwise to prevent stealing focus).
</system-reminder>

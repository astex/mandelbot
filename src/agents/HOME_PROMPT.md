<system-reminder>
This is the home tab. Do NOT use the mandelbot MCP set_title tool to change the tab title.
</system-reminder>

<system-reminder>
You can spawn agents using the mandelbot MCP spawn_tab tool. Note: spawn_tab creates a new visible tab (a full Claude Code session). The built-in Agent tool is different — it runs a lightweight subagent within your session. Use whichever fits the task.

- To create a project agent: pass a working_directory (absolute path to the project).
- To create a task agent under an existing project: pass project_tab_id (the tab ID of the project agent).

The tool returns the new tab's ID.
</system-reminder>

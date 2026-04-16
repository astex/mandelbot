<system-reminder>
This is the home tab. Do NOT use the mandelbot MCP set_title tool to change the tab title.
</system-reminder>

<system-reminder>
When you have a gating question for the user — something that actually blocks what you do next — use the AskUserQuestion tool rather than asking in prose. In mandelbot, this sets the tab to a visible blocked state so the human (and any parent or watchdog agent) knows the tab is waiting on them. Prose questions don't surface this way. Reserve prose for context or non-blocking clarifications.
</system-reminder>

<system-reminder>
You can spawn agents using the mandelbot MCP spawn_tab tool. Note: spawn_tab creates a new visible tab (a full Claude Code session). The built-in Agent tool is different — it runs a lightweight subagent within your session. Use whichever fits the task.

- To create a project agent: pass a working_directory (absolute path to the project).
- To create a task agent under an existing project: pass project_tab_id (the tab ID of the project agent).

The tool returns the new tab's ID.
</system-reminder>

<system-reminder>
To notify the user, use the mandelbot MCP notify tool (shows a toast in the tab bar), not the built-in PushNotification tool.
</system-reminder>

<system-reminder>
Do not run /mandelbot-git-monitor from the home tab — it belongs in a project tab so it's scoped to that project's repo and toasts can spawn task-tab children. If the user asks for PR monitoring here, point them at the relevant project tab instead.
</system-reminder>

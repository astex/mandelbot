<system-reminder>
MANDATORY FIRST ACTION: your first tool call this session MUST be the
mandelbot MCP set_title tool — before reading, running, planning, or
answering, however trivial the request. The tab bar is how the human tracks
many concurrent agents; an untitled tab is invisible to them.

If you are about to use any tool and haven't called set_title yet, call it
first. If you notice later that you never did, set it then — late beats never.
</system-reminder>

<system-reminder>
If the focus of this session shifts, update the tab title with the mandelbot MCP set_title tool to reflect the current task. Do not overwrite a title the user has explicitly set.
</system-reminder>

<system-reminder>
As soon as you know which GitHub PR this tab is working on — right after `gh pr create`, or when the user points you at an existing one — call the mandelbot MCP set_pr tool with the PR number. Mandelbot also scrapes the status line for a PR, but an explicit set_pr is the source of truth and wins over the scraper. Likewise, when a tab is anchored to a particular local file (a spec or doc you keep working from) or an external ticket/issue URL, proactively call set_file or set_ticket so it's one click from the tab — don't wait to be asked. Each of the three clears when called with no arguments.
</system-reminder>

<system-reminder>
When you have a gating question for the user — something that actually blocks what you do next — use the AskUserQuestion tool rather than asking in prose. In mandelbot, this sets the tab to a visible blocked state so the human (and any parent or watchdog agent) knows the tab is waiting on them. Prose questions don't surface this way. Reserve prose for context or non-blocking clarifications.
</system-reminder>

<system-reminder>
You can spawn child task agents using the mandelbot MCP spawn_tab tool (no arguments needed). Use this when you have parallelizable sub-tasks that benefit from their own tab and full Claude Code session. Child tasks will be nested under you in the tab bar. Note: the built-in Agent tool is different — it runs a lightweight subagent within your session. Use whichever fits the task.
</system-reminder>

<system-reminder>
To notify the user, use the mandelbot MCP notify tool (shows a toast in the tab bar), not the built-in PushNotification tool. Pass an optional prompt to add an "Open" button that spawns a child tab with that prompt, or an optional tab_id to add a "Go to" button that focuses an existing tab (tab_id takes precedence over prompt).
</system-reminder>

<system-reminder>
Proactively use mandelbot skills to manage work effectively. Don't wait for the user to ask — recognize the shape of the task and reach for the right workflow:

- **/mandelbot-spike-harden** — The user wants to build something new or substantially reshape an existing feature. Spike it fast, then harden. Good for greenfield features, new integrations, or any request where proving the approach matters before polishing.
- **/mandelbot-implement-iterate** — The user wants to extend, improve, or fill out an existing system — more options, more coverage, more polish. Iterative rounds of build-refactor-build with child agents.
- **/mandelbot-delegate** — Any work that can be split into independent pieces, or that risks filling your context window. Parallelize across child agents. Use this liberally — if the task has 2+ loosely-coupled parts, delegate.
</system-reminder>

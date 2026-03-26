<system-reminder>
CRITICAL: Before or alongside your first response, use the mandelbot MCP set_title tool to set a short tab title (a few words) describing the task.
</system-reminder>

<system-reminder>
If the focus of this session shifts, update the tab title with the mandelbot MCP set_title tool to reflect the current task. Do not overwrite a title the user has explicitly set.
</system-reminder>

<system-reminder>
Use the mandelbot MCP set_status tool to indicate your current state when appropriate:
- Call set_status with "needs_review" when presenting a plan or asking for feedback.
- Call set_status with "error" if you encounter an unrecoverable error.
Status transitions for tool calls (working/blocked/idle) are handled automatically — do not set those manually.
</system-reminder>

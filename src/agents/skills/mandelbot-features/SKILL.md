---
name: mandelbot-features
description: Show the user what mandelbot can do. Use this on first launch to introduce key features and workflows.
allowed-tools: []
---

# Welcome to Mandelbot

Give the user a brief, friendly overview of what mandelbot can do. Cover these key points conversationally — don't just dump a feature list:

## Agent tree

Mandelbot organizes work into a hierarchical tree of agents. The home agent (this tab) sits at the root. From here you can spawn **project agents** scoped to a working directory, and projects can spawn **task agents** for individual changes. Each agent has its own context, history, and working directory.

## Spawning agents

- Use the `spawn_tab` MCP tool to create new agents programmatically
- Use the keyboard shortcut `control+Space` (default: `ctrl+shift+Space`) to spawn an agent interactively
- Use `control+→` to spawn a child task under the current agent

## Navigating the tree

- `movement+↑` / `movement+↓` — move between sibling tabs
- `movement+←` / `movement+→` — move up/down the tree (parent / first child)
- `movement+Space` — jump to the next agent that needs attention
- `movement+-` — toggle back to the previously focused tab
- `movement+0-9` — jump to a tab by index

The default modifier prefixes are `ctrl+shift` (control) and `alt+shift` (movement). These can be changed in `~/.mandelbot/config.json`.

## Delegating work

When a task is large enough to parallelize, agents can use the `/mandelbot-delegate` skill to break work into subtasks and coordinate child agents via a shared status file.

## Configuration

Run `/mandelbot-config` to change theme, font, font size, shell, or keybinding prefixes. Settings live in `~/.mandelbot/config.json`.

## Shell tabs

Not everything needs an agent. Use `control+t` to open a plain shell tab for quick terminal work.

---

Keep the overview concise and welcoming. The user just installed mandelbot — help them feel oriented without overwhelming them.

---
name: mandelbot-keybindings
description: Use this skill when you need to know what keybindings are available in mandelbot, or the user asks about keyboard shortcuts. Read-only reference — to change prefix modifiers, use the mandelbot-config skill instead.
allowed-tools: [Read]
---

# Mandelbot Keybindings Reference

This skill is a read-only reference for the keybindings available in mandelbot. All bindings use configurable modifier prefixes set in `~/.mandelbot/config.json` (see the `mandelbot-config` skill to change them).

## Modifier prefixes

| Prefix | Config key | Default |
|--------|-----------|---------|
| **Control** | `control_prefix` | `ctrl+shift` |
| **Movement** | `movement_prefix` | `alt+shift` |

To know the user's actual prefixes, read `~/.mandelbot/config.json`. If the file doesn't exist or a key is absent, use the default.

## Control bindings (default: ctrl+shift)

| Key | Action | Description |
|-----|--------|-------------|
| `control + t` | New shell tab | Opens a plain shell tab |
| `control + Space` | Spawn agent | Creates a new agent tab (project-level or task under current project) |
| `control + ↓` | Spawn agent | Same as Space — alternate binding |
| `control + →` | Spawn child task | Creates a child task tab under the current agent |
| `control + w` | Close tab | Closes the focused tab |
| `control + c` | Copy | Copies the current terminal selection to clipboard |
| `control + v` | Paste | Pastes clipboard contents into the terminal |
| `control + Click` | Open link | Opens the URL under the cursor in the system browser |

## Movement bindings (default: alt+shift)

| Key | Action | Description |
|-----|--------|-------------|
| `movement + ↓` | Next sibling | Focus the next tab at the same tree level |
| `movement + ↑` | Previous sibling | Focus the previous tab at the same tree level |
| `movement + →` | First child | Navigate deeper into the tab tree |
| `movement + ←` | Parent | Navigate up to the parent tab |
| `movement + Space` | Next idle | Jump to the next idle agent (priority: blocked → needs review → idle task → idle project) |
| `movement + -` | Previous tab | Toggle back to the previously focused tab |
| `movement + 0-9` | Select by index | Focus the tab at position 0–9 in display order |
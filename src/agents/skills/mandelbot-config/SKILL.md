---
name: mandelbot-config
description: Use this skill when the user wants to change mandelbot settings like theme, font, font size, shell, or keybinding prefixes. Reads and writes ~/.mandelbot/config.json.
allowed-tools: [Read, Write]
---

# Update Mandelbot Config

Use this skill when the user asks to change mandelbot settings. Configuration lives at `~/.mandelbot/config.json`.

## Available settings

| Key | Type | Default | Values |
|-----|------|---------|--------|
| `theme` | string | `"dark"` | `"dark"`, `"light"` |
| `font` | string | `"monospace"` | Any font name installed on the system |
| `font_size` | number | `14.0` | Any positive number |
| `control_prefix` | string | `"ctrl+shift"` | Modifier combo (e.g. `"ctrl+shift"`, `"super"`) |
| `movement_prefix` | string | `"alt+shift"` | Modifier combo |
| `shell` | string | `$SHELL` | Path to a shell (e.g. `"/bin/zsh"`) |

Valid modifier names: `ctrl`, `shift`, `alt`, `super` (also `cmd`, `meta`, `logo`).

## Workflow

### 1. Read current config

Read `~/.mandelbot/config.json`. If the file doesn't exist, the current config is all defaults.

### 2. Apply changes

Merge the user's requested changes into the existing config (or a new object if the file didn't exist). Only include keys the user wants to change plus any that were already set — omitted keys use their defaults, so don't add keys unnecessarily.

### 3. Write the file

```bash
mkdir -p ~/.mandelbot
```

Write the updated JSON to `~/.mandelbot/config.json`.

### 4. Tell the user to restart

Config changes take effect on the next launch of mandelbot. Let the user know they need to restart for changes to apply.

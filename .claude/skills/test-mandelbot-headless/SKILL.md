---
name: test-mandelbot-headless
description: Drive mandelbot from a scripted JSON scenario and inspect app state without opening a window. Use this to exercise App::update state transitions (tab tree, focus, status, titles) from an agent that has no display.
---

# Test mandelbot headlessly

Mandelbot has a `--headless <scenario.json>` side-mode that instantiates the
real `App` state machine, runs scripted messages through `App::update`, and
prints JSON snapshots of the tab tree to stdout. No Iced window is opened.

This lets agents working on mandelbot itself exercise state transitions
without needing a display or a real PTY.

## When to use

- Verifying that a change to `App::update` still produces the expected tab
  tree (creation, selection, close, fold, status).
- Reproducing bugs that live in the state machine rather than the renderer or
  the PTY.
- Writing regression checks you can run from `cargo run -- --headless ...` in
  a CI-like loop.

## When NOT to use

- You need to test what's actually drawn on screen. Headless mode does not
  render.
- You need real shell output or a running Claude agent inside a tab. The
  spike drops the `Task<Message>` that `App::update` returns, so every
  subsystem that lives inside those tasks is silent:
  - PTY spawning / shell output
  - Clipboard I/O
  - MCP parent-socket traffic
  - Bell flashes
  - Window subscription (beyond the initial auto-injected resize)
- You want to test `iced::window::*` behavior.

If you need any of those, you need a real GUI test setup — headless mode
won't help.

## Running the demo

From the repo root:

```sh
cargo build
cargo run -- --headless examples/headless-demo.json
```

The demo sets the home tab title, spawns two shell tabs, snapshots, closes
one, and snapshots again. Expected output (trimmed):

```json
{
  "label": "after-two-shells",
  "active_tab_id": 2,
  "tabs": [
    { "id": 0, "title": "demo-home", "rank": "Home", "is_claude": true, ... },
    { "id": 1, "title": null, "rank": "Home", "is_claude": false, ... },
    { "id": 2, "title": null, "rank": "Home", "is_claude": false, ... }
  ]
}
{
  "label": "after-close",
  "active_tab_id": 1,
  "tabs": [
    { "id": 0, "title": "demo-home", ... },
    { "id": 1, "title": null, ... }
  ]
}
```

Snapshots are pretty-printed JSON objects, one per `Snapshot` action, emitted
to stdout in order. Exit code is 0 on success, 1 on error.

## Scenario format

A scenario is a JSON object with an `actions` array. Each action is either a
bare string (for no-arg variants) or an object with a single key.

Available actions (these map 1:1 to `ui::Message` variants; see
`src/headless.rs`):

| Action | Shape | Effect |
|---|---|---|
| `WindowResized` | `{ "WindowResized": { "width": 1600, "height": 900 } }` | Resize the window. One is auto-injected at startup with 1600×900 so the home tab boots. |
| `NewTab` | `"NewTab"` | Append a shell tab. |
| `SpawnAgent` | `"SpawnAgent"` | Same as the keybind — behavior depends on active tab's rank. |
| `SelectTab` | `{ "SelectTab": 2 }` | Focus tab by id. |
| `SelectTabByIndex` | `{ "SelectTabByIndex": 1 }` | Focus the Nth tab in display order. |
| `CloseTab` | `{ "CloseTab": 2 }` | Close tab by id. |
| `SetTitle` | `{ "SetTitle": { "tab_id": 0, "title": "home" } }` | Set a tab's title. |
| `SetStatus` | `{ "SetStatus": { "tab_id": 0, "status": "working" } }` | Set a tab's status. Valid: `idle`, `working`, `blocked`, `needs_review`, `error`. |
| `PendingChar` / `PendingSubmit` / `PendingCancel` | `{ "PendingChar": "a" }`, `"PendingSubmit"`, `"PendingCancel"` | Drive the pending-project-path input. |
| `Snapshot` | `{ "Snapshot": { "label": "after-two-shells" } }` | Dump app state to stdout. `label` is optional. |

### Snapshot fields

```json
{
  "label": "...",
  "active_tab_id": 0,
  "prev_active_tab_id": null,
  "folded": [],
  "tabs": [
    {
      "id": 0,
      "title": "home",
      "rank": "Home",
      "status": "Idle",
      "depth": 0,
      "parent_id": null,
      "is_claude": true,
      "is_pending": false
    }
  ]
}
```

Rank and status come through as their `Debug` string (spike shortcut).

## Spike-level limitations to be honest about

When reading the demo output you may notice the shell tabs have `rank: Home`.
That's because `NewTab` creates shell tabs at the home rank in the real code
path — not a headless artifact.

Things that WILL silently not happen because the returned `Task<Message>` is
dropped:

- No PTY is ever spawned. Tabs have an `event_tx` but the consumer never
  runs, so `write_input` calls just pile up in a channel.
- No MCP socket traffic. The parent-socket listener is bound during `boot`
  but its reader thread is never started (the task that would poll it is
  dropped).
- No clipboard operations.
- No resize subscription beyond the initial auto-injected resize.

If you write a scenario that depends on side effects from any of the above,
it will pass for the wrong reason. Keep scenarios to pure state transitions.

## Writing a new scenario

1. Decide what `App::update` transition you want to exercise.
2. Build a JSON file in `examples/` or a temp dir with the sequence of
   actions.
3. Run `cargo run -- --headless <path>`.
4. Pipe the output to `jq` if you want to assert on specific fields.

Example: check that closing the home tab doesn't panic the app.

```json
{ "actions": [ { "CloseTab": 0 }, { "Snapshot": {} } ] }
```

(Spike note: closing the last tab triggers `iced::exit()` which does nothing
in headless mode, so the snapshot will just show an empty tab list.)

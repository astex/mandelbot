---
name: test-mandelbot-headless
description: Drive mandelbot from a scripted JSON scenario and inspect app state without opening a window. Use this to exercise App::update state transitions (tab tree, focus, status, titles) from an agent that has no display.
---

# Test mandelbot headlessly

Mandelbot has a `--headless <scenario.json>` side-mode that instantiates the
real `App` state machine, runs scripted messages through `App::update`, and
prints newline-delimited JSON snapshots of the tab tree to stdout. No Iced
window is opened, and no parent-socket listener is bound.

This lets agents working on mandelbot itself exercise state transitions
without needing a display or a real PTY.

## When to use

- Verifying that a change to `App::update` still produces the expected tab
  tree (creation, selection, close, fold, status, titles).
- Reproducing bugs that live in the state machine rather than the renderer or
  the PTY.
- Writing regression tests. `tests/headless.rs` is an example — it runs the
  binary against `examples/headless-demo.json` and asserts on the resulting
  snapshot JSON.

## When NOT to use

Every `iced::Task<Message>` returned by `App::update` is **dropped**. Anything
that lives inside those tasks is silent:

- PTY spawning / shell output
- Clipboard I/O
- MCP parent-socket traffic (the listener is never even bound)
- Bell flashes
- Window subscription (beyond the initial auto-injected resize)

Do not use headless mode to test any of those. If you write a scenario that
relies on side effects from a `Task`, it will pass for the wrong reason.

## Running the demo

```sh
cargo build
cargo run -- --headless examples/headless-demo.json
```

The demo sets the home tab title, spawns two shell tabs, snapshots, closes
one, and snapshots again. Output is one JSON object per line (NDJSON):

```
{"label":"after-two-shells","active_tab_id":2,"tabs":[...3 tabs...],...}
{"label":"after-close","active_tab_id":1,"tabs":[...2 tabs...],...}
```

Pipe through `jq` for readability:

```sh
cargo run --quiet -- --headless examples/headless-demo.json | jq .
```

## Running the integration test

```sh
cargo test --test headless
```

This runs `tests/headless.rs` which spawns the binary itself, parses stdout,
and asserts on the expected snapshot fields. Use it as a template for new
behavior tests.

## Scenario format

A scenario is a JSON object with an `actions` array. Each action is either a
bare string (for no-arg variants) or an object with a single key (serde's
external enum tagging).

Actions (these map 1:1 to `ui::Message` variants; see `src/headless.rs`):

| Action | Shape | Effect |
|---|---|---|
| `WindowResized` | `{"WindowResized": {"width": 1600, "height": 900}}` | Resize the window. One is auto-injected at startup (1600×900) so the home tab boots. |
| `NewTab` | `"NewTab"` | Append a shell tab. |
| `SpawnAgent` | `"SpawnAgent"` | Same as the keybind — behavior depends on the active tab's rank. |
| `SelectTab` | `{"SelectTab": 2}` | Focus tab by id. |
| `SelectTabByIndex` | `{"SelectTabByIndex": 1}` | Focus the Nth tab in display order. |
| `CloseTab` | `{"CloseTab": 2}` | Close tab by id. |
| `SetTitle` | `{"SetTitle": {"tab_id": 0, "title": "home"}}` | Set a tab's title. |
| `SetStatus` | `{"SetStatus": {"tab_id": 0, "status": "working"}}` | Set a tab's status. Valid values: `idle`, `working`, `blocked`, `needs_review`, `error`. Unknown values produce a non-zero exit with an error naming the offending action index. |
| `PendingChar` / `PendingSubmit` / `PendingCancel` | `{"PendingChar": "a"}`, `"PendingSubmit"`, `"PendingCancel"` | Drive the pending-project-path input. |
| `Snapshot` | `{"Snapshot": {"label": "after-two-shells"}}` | Dump app state to stdout. `label` is optional. |

## Snapshot schema

Each `Snapshot` action emits one line of JSON matching the `HeadlessSnapshot`
struct in `src/ui.rs`:

```jsonc
{
  "label": "after-two-shells",       // string | null
  "active_tab_id": 2,                 // usize
  "prev_active_tab_id": 1,            // usize | null
  "tabs": [                           // array of HeadlessTab
    {
      "id": 0,                        // usize
      "title": "demo-home",           // string | null
      "rank": "Home",                 // "Home" | "Project" | "Task"
      "status": "Idle",               // "Idle" | "Working" | "Blocked" | "NeedsReview" | "Error"
      "depth": 0,                     // usize
      "parent_id": null,              // usize | null
      "is_claude": true,              // bool
      "is_pending": false             // bool
    }
  ],
  "folded": []                        // sorted array of tab ids
}
```

Ranks and statuses are proper Serialize-derived strings — they're safe to
match on in assertions.

## Writing a new scenario

1. Decide which `App::update` transition you want to exercise.
2. Write a JSON file in `examples/` (or a temp dir) with the action sequence.
3. Run `cargo run -- --headless <path>` and eyeball the output, or add a
   `#[test]` in `tests/headless.rs` that spawns the binary via
   `env!("CARGO_BIN_EXE_mandelbot")` and asserts on the parsed snapshots.

Example: check that closing the home tab exits cleanly (headless `iced::exit()`
is a no-op, so the follow-up snapshot just shows an empty tab list).

```json
{ "actions": [ { "CloseTab": 0 }, { "Snapshot": {} } ] }
```

## Gotchas

- `NewTab` creates a shell tab at the same rank as the active tab (from home,
  that's `Home`). That's a real-code behavior, not a headless artifact.
- The auto-injected `WindowResized` at boot drives the "first resize spawns
  home tab" branch in `App::update`. If your scenario starts with an explicit
  `WindowResized`, it'll be the *second* resize the app sees, not the first.
- The runtime directory at `runtime_dir()` (e.g. `/run/user/$UID/mandelbot-$PID/`)
  is still created so tab FIFOs have somewhere to land, but it's cleaned up
  on `App::drop`.

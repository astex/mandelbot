---
name: test-mandelbot-headless
description: Drive mandelbot from a scripted JSON scenario and inspect app state without opening a window. Use this to exercise App::update state transitions (tab tree, focus, status, titles) and real PTY behavior (shell output, grid content) from an agent that has no display.
---

# Test mandelbot headlessly

Mandelbot has a `--headless <scenario.json>` side-mode that instantiates the
real `App` state machine, runs scripted messages through `App::update`, and
prints newline-delimited JSON snapshots of the tab tree to stdout. No Iced
window is opened, and no parent-socket listener is bound.

Effects returned by `App::update` are fully executed: spawned tabs get real
PTYs, real shell output flows into the alacritty terminal grid, and clipboard
operations are captured in-memory. This lets agents exercise both state
transitions and real shell behavior without a display.

## When to use

- Verifying that a change to `App::update` still produces the expected tab
  tree (creation, selection, close, fold, status, titles).
- Testing real shell behavior: spawn a tab, inject keystrokes, read back grid
  content.
- Reproducing bugs that live in the state machine or the PTY pipeline.
- Writing regression tests. `tests/headless.rs` runs the binary against
  scenario files; `src/headless.rs` has a `#[cfg(test)]` module with API-level
  tests using `HeadlessHost` directly.

## Limitations

- No parent-socket listener is bound, so MCP-related messages
  (`McpSpawnAgent`, `RespondToTab`) are handled but the responses go nowhere.
- Bell effects (`TriggerBell`) are no-ops — no animation in headless mode.
- `Effect::Exit` is a no-op — the headless host doesn't terminate the process.
- No rendering pipeline — `view()` is never called.

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

## Running the tests

```sh
cargo test --test headless       # integration tests (binary subprocess)
cargo test headless::tests       # unit test with real PTY
cargo test                       # all tests
```

The `headless::tests::real_pty_grid_content` test spawns a real shell tab,
types `echo mandelbot-headless-marker`, waits for the output to appear in the
terminal grid, and asserts on it.

## HeadlessHost API (for Rust tests)

For tests that need to interact with real PTYs and inspect grid content, use
the `HeadlessHost` API directly instead of going through the binary:

```rust
use crate::headless::{HeadlessHost, drain_until};
use crate::ui::Message;
use iced::Size;
use std::time::Duration;

let mut host = HeadlessHost::new();

// Boot: inject window resize to spawn home tab.
host.step(Message::WindowResized(Size { width: 1600.0, height: 900.0 }));

// Spawn a shell tab.
host.step(Message::NewTab);

// Wait for the shell to boot and produce output.
drain_until(&mut host, Duration::from_secs(5), |h| {
    h.grid_text(1).map(|t| !t.is_empty()).unwrap_or(false)
});

// Type a command.
host.step(Message::PtyInput(b"echo hello\r".to_vec()));

// Wait for the output to appear in the grid.
drain_until(&mut host, Duration::from_secs(5), |h| {
    h.grid_text(1).map(|t| t.contains("hello")).unwrap_or(false)
});

// Read grid content.
let grid = host.grid_text(1).unwrap();
assert!(grid.contains("hello"));

// Clipboard captures are also available.
assert!(host.clipboard.is_none()); // no OSC 52 writes yet
```

Key methods:
- `HeadlessHost::new()` — create a headless session
- `host.step(msg)` — send a message through `App::update` and execute effects
- `host.drain_pending(timeout)` — process pending messages from tab threads
- `drain_until(&mut host, timeout, predicate)` — drain until predicate is true
- `host.snapshot(label)` — get a `HeadlessSnapshot` of app state
- `host.grid_text(tab_id)` — read terminal grid content as a string
- `host.clipboard` / `host.primary_clipboard` — captured clipboard writes

## Scenario format

A scenario is a JSON object with an `actions` array. Each action is either a
bare string (for no-arg variants) or an object with a single key (serde's
external enum tagging).

Actions (these map 1:1 to `ui::Message` variants; see `src/headless.rs`):

| Action | Shape | Effect |
|---|---|---|
| `WindowResized` | `{"WindowResized": {"width": 1600, "height": 900}}` | Resize the window. One is auto-injected at startup (1600x900) so the home tab boots. |
| `NewTab` | `"NewTab"` | Append a shell tab. Real PTY is spawned. |
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
4. For tests that need real PTY interaction or grid inspection, use the
   `HeadlessHost` API directly (see above).

Example: check that closing the home tab exits cleanly.

```json
{ "actions": [ { "CloseTab": 0 }, { "Snapshot": {} } ] }
```

Example: spawn a shell, run a command, and verify grid output (as a Rust test):

```rust
#[test]
fn shell_echo_test() {
    let mut host = HeadlessHost::new();
    host.step(Message::WindowResized(Size { width: 1600.0, height: 900.0 }));
    host.step(Message::NewTab);
    drain_until(&mut host, Duration::from_secs(3), |h| {
        h.grid_text(1).map(|t| !t.is_empty()).unwrap_or(false)
    });
    host.step(Message::PtyInput(b"echo test-marker\r".to_vec()));
    let found = drain_until(&mut host, Duration::from_secs(3), |h| {
        h.grid_text(1).map(|t| t.contains("test-marker")).unwrap_or(false)
    });
    assert!(found);
}
```

## Gotchas

- `NewTab` creates a shell tab at the same rank as the active tab (from home,
  that's `Home`). That's a real-code behavior, not a headless artifact.
- The auto-injected `WindowResized` at boot drives the "first resize spawns
  home tab" branch in `App::update`. If your scenario starts with an explicit
  `WindowResized`, it'll be the *second* resize the app sees, not the first.
- The runtime directory at `runtime_dir()` (e.g. `/run/user/$UID/mandelbot-$PID/`)
  is still created so tab FIFOs have somewhere to land, and is cleaned up on
  `HeadlessHost::drop`.
- Shell tabs spawn real processes. Tests that run commands should use
  deterministic, fast commands (e.g. `echo marker`) and appropriate timeouts.

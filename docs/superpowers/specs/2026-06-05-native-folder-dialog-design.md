# Native Folder Dialog for New Project Tabs — Design

**Date:** 2026-06-05 (revised 2026-06-12: main-thread dialog spawn, tab
lifecycle guards, re-entrancy, click-order/hover, testing-harness reality,
dependency specifics)
**Status:** Approved, ready for implementation planning

## Summary

Add a native OS folder-selection dialog as an additional way to choose a
project directory when opening a new project. The existing inline
`Project directory:` text prompt (triggered by Ctrl+T) is kept unchanged; a
new clickable `[ Browse… ]` hint in that pending-tab UI pops the native folder
picker. Choosing a folder spawns the project tab immediately.

## Decisions (from brainstorming)

- **Relationship to existing flow:** Augment, not replace. The inline text
  prompt and typing + Enter remain fully functional. The dialog is an added
  option.
- **Pick type:** Folder picker (a project is a directory; the chosen folder
  becomes `project_dir`).
- **Window model:** Unchanged. Single-window / tabs model. "New project window"
  means the new project *tab* that gets spawned. No OS-level multi-window work.
- **Default directory:** The user's home directory (`$HOME`).
- **Invocation:** A clickable `[ Browse… ]` button/hint rendered in the pending
  tab UI (no new keybinding).
- **After selection:** Spawn the project immediately (same behavior as pressing
  Enter on a typed path: canonicalize, dedup against already-open projects,
  spawn, focus).

## Chosen Approach

Use the [`rfd`](https://crates.io/crates/rfd) crate (the de-facto cross-platform
native dialog library for Rust/winit GUI apps) via `rfd::AsyncFileDialog`,
driven through Iced's `Task::perform`. The async API is the correct fit for a
GUI event loop and avoids the macOS main-thread pitfalls of the synchronous API.

**Main-thread spawn pattern (required).** rfd's docs recommend *spawning*
dialogs on the main thread and *awaiting* them elsewhere; off-main-thread
spawning in windowed apps is possible but adds overhead, and rfd-on-winit-macOS
has a history of event-loop freezes (winit #1779, #2752, #3179). Iced polls
`Task::perform` futures on a background executor — so the dialog must NOT be
constructed inside the async block. Instead, build the future synchronously in
the `update()` handler (which runs on the main thread) and only await it in the
task:

```rust
// inside the OpenProjectDialog handler — main thread
let dialog = rfd::AsyncFileDialog::new()
    .set_title("Open Project")
    .set_directory(home)
    .pick_folder();           // dialog spawned here, on the main thread
Task::perform(dialog, move |handle| Message::ProjectDialogResult {
    tab_id,
    path: handle.map(|h| h.path().to_path_buf()),
})
```

### Approaches rejected

- **Sync `rfd::FileDialog` on a background thread** (via the existing
  `spawn_blocking_task`): simpler wiring, but on macOS native dialogs must run
  on the main thread; calling the sync API off-thread risks crashes/hangs.
- **Custom in-app folder browser widget:** no new dependency, but it is a large
  amount of UI work and is not a *native* dialog, which is the requirement.

## Architecture

### Current flow (for reference)

- Ctrl+T → `Message::NewTab` → `spawn_pending_project_tab()` creates a
  `TerminalTab` with `pending_input: Some(String::new())`
  (`src/ui/handlers/tabs.rs`).
- The pending tab renders as a text prompt `Project directory: {input}_` inside
  the custom `Terminal` widget's `draw()` (`src/widget/terminal.rs:293-312`).
- Keystrokes publish `Message::PendingInput(PendingKey::{Char,Backspace,Submit,Cancel})`.
- `handle_pending_input` (`src/ui/handlers/tabs.rs:357-413`) handles the keys;
  `Submit` expands `~`, canonicalizes, dedups via `find_project_for_dir`,
  removes the pending tab, and calls `spawn_tab(..., AgentRank::Project, ...)`,
  then focuses the new tab.

### Changes

**1. Dependency.** Add `rfd` (currently 0.17.x) to `Cargo.toml`. Feature flags
are a no-op on macOS; if/when Linux builds matter, the backend choice (`gtk3`
vs the default `xdg-portal`) determines system dependencies and async-runtime
wiring and should be chosen deliberately at that point.

For the default directory, use `std::env::var("HOME")` for consistency with the
existing `~` expansion in the `Submit` arm (`src/ui/handlers/tabs.rs:379`)
rather than introducing a second mechanism.

**2. Extract shared open-project helper.** Pull the canonicalize → dedup →
remove-pending → `spawn_tab` → focus sequence out of the `Submit` arm into a
reusable method:

```
fn open_project_from_path(&mut self, tab_id: usize, path: PathBuf) -> Task<Message>
```

- **Guards that `tab_id` still exists** (`self.tabs.get(tab_id)`), returning
  `Task::none()` if not. The dialog is non-modal with respect to the app: while
  it is open the user can press Esc (closing the pending tab) or type a path
  and hit Enter (removing the pending tab and spawning the project), so the
  result can arrive with a dead `tab_id`. Dropping the result is correct — the
  pending tab's `parent_id` is unrecoverable once it's gone, and the user
  already abandoned the flow. (Tab IDs are monotonically increasing and never
  reused, so a stale ID cannot alias onto a newer tab.)
- Canonicalizes `path` (`std::fs::canonicalize`, falling back to the raw path).
- If `find_project_for_dir` returns an existing project tab, focus it and close
  the pending `tab_id`.
- Otherwise capture the pending tab's `parent_id`, remove the pending tab, call
  `spawn_tab(true, AgentRank::Project, Some(canonical), parent_id, ...)`, focus
  the new tab, and return its `Task`.

To keep the testing story honest (see Testing below), factor the pure parts —
path expansion/canonicalization and the dedup decision — into free functions or
`TabStore`-level methods; the `App` method stays a thin orchestrator.

The `Submit` arm becomes: expand `~`, then call `open_project_from_path`. The
dialog result calls `open_project_from_path` directly (the dialog already
returns an absolute path, so no `~` expansion needed).

**3. `[ Browse… ]` hint rendering.** In the pending-tab branch of
`Terminal::draw` (`src/widget/terminal.rs:293`), render a second line below the
`Project directory:` prompt — `[ Browse… ]` — in an accent color. A small helper
computes the hint's `Rectangle` so `draw` and `update` agree on its location.
Make it pure geometry over the inputs it needs (bounds plus char metrics)
rather than a method requiring a fully-built widget, so it is unit-testable:

```
fn browse_hint_rect(bounds: &Rectangle, char_width: f32, char_height: f32) -> Rectangle
```

**4. Click handling.** In `Terminal::update`'s
`Event::Mouse(ButtonPressed(Left))` arm (`src/widget/terminal.rs:531`), when the
tab is pending (`self.tab.pending_input.is_some()`) and the cursor falls inside
`browse_hint_rect`, publish `Message::OpenProjectDialog(self.tab.id)` and
`shell.capture_event()`.

The pending check must come **first** in that arm — before the existing
link-click, scrollbar, and text-selection branches. A pending tab has a real
(empty) terminal grid, so today clicks on it fall through into the selection
machinery. Swallow all mouse-press handling for pending tabs (clicks outside
the hint rect do nothing), rather than only special-casing the hint.

**4a. Hover affordance.** `Terminal::mouse_interaction`
(`src/widget/terminal.rs:500`) already returns `mouse::Interaction::Pointer`
when hovering a link; do the same when the tab is pending and the cursor is
inside `browse_hint_rect`. It receives the cursor position, so it can reuse the
rect helper directly — no extra state needed.

**4b. Re-entrancy guard.** Nothing else stops a second click from spawning a
second dialog. Track an open dialog (e.g. a `dialog_open` flag on the pending
tab's meta or on the app), set in the `OpenProjectDialog` handler and cleared
in `ProjectDialogResult`; ignore `OpenProjectDialog` while set. This also gives
the renderer a way to draw the hint as disabled while the picker is up.

**5. New messages.**

```
OpenProjectDialog(usize),                                  // tab_id of the pending tab
ProjectDialogResult { tab_id: usize, path: Option<PathBuf> },
```

- `OpenProjectDialog(tab_id)` handler: if the dialog-open guard is set or the
  tab is gone, return `Task::none()`. Otherwise set the guard and return the
  main-thread-spawn `Task::perform` shown under **Chosen Approach** above (the
  dialog future is built synchronously in the handler, NOT inside an async
  block).

- `ProjectDialogResult { tab_id, path }` handler: clear the dialog-open guard,
  then:
  - `Some(path)` → `self.open_project_from_path(tab_id, path)` (which itself
    no-ops if the pending tab has since been closed).
  - `None` (cancelled) → `Task::none()`; the pending tab stays so the user can
    still type a path.

### Data flow

```
Ctrl+T ──► pending project tab (inline prompt + [ Browse… ] hint)
   │
   ├─ type path + Enter ──► PendingInput(Submit) ──► open_project_from_path ──► project tab
   │
   └─ click [ Browse… ] ──► OpenProjectDialog(tab_id)
                                 └─► rfd::AsyncFileDialog.pick_folder()
                                        └─► ProjectDialogResult { tab_id, path }
                                              ├─ Some ──► open_project_from_path ──► project tab
                                              └─ None  ──► no-op (stay in pending tab)
```

### Error handling

- Dialog cancelled / no selection → `path = None` → clear guard, no-op.
- Pending tab closed (Esc) or submitted via typed path while the dialog was
  open → result arrives with a dead `tab_id` → dropped by the guard in
  `open_project_from_path`.
- Second click on the hint while a dialog is open → ignored via the
  re-entrancy guard.
- Non-canonicalizable path → fall back to the raw `PathBuf` (existing behavior).
- Already-open project → focus the existing tab, close the pending one (existing
  behavior, preserved by the shared helper).
- Unparented dialog: without `set_parent` the picker is a free-floating panel
  and can land behind the mandelbot window on macOS. Accepted for now; Iced
  0.14's `window::run_with_handle` can supply a parent handle to rfd's
  `set_parent` as follow-up polish.

## Testing

Native dialogs cannot be driven in unit tests, so the dialog invocation is kept
as a thin shim.

**Caveat:** there is currently no test harness for `App`-level handlers — only
`src/tabs.rs` (the `TabStore`) has test modules; `src/ui/handlers/tabs.rs` and
`src/widget/terminal.rs` have none, and `App` drags in window size, config, and
channels. Rather than building an `App` test harness, factor the testable logic
down (pure functions / `TabStore` methods, per the helper design above) and
test there.

Tested units:

- Path expansion + canonicalization and the already-open dedup decision, as
  pure/`TabStore`-level functions extracted from `open_project_from_path`. The
  `Submit` path exercising the shared helper should retain its existing
  behavior (verified manually if no harness exists at that level).
- Stale-tab guard: result for a removed `tab_id` is a no-op.
- `browse_hint_rect` geometry and the click hit-test (point inside vs. outside
  the rect) — testable because the rect helper is a pure function of bounds and
  char metrics.

## Risk to validate early

`rfd::AsyncFileDialog` main-thread behavior under Iced 0.14's winit backend on
macOS — before building out the surrounding wiring, write a ~20-line spike
using the main-thread spawn pattern from **Chosen Approach** and confirm the
picker opens, returns a path, and returns `None` on cancel.

## Out of scope

- OS-level multi-window support.
- Replacing or removing the inline text-input flow.
- Remembering the last-used directory (default is always `$HOME`).
- File (non-folder) selection.
- A keybinding for the dialog (invocation is the clickable hint only).
- Parenting the dialog to the main window via `set_parent` (see Error
  handling; follow-up polish).
- Drag-and-drop of a folder onto the pending tab. Today `FileDropped` pastes
  the shell-escaped path into the PTY (`src/widget/terminal.rs:821`), which is
  nonsensical for a pending tab. Once `open_project_from_path` exists,
  drop-folder-to-open-project becomes a near-free second consumer of the
  helper — noted here as an enabled follow-up.

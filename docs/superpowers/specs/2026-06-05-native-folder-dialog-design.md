# Native Folder Dialog for New Project Tabs — Design

**Date:** 2026-06-05
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

**1. Dependency.** Add `rfd` to `Cargo.toml`.

**2. Extract shared open-project helper.** Pull the canonicalize → dedup →
remove-pending → `spawn_tab` → focus sequence out of the `Submit` arm into a
reusable method:

```
fn open_project_from_path(&mut self, tab_id: usize, path: PathBuf) -> Task<Message>
```

- Canonicalizes `path` (`std::fs::canonicalize`, falling back to the raw path).
- If `find_project_for_dir` returns an existing project tab, focus it and close
  the pending `tab_id`.
- Otherwise capture the pending tab's `parent_id`, remove the pending tab, call
  `spawn_tab(true, AgentRank::Project, Some(canonical), parent_id, ...)`, focus
  the new tab, and return its `Task`.

The `Submit` arm becomes: expand `~`, then call `open_project_from_path`. The
dialog result calls `open_project_from_path` directly (the dialog already
returns an absolute path, so no `~` expansion needed).

**3. `[ Browse… ]` hint rendering.** In the pending-tab branch of
`Terminal::draw` (`src/widget/terminal.rs:293`), render a second line below the
`Project directory:` prompt — `[ Browse… ]` — in an accent color. A small helper
computes the hint's `Rectangle` from the widget `bounds` so `draw` and `update`
agree on its location:

```
fn browse_hint_rect(&self, bounds: &Rectangle) -> Rectangle
```

**4. Click handling.** In `Terminal::update`'s
`Event::Mouse(ButtonPressed(Left))` arm (`src/widget/terminal.rs:531`), when the
tab is pending (`self.tab.pending_input.is_some()`) and the cursor falls inside
`browse_hint_rect`, publish `Message::OpenProjectDialog(self.tab.id)` and
`shell.capture_event()`.

**5. New messages.**

```
OpenProjectDialog(usize),                                  // tab_id of the pending tab
ProjectDialogResult { tab_id: usize, path: Option<PathBuf> },
```

- `OpenProjectDialog(tab_id)` handler returns:

  ```
  Task::perform(
      async move {
          rfd::AsyncFileDialog::new()
              .set_title("Open Project")
              .set_directory(home_dir())   // $HOME
              .pick_folder()
              .await
              .map(|handle| handle.path().to_path_buf())
      },
      move |path| Message::ProjectDialogResult { tab_id, path },
  )
  ```

- `ProjectDialogResult { tab_id, path }` handler:
  - `Some(path)` → `self.open_project_from_path(tab_id, path)`.
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

- Dialog cancelled / no selection → `path = None` → no-op.
- Non-canonicalizable path → fall back to the raw `PathBuf` (existing behavior).
- Already-open project → focus the existing tab, close the pending one (existing
  behavior, preserved by the shared helper).

## Testing

Native dialogs cannot be driven in unit tests, so the dialog invocation is kept
as a thin shim. Tested units:

- `open_project_from_path`: canonicalization, already-open dedup (focus existing
  + close pending), and spawn/focus of a new project tab. The `Submit` path
  exercising this helper should retain its existing behavior.
- `browse_hint_rect` geometry and the click hit-test (point inside vs. outside
  the rect).

## Risk to validate early

`rfd::AsyncFileDialog` main-thread behavior under Iced 0.14's winit backend on
macOS — confirm the picker opens and returns a path/None correctly before
building out the surrounding wiring.

## Out of scope

- OS-level multi-window support.
- Replacing or removing the inline text-input flow.
- Remembering the last-used directory (default is always `$HOME`).
- File (non-folder) selection.
- A keybinding for the dialog (invocation is the clickable hint only).

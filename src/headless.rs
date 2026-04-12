//! Headless driver for mandelbot.
//!
//! Reads a JSON scenario file, constructs an `App` via `HeadlessHost::new`
//! (no Iced runtime, no parent socket bound), injects a `WindowResized` to
//! boot the home tab, then applies scenario actions and dumps typed snapshots
//! to stdout as newline-delimited JSON.
//!
//! Unlike the original headless driver, effects returned by `App::update` are
//! actually executed: `StartTab` effects spawn real PTYs via `run_tab_thread`,
//! so shell output flows into the alacritty grid and can be inspected.

use std::collections::VecDeque;
use std::error::Error;
use std::fmt;
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use iced::Size;
use serde::Deserialize;

use crate::config::Config;
use crate::effect::Effect;
use crate::tab::{AgentStatus, create_fifo, run_fifo_thread, run_tab_thread};
use crate::ui::{App, HeadlessSnapshot, Message, PendingKey};

const DEFAULT_WINDOW_WIDTH: f32 = 1600.0;
const DEFAULT_WINDOW_HEIGHT: f32 = 900.0;

#[derive(Debug, Deserialize)]
pub struct Scenario {
    pub actions: Vec<Action>,
}

/// Scenario action. Serde's external tagging makes unit variants serialize as
/// a bare string (`"NewTab"`) and struct/tuple variants as a single-key object
/// (`{"SetTitle": {...}}`). See SKILL.md for the worked examples.
#[derive(Debug, Deserialize)]
pub enum Action {
    WindowResized { width: f32, height: f32 },
    NewTab,
    SpawnAgent,
    SelectTab(usize),
    SelectTabByIndex(usize),
    CloseTab(usize),
    SetTitle { tab_id: usize, title: String },
    SetStatus { tab_id: usize, status: String },
    PendingChar(char),
    PendingSubmit,
    PendingCancel,
    Snapshot { label: Option<String> },
}

#[derive(Debug)]
pub enum HeadlessError {
    ReadScenario(std::io::Error),
    ParseScenario(serde_json::Error),
    UnknownStatus { action_index: usize, status: String },
    WriteSnapshot(std::io::Error),
}

impl fmt::Display for HeadlessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadScenario(e) => write!(f, "reading scenario file: {e}"),
            Self::ParseScenario(e) => write!(f, "parsing scenario JSON: {e}"),
            Self::UnknownStatus {
                action_index,
                status,
            } => write!(
                f,
                "action {action_index}: unknown SetStatus value {status:?} (expected one of: idle, working, blocked, needs_review, error)"
            ),
            Self::WriteSnapshot(e) => write!(f, "writing snapshot to stdout: {e}"),
        }
    }
}

impl Error for HeadlessError {}

/// Headless host that drives `App::update` and executes effects on real
/// threads. Spawned tabs get real PTYs, real shell output flows into the
/// alacritty grid, and scenarios can inject input and read back grid contents.
pub struct HeadlessHost {
    app: App,
    /// Messages from tab threads waiting to be processed.
    #[allow(dead_code)]
    pending_rx: mpsc::Receiver<Message>,
    pending_tx: mpsc::Sender<Message>,
    /// Keep thread join handles alive so tab I/O threads run.
    _tab_threads: Vec<std::thread::JoinHandle<()>>,
    /// Clipboard captures for assertions.
    pub clipboard: Option<String>,
    pub primary_clipboard: Option<String>,
    /// Runtime directory for this headless session.
    runtime_dir: std::path::PathBuf,
}

impl HeadlessHost {
    pub fn new() -> Self {
        let runtime_dir = crate::tab::runtime_dir();
        std::fs::create_dir_all(&runtime_dir)
            .expect("failed to create runtime dir");
        let parent_socket_path = runtime_dir.join("parent.sock");

        let config = Config::load();
        let app = App::new(config, parent_socket_path);

        let (pending_tx, pending_rx) = mpsc::channel();

        Self {
            app,
            pending_rx,
            pending_tx,
            _tab_threads: Vec::new(),
            clipboard: None,
            primary_clipboard: None,
            runtime_dir,
        }
    }

    /// Send a message through App::update and execute all resulting effects.
    pub fn step(&mut self, msg: Message) {
        let effects = self.app.update(msg);
        for effect in effects {
            self.run_effect(effect);
        }
    }

    /// Process all pending messages from tab threads, feeding each back
    /// through `step`. Repeats until no more messages arrive within the
    /// given timeout.
    #[allow(dead_code)]
    pub fn drain_pending(&mut self, timeout: Duration) {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let mut batch: VecDeque<Message> = VecDeque::new();
            // Collect all immediately available messages.
            while let Ok(msg) = self.pending_rx.try_recv() {
                batch.push_back(msg);
            }
            if batch.is_empty() {
                // No messages ready — wait a bit for tab threads to produce.
                if std::time::Instant::now() >= deadline {
                    break;
                }
                match self.pending_rx.recv_timeout(Duration::from_millis(50)) {
                    Ok(msg) => batch.push_back(msg),
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        if std::time::Instant::now() >= deadline {
                            break;
                        }
                        continue;
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
            // Process all collected messages.
            for msg in batch {
                self.step(msg);
            }
        }
    }

    /// Get a snapshot of the current app state.
    pub fn snapshot(&self, label: Option<String>) -> HeadlessSnapshot {
        self.app.headless_snapshot(label)
    }

    /// Read the visible grid content for a tab as a string (lines joined by
    /// newline, trailing whitespace trimmed).
    #[allow(dead_code)]
    pub fn grid_text(&self, tab_id: usize) -> Option<String> {
        use alacritty_terminal::grid::Dimensions;
        use alacritty_terminal::index::{Column, Line};
        use alacritty_terminal::term::cell::Cell;

        let tab = self.app.tabs().iter().find(|t| t.id == tab_id)?;
        let term = tab.lock_term();
        let lines = term.screen_lines();
        let cols = term.columns();

        let mut result = String::new();
        for line_idx in 0..lines {
            let mut line_str = String::new();
            for col_idx in 0..cols {
                let point = alacritty_terminal::index::Point {
                    line: Line(line_idx as i32),
                    column: Column(col_idx),
                };
                let cell: &Cell = &term.grid()[point];
                line_str.push(cell.c);
            }
            let trimmed = line_str.trim_end();
            result.push_str(trimmed);
            result.push('\n');
        }

        // Trim trailing empty lines.
        let result = result.trim_end_matches('\n').to_string();
        Some(result)
    }

    fn run_effect(&mut self, effect: Effect) {
        match effect {
            Effect::StartTab {
                tab_id: _,
                params,
                event_rx,
                pty_event_tx,
                term,
                listener,
                fifo_path,
            } => {
                // Create FIFO for status updates.
                create_fifo(&fifo_path);

                // Spawn fifo reader thread.
                let fifo_tx = self.pending_tx.clone();
                let fifo_path_clone = fifo_path.clone();
                let fifo_id = fifo_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(0);
                let fifo_handle = std::thread::spawn(move || {
                    run_fifo_thread(fifo_id, fifo_path_clone, move |msg| {
                        let _ = fifo_tx.send(msg);
                    });
                });
                self._tab_threads.push(fifo_handle);

                // Spawn tab I/O thread.
                let tab_tx = self.pending_tx.clone();
                let tab_handle = std::thread::spawn(move || {
                    run_tab_thread(params, event_rx, pty_event_tx, term, listener, move |msg| {
                        let _ = tab_tx.send(msg);
                    });
                });
                self._tab_threads.push(tab_handle);
            }
            Effect::WriteClipboard(text) => {
                self.clipboard = Some(text);
            }
            Effect::WritePrimaryClipboard(text) => {
                self.primary_clipboard = Some(text);
            }
            Effect::ReadClipboard { tab_id, formatter } => {
                if let Some(text) = &self.clipboard {
                    let response = (formatter)(text);
                    let _ = self.pending_tx.send(
                        Message::ClipboardLoadResult(tab_id, Some(response)),
                    );
                } else {
                    let _ = self.pending_tx.send(
                        Message::ClipboardLoadResult(tab_id, None),
                    );
                }
            }
            Effect::ReadPrimaryClipboard { tab_id, formatter } => {
                if let Some(text) = &self.primary_clipboard {
                    let response = (formatter)(text);
                    let _ = self.pending_tx.send(
                        Message::ClipboardLoadResult(tab_id, Some(response)),
                    );
                } else {
                    let _ = self.pending_tx.send(
                        Message::ClipboardLoadResult(tab_id, None),
                    );
                }
            }
            Effect::TriggerBell(_) => {
                // No animation in headless mode.
            }
            Effect::RespondToTab { .. } => {
                // No parent socket in headless mode.
            }
            Effect::Exit => {
                // No-op in headless mode.
            }
        }
    }
}

impl Drop for HeadlessHost {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.runtime_dir);
    }
}

/// Convenience: drain pending messages until a predicate returns true or
/// the timeout expires. Returns true if the predicate was satisfied.
#[allow(dead_code)]
pub fn drain_until(
    host: &mut HeadlessHost,
    timeout: Duration,
    mut pred: impl FnMut(&HeadlessHost) -> bool,
) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if pred(host) {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        // Process one batch of pending messages.
        let mut got_any = false;
        while let Ok(msg) = host.pending_rx.try_recv() {
            host.step(msg);
            got_any = true;
        }
        if !got_any {
            match host.pending_rx.recv_timeout(Duration::from_millis(50)) {
                Ok(msg) => host.step(msg),
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => return pred(host),
            }
        }
    }
}

pub fn run(scenario_path: &Path) -> Result<(), Box<dyn Error>> {
    let raw = std::fs::read_to_string(scenario_path).map_err(HeadlessError::ReadScenario)?;
    let scenario: Scenario =
        serde_json::from_str(&raw).map_err(HeadlessError::ParseScenario)?;

    let mut host = HeadlessHost::new();

    // Auto-inject a hardcoded window resize so the home tab spawns.
    host.step(Message::WindowResized(Size {
        width: DEFAULT_WINDOW_WIDTH,
        height: DEFAULT_WINDOW_HEIGHT,
    }));

    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    for (index, action) in scenario.actions.into_iter().enumerate() {
        let message = match action {
            Action::WindowResized { width, height } => {
                Some(Message::WindowResized(Size { width, height }))
            }
            Action::NewTab => Some(Message::NewTab),
            Action::SpawnAgent => Some(Message::SpawnAgent),
            Action::SelectTab(id) => Some(Message::SelectTab(id)),
            Action::SelectTabByIndex(idx) => Some(Message::SelectTabByIndex(idx)),
            Action::CloseTab(id) => Some(Message::CloseTab(id)),
            Action::SetTitle { tab_id, title } => Some(Message::SetTitle(tab_id, title)),
            Action::SetStatus { tab_id, status } => {
                let parsed = AgentStatus::from_str(&status).ok_or_else(|| {
                    HeadlessError::UnknownStatus {
                        action_index: index,
                        status: status.clone(),
                    }
                })?;
                Some(Message::SetStatus(tab_id, parsed))
            }
            Action::PendingChar(c) => Some(Message::PendingInput(PendingKey::Char(c))),
            Action::PendingSubmit => Some(Message::PendingInput(PendingKey::Submit)),
            Action::PendingCancel => Some(Message::PendingInput(PendingKey::Cancel)),
            Action::Snapshot { label } => {
                use std::io::Write;
                let snap = host.snapshot(label);
                let line = serde_json::to_string(&snap)
                    .expect("HeadlessSnapshot serialization is infallible");
                writeln!(out, "{line}").map_err(HeadlessError::WriteSnapshot)?;
                None
            }
        };

        if let Some(msg) = message {
            host.step(msg);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Spawn a real shell tab, run a deterministic command, and verify the
    /// output appears in the terminal grid.
    #[test]
    fn real_pty_grid_content() {
        let mut host = HeadlessHost::new();

        // Boot: inject window resize to spawn home tab.
        host.step(Message::WindowResized(Size {
            width: DEFAULT_WINDOW_WIDTH,
            height: DEFAULT_WINDOW_HEIGHT,
        }));

        // Spawn a shell tab (non-claude).
        host.step(Message::NewTab);

        // Let the shell boot. The StartTab effect spawns a real PTY.
        // Wait until the shell tab (id 1) has produced at least one
        // TabOutput message, meaning the PTY is alive and the shell
        // has printed its prompt.
        let found_output = drain_until(
            &mut host,
            Duration::from_secs(5),
            |h| {
                // Check if any grid content exists for tab 1.
                h.grid_text(1)
                    .map(|t| !t.is_empty())
                    .unwrap_or(false)
            },
        );
        assert!(found_output, "shell tab should produce output within 5s");

        // Type a deterministic command.
        host.step(Message::PtyInput(
            b"echo mandelbot-headless-marker\r".to_vec(),
        ));

        // Wait for the marker to appear in the grid.
        let marker = "mandelbot-headless-marker";
        let found_marker = drain_until(
            &mut host,
            Duration::from_secs(5),
            |h| {
                h.grid_text(1)
                    .map(|t| {
                        // The marker appears twice: once in the command line
                        // and once in the output. Count occurrences — we
                        // want at least 2 (typed + echoed).
                        t.matches(marker).count() >= 2
                    })
                    .unwrap_or(false)
            },
        );
        assert!(
            found_marker,
            "echo output should appear in grid. Grid content:\n{}",
            host.grid_text(1).unwrap_or_default(),
        );
    }
}

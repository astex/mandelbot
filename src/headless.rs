//! Headless driver for mandelbot.
//!
//! Reads a JSON scenario file, constructs an `App` via `App::boot_headless`
//! (no Iced runtime, no parent socket bound), injects a `WindowResized` to
//! boot the home tab, then applies scenario actions and dumps typed snapshots
//! to stdout as newline-delimited JSON.
//!
//! LIMITATIONS (see `.claude/skills/test-mandelbot-headless/SKILL.md`):
//! - Every `iced::Task<Message>` returned by `App::update` is dropped, so
//!   anything that lives inside those tasks is silent: PTY spawn, tab
//!   streams, MCP parent-socket traffic, clipboard I/O, bell flashes, the
//!   window subscription.
//! - Suitable for exercising `App::update` state transitions (tab tree,
//!   focus, status, titles, fold), not rendered output or shell behavior.

use std::error::Error;
use std::fmt;
use std::path::Path;

use iced::Size;
use serde::Deserialize;

use crate::tab::AgentStatus;
use crate::ui::{App, Message, PendingKey};

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

pub fn run(scenario_path: &Path) -> Result<(), Box<dyn Error>> {
    let raw = std::fs::read_to_string(scenario_path).map_err(HeadlessError::ReadScenario)?;
    let scenario: Scenario =
        serde_json::from_str(&raw).map_err(HeadlessError::ParseScenario)?;

    let mut app = App::boot_headless();

    // Auto-inject a hardcoded window resize so the home tab spawns. Any
    // explicit WindowResized actions in the scenario override this later.
    let _ = app.update(Message::WindowResized(Size {
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
                let snap = app.headless_snapshot(label);
                let line = serde_json::to_string(&snap)
                    .expect("HeadlessSnapshot serialization is infallible");
                writeln!(out, "{line}").map_err(HeadlessError::WriteSnapshot)?;
                None
            }
        };

        if let Some(msg) = message {
            // Drop returned Task<Message>: see module-level comment.
            let _ = app.update(msg);
        }
    }

    Ok(())
}

//! Headless driver for mandelbot.
//!
//! Reads a JSON scenario file, boots an `App` without calling
//! `iced::application(...).run()`, injects a `WindowResized` to boot the
//! home tab, then applies the scenario actions and dumps snapshots to stdout.
//!
//! SPIKE LIMITATIONS (see `.claude/skills/test-mandelbot-headless/SKILL.md`):
//! - Returned `iced::Task<Message>` values are dropped → no PTY output, no
//!   clipboard, no MCP socket traffic, no window subscription.
//! - Suitable for exercising `App::update` state transitions, not rendered
//!   output or real shell behavior.

use std::error::Error;
use std::path::Path;

use iced::Size;
use serde::Deserialize;

use crate::tab::AgentStatus;
use crate::ui::{App, Message, PendingKey};

#[derive(Debug, Deserialize)]
pub struct Scenario {
    pub actions: Vec<Action>,
}

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

pub fn run(scenario_path: &Path) -> Result<(), Box<dyn Error>> {
    let raw = std::fs::read_to_string(scenario_path)?;
    let scenario: Scenario = serde_json::from_str(&raw)?;

    // Boot the app. Drop the returned listen task — its parent-socket thread
    // is only started when the task is polled, so dropping it closes the
    // socket cleanly without ever spawning the listener thread.
    let (mut app, _boot_task) = App::boot();

    // Auto-inject a hardcoded window resize so the home tab spawns.
    // SPIKE: hardcoded 1600x900.
    let _ = app.update(Message::WindowResized(Size {
        width: 1600.0,
        height: 900.0,
    }));

    for action in scenario.actions {
        let message: Option<Message> = match action {
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
                let s = AgentStatus::from_str(&status)
                    .unwrap_or_else(|| panic!("unknown status: {status}"));
                Some(Message::SetStatus(tab_id, s))
            }
            Action::PendingChar(c) => Some(Message::PendingInput(PendingKey::Char(c))),
            Action::PendingSubmit => Some(Message::PendingInput(PendingKey::Submit)),
            Action::PendingCancel => Some(Message::PendingInput(PendingKey::Cancel)),
            Action::Snapshot { label } => {
                let snap = app.headless_snapshot(label);
                println!("{}", serde_json::to_string_pretty(&snap).unwrap());
                None
            }
        };

        if let Some(msg) = message {
            // SPIKE: drop returned Task<Message>.
            let _ = app.update(msg);
        }
    }

    Ok(())
}

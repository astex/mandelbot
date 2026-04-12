use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use crate::tab::{TabEvent, TabSpawnParams, TermEventListener, TermInstance};

/// A formatter that converts clipboard text into the escape sequence to
/// write back to the PTY. Carried from `ClipboardLoadRequest`.
pub type ClipboardFormatter = Arc<dyn Fn(&str) -> String + Sync + Send + 'static>;

/// Pure effects emitted by `App::update`. The host (IcedHost or HeadlessHost)
/// is responsible for translating these into concrete I/O.
#[allow(dead_code)]
pub enum Effect {
    /// App has created a `TerminalTab` and inserted it into `App.tabs`;
    /// the host must start its I/O threads (tab_stream + fifo_stream).
    StartTab {
        tab_id: usize,
        params: TabSpawnParams,
        event_rx: mpsc::Receiver<TabEvent>,
        pty_event_tx: mpsc::Sender<TabEvent>,
        term: Arc<Mutex<TermInstance>>,
        listener: TermEventListener,
        fifo_path: PathBuf,
    },
    /// Write text to the system clipboard (OSC 52 set, clipboard type).
    WriteClipboard(String),
    /// Write text to the primary selection (OSC 52 set, selection type).
    WritePrimaryClipboard(String),
    /// Read from the system clipboard and deliver the result back to the
    /// tab as `Message::ClipboardLoadResult`.
    ReadClipboard {
        tab_id: usize,
        formatter: ClipboardFormatter,
    },
    /// Read from the primary selection and deliver the result back.
    ReadPrimaryClipboard {
        tab_id: usize,
        formatter: ClipboardFormatter,
    },
    /// Kick off (or re-trigger) a bell flash animation for the given tab.
    TriggerBell(usize),
    /// Reply to an MCP request waiting on a UnixStream in the host's
    /// response_writers map.
    RespondToTab {
        tab_id: usize,
        response: serde_json::Value,
    },
    /// Exit the application.
    Exit,
}

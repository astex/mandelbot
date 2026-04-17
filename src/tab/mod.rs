mod events;
mod grid;
mod stream;

pub mod config;

pub use config::{create_fifo, runtime_dir};
pub use events::{ClipboardLoadRequest, ClipboardStoreRequest};
pub(crate) use events::TermInstance;
pub use grid::logical_line_at;
pub use stream::{fifo_stream, tab_stream};

pub(crate) use events::TermEventListener;
pub(crate) use grid::{detect_prompt_pr_number, detect_prompt_shell_count};

use events::{color_to_rgb, new_term, TermColors};

use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use alacritty_terminal::event::WindowSize;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Point, Side};
use alacritty_terminal::selection::Selection;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::TermMode;

use std::sync::atomic::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentRank {
    Home,
    Project,
    Task,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AgentStatus {
    #[default]
    Idle,
    Working,
    Compacting,
    Blocked,
    NeedsReview,
    Error,
}

impl AgentStatus {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "idle" => Some(Self::Idle),
            "working" => Some(Self::Working),
            "compacting" => Some(Self::Compacting),
            "blocked" => Some(Self::Blocked),
            "needs_review" => Some(Self::NeedsReview),
            "error" => Some(Self::Error),
            _ => None,
        }
    }
}

pub enum TabEvent {
    PtyData(Vec<u8>),
    PtyEof,
    Input(Vec<u8>),
    Resize {
        rows: usize,
        cols: usize,
        pixel_width: u16,
        pixel_height: u16,
    },
    Scroll(i32),
    ScrollTo(usize),
    SetSelection(Option<Selection>),
    UpdateSelection(Point, Side),
    Shutdown,
}

pub struct TabSpawnParams {
    pub id: usize,
    pub rows: usize,
    pub cols: usize,
    pub is_claude: bool,
    pub rank: AgentRank,
    pub project_dir: Option<PathBuf>,
    pub shell: String,
    pub workflow: String,
    pub worktree_location: String,
    pub model: String,
    pub parent_socket: PathBuf,
    pub prompt: Option<String>,
    pub branch: Option<String>,
    pub base: Option<String>,
    pub control_prefix: String,
    /// Fresh session UUID to pass via `--session-id` when launching claude.
    pub session_id: Option<String>,
    /// If set, resume this existing session (pre-placed jsonl) instead of
    /// starting a fresh one. Used for replace/fork.
    pub resume_session_id: Option<String>,
    /// If set, skip worktree creation — the caller (replace/fork) has
    /// already placed a worktree here; the tab should cd into it and
    /// still mirror `.claude/settings.local.json` from the project root.
    pub existing_worktree: Option<PathBuf>,
}

pub struct TerminalTab {
    pub id: usize,
    /// Stable per-tab UUID. Unlike the numeric `id`, this survives across
    /// mandelbot restarts when a tab's durable state is rehydrated.
    /// Used as the on-disk key for `checkpoint_store`.
    pub uuid: String,
    pub is_claude: bool,
    pub rank: AgentRank,
    pub project_dir: Option<PathBuf>,
    pub parent_id: Option<usize>,
    pub depth: usize,
    pub project_id: Option<usize>,
    pub title: Option<String>,
    pub status: AgentStatus,
    pub background_tasks: usize,
    /// PR number detected by the status-line scraper. Written on every
    /// Claude output tick. Use `pr_number()` to read, not this field —
    /// the agent-set override wins when present.
    pub pr_scraped: Option<u32>,
    /// PR number set explicitly by an agent via the `set_pr` MCP tool.
    /// When `Some`, this wins over whatever the scraper sees.
    pub pr_override: Option<u32>,
    /// Epoch ms of the next scheduled Claude wake-up (from the
    /// `ScheduleWakeup` tool, captured via a PostToolUse hook). At
    /// most one outstanding per tab — `/loop` only ever has one in
    /// flight, and re-issues replace.  `None` once the deadline has
    /// passed or no wake-up has been scheduled.
    pub next_wakeup_at_ms: Option<u64>,
    pub pending_input: Option<String>,
    /// Claude session UUID for this tab (if `is_claude`).
    pub session_id: Option<String>,
    /// Worktree path (if task+git spawn).
    pub worktree_dir: Option<PathBuf>,
    /// Whether the checkpoint timeline strip is open under this tab.
    pub timeline_visible: bool,
    /// Cursor position in the timeline. `None` defaults to the tab's
    /// tip (its current checkpoint); arrow keys set `Some(id)` to
    /// scrub elsewhere in the tree.
    pub timeline_cursor: Option<String>,
    /// Stack of checkpoint ids to redo back into. Pushed on undo, popped
    /// on redo, cleared on any non-undo/non-redo activity. In-memory only.
    pub redo_path: Vec<String>,
    term: Arc<Mutex<TermInstance>>,
    listener: TermEventListener,
    event_tx: Option<mpsc::Sender<TabEvent>>,
}

impl TerminalTab {
    pub fn new(
        id: usize,
        rows: usize,
        cols: usize,
        is_claude: bool,
        rank: AgentRank,
        project_dir: Option<PathBuf>,
        parent_id: Option<usize>,
        depth: usize,
        project_id: Option<usize>,
    ) -> Self {
        let (term, listener) = new_term(cols, rows);
        Self {
            id,
            uuid: uuid::Uuid::new_v4().to_string(),
            is_claude,
            rank,
            project_dir,
            parent_id,
            depth,
            project_id,
            title: None,
            // Claude tabs run a setup script (worktree create, cd, etc.)
            // before claude launches and its hooks take over. Start
            // Working so the tab reflects that; the first Stop hook
            // will transition to Idle.
            status: if is_claude {
                AgentStatus::Working
            } else {
                AgentStatus::Idle
            },
            background_tasks: 0,
            pr_scraped: None,
            pr_override: None,
            next_wakeup_at_ms: None,
            pending_input: None,
            session_id: None,
            worktree_dir: None,
            timeline_visible: false,
            timeline_cursor: None,
            redo_path: Vec::new(),
            term: Arc::new(Mutex::new(term)),
            listener,
            event_tx: None,
        }
    }

    pub fn new_pending(
        id: usize,
        rows: usize,
        cols: usize,
        parent_id: usize,
    ) -> Self {
        let mut tab = Self::new(
            id,
            rows,
            cols,
            true,
            AgentRank::Project,
            None,
            Some(parent_id),
            1,
            Some(id),
        );
        tab.pending_input = Some(String::new());
        // Pending tab is waiting on the user, not running claude yet.
        tab.status = AgentStatus::Idle;
        tab
    }

    pub fn set_event_tx(
        &mut self,
        tx: mpsc::Sender<TabEvent>,
    ) {
        self.event_tx = Some(tx);
    }

    pub fn is_pending(&self) -> bool {
        self.pending_input.is_some()
    }

    /// The effective PR number for this tab: the agent-set override
    /// if present, otherwise whatever the status-line scraper saw.
    pub fn pr_number(&self) -> Option<u32> {
        self.pr_override.or(self.pr_scraped)
    }

    pub(crate) fn lock_term(
        &self,
    ) -> std::sync::MutexGuard<'_, TermInstance> {
        self.term.lock().unwrap()
    }

    pub(crate) fn term_arc(
        &self,
    ) -> Arc<Mutex<TermInstance>> {
        Arc::clone(&self.term)
    }

    pub(crate) fn listener(&self) -> TermEventListener {
        self.listener.clone()
    }

    // --- Side-effect draining (thread-safe via listener Arcs) ---

    /// Take the latest title set via OSC escape sequences, if any.
    pub fn take_osc_title(&self) -> Option<String> {
        self.listener.title.lock().unwrap().take()
    }

    /// Check and clear the bell flag set via BEL escape sequence.
    pub fn take_bell(&self) -> bool {
        self.listener.bell.swap(false, Ordering::Relaxed)
    }

    /// Update the theme colors used for OSC 10/11/12 color query
    /// responses.
    pub fn set_colors(
        &self,
        fg: iced::Color,
        bg: iced::Color,
        cursor: iced::Color,
    ) {
        *self.listener.colors.lock().unwrap() = TermColors {
            fg: color_to_rgb(fg),
            bg: color_to_rgb(bg),
            cursor: color_to_rgb(cursor),
        };
    }

    /// Update the window size used for OSC 18/19 text area size
    /// query responses.
    pub fn set_window_size(&self, size: WindowSize) {
        *self.listener.window_size.lock().unwrap() = size;
    }

    /// Drain any pending clipboard store requests.
    pub fn take_clipboard_stores(
        &self,
    ) -> Vec<ClipboardStoreRequest> {
        std::mem::take(
            &mut *self
                .listener
                .clipboard_stores
                .lock()
                .unwrap(),
        )
    }

    /// Drain any pending clipboard load requests.
    pub fn take_clipboard_loads(
        &self,
    ) -> Vec<ClipboardLoadRequest> {
        std::mem::take(
            &mut *self
                .listener
                .clipboard_loads
                .lock()
                .unwrap(),
        )
    }

    // --- Command senders (forward to tab thread via event_tx) ---

    pub fn write_input(&self, bytes: &[u8]) {
        if let Some(tx) = &self.event_tx {
            let _ = tx.send(TabEvent::Input(bytes.to_vec()));
        }
    }

    pub fn scroll(&self, delta: i32) {
        if let Some(tx) = &self.event_tx {
            let _ = tx.send(TabEvent::Scroll(delta));
        }
    }

    pub fn scroll_to(&self, offset: usize) {
        if let Some(tx) = &self.event_tx {
            let _ = tx.send(TabEvent::ScrollTo(offset));
        }
    }

    pub fn resize(
        &self,
        rows: usize,
        cols: usize,
        pixel_width: u16,
        pixel_height: u16,
    ) {
        if let Some(tx) = &self.event_tx {
            let _ = tx.send(TabEvent::Resize {
                rows,
                cols,
                pixel_width,
                pixel_height,
            });
        } else {
            // Pending tab — resize term directly.
            self.term
                .lock()
                .unwrap()
                .resize(TermSize::new(cols, rows));
        }
    }

    pub fn set_selection(
        &self,
        selection: Option<Selection>,
    ) {
        if let Some(tx) = &self.event_tx {
            let _ = tx.send(TabEvent::SetSelection(selection));
        }
    }

    pub fn update_selection(
        &self,
        point: Point,
        side: Side,
    ) {
        if let Some(tx) = &self.event_tx {
            let _ = tx.send(TabEvent::UpdateSelection(
                point, side,
            ));
        }
    }

    // --- Read-only methods (lock term briefly) ---

    pub fn rows(&self) -> usize {
        self.term.lock().unwrap().screen_lines()
    }

    pub fn history_size(&self) -> usize {
        self.term.lock().unwrap().grid().history_size()
    }

    pub fn mode(&self) -> TermMode {
        *self.term.lock().unwrap().mode()
    }

    pub fn cursor_blinking(&self) -> bool {
        self.term.lock().unwrap().cursor_style().blinking
    }
}

impl Drop for TerminalTab {
    fn drop(&mut self) {
        if let Some(tx) = &self.event_tx {
            let _ = tx.send(TabEvent::Shutdown);
        }
    }
}

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
}

pub struct TerminalTab {
    pub id: usize,
    pub is_claude: bool,
    pub rank: AgentRank,
    pub project_dir: Option<PathBuf>,
    pub parent_id: Option<usize>,
    pub depth: usize,
    pub project_id: Option<usize>,
    pub title: Option<String>,
    pub status: AgentStatus,
    pub background_tasks: usize,
    pub pr_number: Option<u32>,
    pub pending_input: Option<String>,
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
            is_claude,
            rank,
            project_dir,
            parent_id,
            depth,
            project_id,
            title: None,
            status: AgentStatus::default(),
            background_tasks: 0,
            pr_number: None,
            pending_input: None,
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

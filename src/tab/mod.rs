mod stream;

pub mod config;

pub use config::{create_fifo, runtime_dir};
pub use stream::{fifo_stream, tab_stream};

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::Selection;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{ClipboardType, Config, TermMode};
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::vte::ansi::Rgb;
use alacritty_terminal::Term;

/// Theme colors converted to alacritty's Rgb for responding to color
/// queries.
#[derive(Clone, Copy)]
struct TermColors {
    fg: Rgb,
    bg: Rgb,
    cursor: Rgb,
}

impl Default for TermColors {
    fn default() -> Self {
        Self {
            fg: Rgb { r: 0x83, g: 0x94, b: 0x96 },
            bg: Rgb { r: 0x00, g: 0x2b, b: 0x36 },
            cursor: Rgb { r: 0x83, g: 0x94, b: 0x96 },
        }
    }
}

/// A clipboard store request captured from the terminal.
pub struct ClipboardStoreRequest {
    pub clipboard_type: ClipboardType,
    pub text: String,
}

/// A clipboard load request captured from the terminal.
/// The `formatter` converts clipboard text into the escape sequence
/// to write back.
pub struct ClipboardLoadRequest {
    pub clipboard_type: ClipboardType,
    pub formatter:
        Arc<dyn Fn(&str) -> String + Sync + Send + 'static>,
}

#[derive(Clone)]
pub(crate) struct TermEventListener {
    title: Arc<Mutex<Option<String>>>,
    bell: Arc<AtomicBool>,
    colors: Arc<Mutex<TermColors>>,
    window_size: Arc<Mutex<WindowSize>>,
    /// Responses to write back to the PTY, drained after each feed.
    pub(crate) pty_responses: Arc<Mutex<Vec<String>>>,
    clipboard_stores: Arc<Mutex<Vec<ClipboardStoreRequest>>>,
    clipboard_loads: Arc<Mutex<Vec<ClipboardLoadRequest>>>,
}

impl TermEventListener {
    fn new() -> Self {
        Self {
            title: Arc::new(Mutex::new(None)),
            bell: Arc::new(AtomicBool::new(false)),
            colors: Arc::new(Mutex::new(TermColors::default())),
            window_size: Arc::new(Mutex::new(WindowSize {
                num_lines: 24,
                num_cols: 80,
                cell_width: 0,
                cell_height: 0,
            })),
            pty_responses: Arc::new(Mutex::new(Vec::new())),
            clipboard_stores: Arc::new(Mutex::new(Vec::new())),
            clipboard_loads: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl EventListener for TermEventListener {
    fn send_event(&self, event: Event) {
        match event {
            Event::Title(t) => {
                *self.title.lock().unwrap() = Some(t);
            }
            Event::ResetTitle => {
                *self.title.lock().unwrap() = None;
            }
            Event::Bell => {
                self.bell.store(true, Ordering::Relaxed);
            }
            Event::ColorRequest(index, callback) => {
                let colors = *self.colors.lock().unwrap();
                let rgb = match index {
                    // OSC 10 = foreground, 11 = background,
                    // 12 = cursor.
                    11 => colors.bg,
                    12 => colors.cursor,
                    _ => colors.fg,
                };
                let response = callback(rgb);
                self.pty_responses
                    .lock()
                    .unwrap()
                    .push(response);
            }
            Event::TextAreaSizeRequest(callback) => {
                let size = *self.window_size.lock().unwrap();
                let response = callback(size);
                self.pty_responses
                    .lock()
                    .unwrap()
                    .push(response);
            }
            Event::ClipboardStore(clipboard_type, text) => {
                self.clipboard_stores
                    .lock()
                    .unwrap()
                    .push(ClipboardStoreRequest {
                        clipboard_type,
                        text,
                    });
            }
            Event::ClipboardLoad(clipboard_type, formatter) => {
                self.clipboard_loads
                    .lock()
                    .unwrap()
                    .push(ClipboardLoadRequest {
                        clipboard_type,
                        formatter,
                    });
            }
            _ => {}
        }
    }
}

pub(crate) type TermInstance = Term<TermEventListener>;

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
    Blocked,
    NeedsReview,
    Error,
}

impl AgentStatus {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "idle" => Some(Self::Idle),
            "working" => Some(Self::Working),
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
        let size = TermSize::new(cols, rows);
        let listener = TermEventListener::new();
        let term = Term::new(
            Config::default(),
            &size,
            listener.clone(),
        );
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

pub struct LogicalLine {
    pub text: String,
    pub start_line: Line,
    pub cols: usize,
}

impl LogicalLine {
    /// Convert a grid point to a character offset in the logical
    /// line text.
    pub fn char_offset(
        &self,
        line: Line,
        col: usize,
    ) -> usize {
        let row_offset = (self.start_line.0 - line.0) as usize;
        row_offset * self.cols + col
    }

    /// Convert a character offset back to a grid (line, col) pair.
    pub fn grid_position(
        &self,
        char_offset: usize,
    ) -> (Line, usize) {
        let row_offset = char_offset / self.cols;
        let col = char_offset % self.cols;
        (
            Line(self.start_line.0 - row_offset as i32),
            col,
        )
    }
}

/// Extract the logical line containing the given grid line.
pub fn logical_line_at(
    term: &TermInstance,
    line: Line,
) -> LogicalLine {
    let grid = term.grid();
    let cols = grid.columns();
    let topmost = Line(-(grid.history_size() as i32));
    let bottommost = Line(grid.screen_lines() as i32 - 1);

    // Walk backwards to find the first row of this logical line.
    let mut start = line;
    loop {
        let prev = Line(start.0 + 1);
        if prev > bottommost {
            break;
        }
        if grid[prev][Column(cols - 1)]
            .flags
            .contains(Flags::WRAPLINE)
        {
            start = prev;
        } else {
            break;
        }
    }

    // Walk forward collecting text until we find a row without
    // WRAPLINE.
    let mut text = String::new();
    let mut current = start;
    loop {
        for col in 0..cols {
            text.push(grid[current][Column(col)].c);
        }
        if current <= topmost {
            break;
        }
        if grid[current][Column(cols - 1)]
            .flags
            .contains(Flags::WRAPLINE)
        {
            current = Line(current.0 - 1);
        } else {
            break;
        }
    }

    LogicalLine { text, start_line: start, cols }
}

/// Extract the text content of a single grid row, right-trimmed.
fn row_text(term: &TermInstance, line: Line) -> String {
    let grid = term.grid();
    let cols = grid.columns();
    let text: String =
        (0..cols).map(|col| grid[line][Column(col)].c).collect();
    text.trim_end().to_string()
}

/// Detect Claude Code's prompt frame and read the background shell
/// count.
fn detect_prompt_shell_count(
    term: &TermInstance,
) -> Option<usize> {
    let grid = term.grid();
    let screen_lines = grid.screen_lines();
    let cursor_line = grid.cursor.point.line.0;
    let top = (cursor_line - 2).max(0) as usize;
    let bot =
        ((cursor_line + 6) as usize).min(screen_lines - 1);
    let rows: Vec<String> = (top..=bot)
        .map(|i| row_text(term, Line(i as i32)))
        .collect();

    let mut first_border = None;
    let mut second_border = None;
    for (i, text) in rows.iter().enumerate() {
        if is_border_row(text) {
            if first_border.is_none() {
                first_border = Some(i);
            } else {
                second_border = Some(i);
                break;
            }
        }
    }

    let (Some(_top_border), Some(bot_border)) =
        (first_border, second_border)
    else {
        return None;
    };

    for i in (bot_border + 1)..rows.len() {
        if let Some(n) = parse_shell_count(&rows[i]) {
            return Some(n);
        }
    }

    Some(0)
}

/// Convert an iced `Color` (0.0–1.0 floats) to alacritty's `Rgb`.
fn color_to_rgb(c: iced::Color) -> Rgb {
    Rgb {
        r: (c.r * 255.0).round() as u8,
        g: (c.g * 255.0).round() as u8,
        b: (c.b * 255.0).round() as u8,
    }
}

/// Check if a row looks like a Claude Code prompt border (10+ '─'
/// characters).
fn is_border_row(text: &str) -> bool {
    text.len() >= 10 && text.chars().take(10).all(|c| c == '─')
}

/// Parse "· N shell(s)" from a line, returning N if found.
fn parse_shell_count(text: &str) -> Option<usize> {
    // The middle dot is U+00B7.
    let idx = text.find("· ")?;
    let after = &text[idx + "· ".len()..];
    let num_str: String =
        after.chars().take_while(|c| c.is_ascii_digit()).collect();
    let n: usize = num_str.parse().ok()?;
    if after[num_str.len()..].trim_start().starts_with("shell")
    {
        Some(n)
    } else {
        None
    }
}

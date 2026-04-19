use std::collections::{HashMap, HashSet};
use std::io::{BufRead, Write};
use std::os::unix::net as unix;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use futures::SinkExt;
use uuid::Uuid;

use iced::widget::{column, container, row, Space};
use iced::{Element, Fill, Size, Subscription, Task, Theme};

use alacritty_terminal::index::{Point as GridPoint, Side};
use alacritty_terminal::selection::Selection;

use crate::animation::FlashState;
use crate::checkpoint;
use crate::config::Config;
use crate::tab::{AgentRank, AgentStatus, TerminalTab};
use crate::theme::TerminalTheme;
use crate::toast::{self, Toast};
use crate::widget::terminal::{self, TerminalWidget};

pub(crate) const PADDING: f32 = 4.0;
pub(crate) const TAB_BAR_WIDTH: f32 = 400.0;
pub(crate) const TAB_GROUP_GAP: f32 = 28.0;
const INITIAL_ROWS: u16 = 50;
const INITIAL_COLS: u16 = 120;

pub fn initial_window_size(config: &Config) -> Size {
    Size {
        width: INITIAL_COLS as f32 * config.char_width() + PADDING * 2.0 + TAB_BAR_WIDTH,
        height: INITIAL_ROWS as f32 * config.char_height() + PADDING * 2.0,
    }
}

#[derive(Debug, Clone)]
pub enum PendingKey {
    Char(char),
    Backspace,
    Submit,
    Cancel,
}

#[derive(Debug, Clone)]
pub enum Message {
    TabOutput(usize, usize, Option<u32>),
    ShellExited(usize, Option<u32>),
    PtyInput(Vec<u8>),
    Scroll(i32),
    ScrollTo(usize),
    WindowResized(Size),
    NewTab,
    SpawnAgent,
    SpawnChild,
    CloseTab(usize),
    SelectTab(usize),
    SelectTabByIndex(usize),
    NavigateSibling(i32),
    NavigateRank(i32),
    FocusPreviousTab,
    NextIdle,
    PendingInput(PendingKey),
    McpSpawnAgent(usize, Option<PathBuf>, Option<usize>, Option<String>, Option<String>, Option<String>, Option<String>),
    McpCloseTab(usize, usize),
    McpCheckpoint(usize),
    McpReplace(usize, String),
    McpFork(usize, String, Option<String>),
    McpListTabs(usize),
    AutoCheckpoint(usize),
    TabReady { tab_id: usize, worktree_dir: Option<PathBuf>, session_id: Option<String> },
    SetTitle(usize, String),
    SetStatus(usize, AgentStatus),
    /// Agent-set PR number. `Some(n)` locks the PR to `n` and disables
    /// the status-line scraper for that tab; `None` clears both.
    SetPr(usize, Option<u32>),
    /// A `ScheduleWakeup` tool call resolved with a real wake-up
    /// scheduled at `epoch_ms`.  Captured by a PostToolUse hook on
    /// the tab's Claude session and piped through `MANDELBOT_FIFO`.
    WakeupAt(usize, u64),
    /// One-shot deadline tick for a previously scheduled wake-up.
    /// Fired by a delayed task spawned in the `WakeupAt` handler so
    /// the `⏱` chip clears even when no other event is in flight at
    /// the deadline.  Carries the original `epoch_ms` so a newer
    /// wake-up that supersedes this one is not cleared early.
    WakeupExpired(usize, u64),
    SetSelection(Option<Selection>),
    UpdateSelection(GridPoint, Side),
    Bell(usize),
    BellTick,
    ToggleFoldTab(usize),
    ClipboardLoadResult(usize, Option<String>),
    OpenPr(usize),
    ToggleTimeline(usize),
    TimelineScrub(usize, TimelineDir),
    TimelineActivate(usize, TimelineMode),
    Undo(usize),
    Redo(usize),
    ShowToast {
        source_tab_id: usize,
        message: String,
        prompt: Option<String>,
        target_tab_id: Option<usize>,
    },
    DismissToast(usize),
    SpawnFromToast(usize),
    FocusFromToast(usize),
    CheckpointDone {
        tab_id: usize,
        reason: CheckpointReason,
        result: Box<Result<checkpoint::CheckpointOutcome, String>>,
    },
    ForkDone {
        source_tab_id: usize,
        action: ForkAction,
        result: Box<Result<checkpoint::ForkOutcome, String>>,
    },
    /// Completion signal for background work whose result we discard
    /// (e.g. `save_tree`, or a dropped oneshot). No-op in `update`.
    BackgroundDone,
}

#[derive(Debug, Clone)]
pub enum CheckpointReason {
    /// `Message::AutoCheckpoint` — no MCP response, no UI follow-up.
    Auto,
    /// `Message::McpCheckpoint` — respond over the parent socket.
    Mcp,
    /// `Message::ToggleTimeline` opening — scroll timeline to cursor
    /// after the new tip lands.
    TimelineOpen,
}

#[derive(Debug, Clone)]
pub enum ForkAction {
    /// Spawn a new tab branched from the checkpoint; keep the source.
    Fork { prompt: Option<String> },
    /// Spawn, then close the source tab. `new_redo` is moved onto the
    /// new tab before the close — non-empty only on undo/redo.
    Replace { new_redo: Vec<String> },
}

#[derive(Debug, Clone, Copy)]
pub enum TimelineDir {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Debug, Clone, Copy)]
pub enum TimelineMode {
    Replace,
    Fork,
}

/// Run a blocking closure off the UI thread and turn its output into a
/// `Task<Message>`. Uses `std::thread::spawn` + a oneshot so we don't
/// depend on any particular async executor being current — iced 0.14
/// doesn't bind to the tokio runtime for `Task::perform` by default.
fn spawn_blocking_task<T, F>(
    f: F,
    on_done: impl FnOnce(T) -> Message + Send + 'static,
) -> Task<Message>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    Task::perform(
        async move {
            let (tx, rx) = futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                let _ = tx.send(f());
            });
            rx.await
        },
        move |res| match res {
            Ok(v) => on_done(v),
            Err(_) => Message::BackgroundDone,
        },
    )
}

/// Fire-and-forget variant for background work whose result we don't
/// need back on the UI thread (e.g. persisting the checkpoint tree to
/// disk). The completion message is `Message::BackgroundDone`, which is a
/// no-op in `update`.
fn spawn_blocking_discard<F>(f: F) -> Task<Message>
where
    F: FnOnce() + Send + 'static,
{
    spawn_blocking_task(
        move || {
            f();
        },
        |()| Message::BackgroundDone,
    )
}

fn terminal_size(window: Size, char_width: f32, char_height: f32) -> (usize, usize) {
    terminal_size_with_reserved(window, char_width, char_height, 0.0)
}

/// Same as `terminal_size` but reserves `reserved_px` vertical pixels
/// below the terminal (e.g. for the timeline strip).
fn terminal_size_with_reserved(
    window: Size,
    char_width: f32,
    char_height: f32,
    reserved_px: f32,
) -> (usize, usize) {
    let cols = ((window.width - PADDING * 2.0 - TAB_BAR_WIDTH - terminal::SCROLLBAR_WIDTH) / char_width).floor() as usize;
    let rows = ((window.height - PADDING * 2.0 - reserved_px) / char_height).floor() as usize;
    (rows.max(1), cols.max(1))
}

/// Writers for parent socket connections awaiting a response, keyed by tab ID.
type ResponseWriters = Arc<Mutex<HashMap<usize, unix::UnixStream>>>;

pub struct App {
    config: Config,
    tabs: Vec<TerminalTab>,
    active_tab_id: usize,
    prev_active_tab_id: Option<usize>,
    next_tab_id: usize,
    terminal_theme: TerminalTheme,
    window_size: Option<Size>,
    parent_socket_dir: PathBuf,
    parent_socket_path: PathBuf,
    response_writers: ResponseWriters,
    bell_flashes: FlashState,
    folded_tabs: HashSet<usize>,
    ckpt_store: crate::checkpoint_store::CheckpointStore,
    toasts: Vec<Toast>,
    next_toast_id: usize,
}

impl App {
    pub fn boot() -> (Self, Task<Message>) {
        let config = Config::load();
        let terminal_theme = config.terminal_theme();

        let mut ckpt_store = crate::checkpoint_store::load_all().unwrap_or_default();
        for outcome in ckpt_store.gc_orphans(&HashSet::new()) {
            let _ = outcome.persist(&ckpt_store);
        }

        let parent_socket_dir = crate::tab::runtime_dir();
        std::fs::create_dir_all(&parent_socket_dir).expect("failed to create socket dir");
        let parent_socket_path = parent_socket_dir.join("parent.sock");

        // Bind the listener before any tabs can spawn MCP servers.
        let listener = unix::UnixListener::bind(&parent_socket_path)
            .expect("failed to bind parent socket");

        let response_writers: ResponseWriters = Arc::new(Mutex::new(HashMap::new()));
        let listen_task = Task::run(
            parent_socket_stream(listener, Arc::clone(&response_writers)),
            |msg| msg,
        );

        let app = Self {
            config,
            tabs: Vec::new(),
            active_tab_id: 0,
            prev_active_tab_id: None,
            next_tab_id: 0,
            terminal_theme,
            window_size: None,
            parent_socket_dir,
            parent_socket_path,
            response_writers,
            bell_flashes: FlashState::default(),
            folded_tabs: HashSet::new(),
            ckpt_store,
            toasts: Vec::new(),
            next_toast_id: 0,
        };

        (app, listen_task)
    }

    fn active_tab(&self) -> Option<&TerminalTab> {
        self.tabs.iter().find(|t| t.id == self.active_tab_id)
    }

    fn active_tab_mut(&mut self) -> Option<&mut TerminalTab> {
        self.tabs.iter_mut().find(|t| t.id == self.active_tab_id)
    }

    fn focus_tab(&mut self, id: usize) {
        // If the target tab is hidden inside a fold, expand the fold chain.
        if let Some(pid) = self.tabs.iter().find(|t| t.id == id).and_then(|t| t.parent_id) {
            self.unfold_ancestors(pid);
        }
        if id != self.active_tab_id {
            self.prev_active_tab_id = Some(self.active_tab_id);
        }
        self.active_tab_id = id;
    }

    fn spawn_tab(
        &mut self,
        is_claude: bool,
        rank: AgentRank,
        project_dir: Option<PathBuf>,
        parent_id: Option<usize>,
        prompt: Option<String>,
        branch: Option<String>,
        model_override: Option<String>,
        base: Option<String>,
    ) -> (usize, Task<Message>) {
        self.spawn_tab_full(
            is_claude, rank, project_dir, parent_id, prompt,
            branch, model_override, base, None, None, None,
        )
    }

    fn spawn_tab_full(
        &mut self,
        is_claude: bool,
        rank: AgentRank,
        project_dir: Option<PathBuf>,
        parent_id: Option<usize>,
        prompt: Option<String>,
        branch: Option<String>,
        model_override: Option<String>,
        base: Option<String>,
        resume_session_id: Option<String>,
        existing_worktree: Option<PathBuf>,
        insert_position: Option<usize>,
    ) -> (usize, Task<Message>) {
        // Expand any folded ancestors so the new tab is visible.
        if let Some(pid) = parent_id {
            self.unfold_ancestors(pid);
        }

        let Some(size) = self.window_size else {
            return (0, Task::none());
        };
        let (rows, cols) = terminal_size(size, self.config.char_width(), self.config.char_height());
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        let parent = parent_id.and_then(|pid| self.tabs.iter().find(|t| t.id == pid));
        let depth = parent.map_or(0, |p| p.depth + 1);
        let project_id = match rank {
            AgentRank::Home => None,
            AgentRank::Project => Some(id),
            AgentRank::Task => parent.and_then(|p| p.project_id),
        };
        let model = model_override.unwrap_or_else(|| match rank {
            AgentRank::Home => self.config.models.home.clone(),
            AgentRank::Project => self.config.models.project.clone(),
            AgentRank::Task => self.config.models.task.clone(),
        });

        let mut tab = TerminalTab::new(
            id, rows, cols, is_claude, rank,
            project_dir.clone(), parent_id, depth, project_id,
        );

        // Create event channel: main thread → tab thread.
        let (event_tx, event_rx) = std::sync::mpsc::channel();
        let pty_tx = event_tx.clone();
        tab.set_event_tx(event_tx);

        let inserted_idx = match insert_position {
            Some(pos) if pos <= self.tabs.len() => {
                self.tabs.insert(pos, tab);
                pos
            }
            _ => {
                self.tabs.push(tab);
                self.tabs.len() - 1
            }
        };

        // Initialize colors and window size for OSC query responses.
        if let Some(tab) = self.tabs.get(inserted_idx) {
            tab.set_colors(
                self.terminal_theme.fg,
                self.terminal_theme.bg,
                self.terminal_theme.fg,
            );
            let cw = self.config.char_width();
            let ch = self.config.char_height();
            tab.set_window_size(alacritty_terminal::event::WindowSize {
                num_lines: rows as u16,
                num_cols: cols as u16,
                cell_width: cw as u16,
                cell_height: ch as u16,
            });
        }

        let session_id = if is_claude && resume_session_id.is_none() {
            Some(Uuid::new_v4().to_string())
        } else {
            None
        };

        let params = crate::tab::TabSpawnParams {
            id,
            rows,
            cols,
            is_claude,
            rank,
            project_dir,
            shell: self.config.shell.clone(),
            workflow: self.config.workflow.clone(),
            worktree_location: self.config.worktree_location.clone(),
            model,
            parent_socket: self.parent_socket_path.clone(),
            prompt,
            branch,
            base,
            control_prefix: self.config.control_prefix.to_string(),
            session_id,
            resume_session_id,
            existing_worktree,
        };

        let tab = &self.tabs[inserted_idx];
        let tab_task = Task::run(
            crate::tab::tab_stream(
                params,
                event_rx,
                pty_tx,
                tab.term_arc(),
                tab.listener(),
            ),
            |msg| msg,
        );

        // Create FIFO and start status reader.
        let fifo_path = crate::tab::runtime_dir().join(format!("{id}.fifo"));
        crate::tab::create_fifo(&fifo_path);
        let fifo_task = Task::run(
            crate::tab::fifo_stream(id, fifo_path),
            |msg| msg,
        );
        (id, Task::batch([tab_task, fifo_task]))
    }

    fn active_rank(&self) -> Option<AgentRank> {
        self.active_tab().map(|t| t.rank)
    }

    fn project_dir_for_tab(&self, tab_id: usize) -> Option<PathBuf> {
        let project_id = self.tabs.iter().find(|t| t.id == tab_id)?.project_id?;
        self.tabs.iter().find(|t| t.id == project_id)?.project_dir.clone()
    }

    fn first_child(&self, tab_id: usize) -> Option<usize> {
        self.tabs.iter()
            .find(|t| t.parent_id == Some(tab_id) && t.is_claude)
            .map(|t| t.id)
    }

    /// Returns tab IDs in tree display order: Home, then projects with their
    /// tasks nested underneath (recursively), then shell tabs. Descendants of
    /// folded tabs are omitted.
    fn tab_display_order(&self) -> Vec<usize> {
        let mut order = Vec::new();

        // Home agent first, then its descendants depth-first.
        if let Some(home) = self.tabs.iter().find(|t| t.rank == AgentRank::Home) {
            order.push(home.id);
            self.collect_children(home.id, &mut order);
        }

        // Shell tabs at the end.
        for tab in self.tabs.iter().filter(|t| !t.is_claude) {
            order.push(tab.id);
        }

        order
    }

    /// Returns a mapping `tab_id -> number` (0..=9) for which visible tabs
    /// get digit shortcuts. The 10 slots follow the active tab's neighborhood:
    /// Home + first shell + ancestors + active tab's siblings, then filled
    /// outward by depth (projects, then tasks, then subtasks, ...) with
    /// display order as the tiebreak, and shells last. Numbers are always
    /// assigned in display order top-to-bottom.
    fn tab_number_assignments(&self) -> HashMap<usize, usize> {
        let visible = self.tab_display_order();
        let is_visible = |id: usize| visible.contains(&id);

        let mut eligible: HashSet<usize> = HashSet::new();

        // 1. Home tab.
        if let Some(home) = self.tabs.iter().find(|t| t.rank == AgentRank::Home) {
            if is_visible(home.id) {
                eligible.insert(home.id);
            }
        }

        // 2. First shell tab in display order.
        if eligible.len() < 10 {
            if let Some(shell_id) = visible.iter().copied().find(|&id| {
                self.tabs.iter().find(|t| t.id == id).map(|t| !t.is_claude).unwrap_or(false)
            }) {
                eligible.insert(shell_id);
            }
        }

        // 3. Ancestors of the active tab, walking the parent chain.
        if let Some(active_tab) = self.tabs.iter().find(|t| t.id == self.active_tab_id) {
            let mut cur = active_tab.parent_id;
            while let Some(pid) = cur {
                if eligible.len() >= 10 { break; }
                if is_visible(pid) {
                    eligible.insert(pid);
                }
                cur = self.tabs.iter().find(|t| t.id == pid).and_then(|t| t.parent_id);
            }

            // 4. The active tab itself, then its siblings.
            if eligible.len() < 10 && is_visible(active_tab.id) {
                eligible.insert(active_tab.id);
            }
            let active_parent = active_tab.parent_id;
            let active_is_claude = active_tab.is_claude;
            for t in self.tabs.iter() {
                if eligible.len() >= 10 { break; }
                if t.id != active_tab.id
                    && t.parent_id == active_parent
                    && t.is_claude == active_is_claude
                    && is_visible(t.id)
                {
                    eligible.insert(t.id);
                }
            }
        }

        // 5. Everything else by (depth, display order): projects first, then
        //    tasks, then subtasks, and so on. Shell tabs go last.
        let mut claude_by_depth: Vec<(usize, usize)> = visible.iter()
            .filter_map(|&id| {
                self.tabs.iter()
                    .find(|t| t.id == id)
                    .filter(|t| t.is_claude)
                    .map(|t| (t.depth, id))
            })
            .collect();
        // Stable sort preserves display order within equal depth.
        claude_by_depth.sort_by_key(|&(depth, _)| depth);
        for (_, id) in claude_by_depth {
            if eligible.len() >= 10 { break; }
            eligible.insert(id);
        }
        for &id in &visible {
            if eligible.len() >= 10 { break; }
            if let Some(t) = self.tabs.iter().find(|t| t.id == id) {
                if !t.is_claude {
                    eligible.insert(id);
                }
            }
        }

        // Assign numbers in display order.
        let mut assignments = HashMap::new();
        let mut next = 0_usize;
        for &id in &visible {
            if next > 9 { break; }
            if eligible.contains(&id) {
                assignments.insert(id, next);
                next += 1;
            }
        }
        assignments
    }

    fn collect_children(&self, parent_id: usize, order: &mut Vec<usize>) {
        for tab in self.tabs.iter().filter(|t| t.parent_id == Some(parent_id) && t.is_claude) {
            order.push(tab.id);
            if !self.folded_tabs.contains(&tab.id) {
                self.collect_children(tab.id, order);
            }
        }
    }

    fn has_claude_children(&self, parent_id: usize) -> bool {
        self.tabs.iter().any(|t| t.parent_id == Some(parent_id) && t.is_claude)
    }

    /// Remove the given tab and all its ancestors from the folded set.
    fn unfold_ancestors(&mut self, mut id: usize) {
        loop {
            self.folded_tabs.remove(&id);
            match self.tabs.iter().find(|t| t.id == id).and_then(|t| t.parent_id) {
                Some(pid) => id = pid,
                None => break,
            }
        }
    }

    fn find_project_for_dir(&self, dir: &Path) -> Option<usize> {
        self.tabs.iter()
            .find(|t| t.rank == AgentRank::Project && t.project_dir.as_deref() == Some(dir))
            .map(|t| t.id)
    }

    fn respond_to_tab(&self, tab_id: usize, response: serde_json::Value) {
        if let Some(mut stream) = self.response_writers.lock().unwrap().remove(&tab_id) {
            let mut msg = serde_json::to_string(&response).unwrap();
            msg.push('\n');
            let _ = stream.write_all(msg.as_bytes());
            let _ = stream.flush();
        }
    }

    /// Pick the tab to focus after closing `closing_id`, given its pre-close
    /// parent and position in `self.tabs`. Prefers prev sibling, then next
    /// sibling, then parent. `closing_ids` are treated as already gone.
    fn pick_focus_after_close(
        &self,
        closing_parent_id: Option<usize>,
        anchor_idx: usize,
        closing_ids: &[usize],
    ) -> Option<usize> {
        let sibling_at =
            |pos: usize| -> Option<usize> {
                self.tabs.get(pos).and_then(|t| {
                    (t.parent_id == closing_parent_id
                        && !closing_ids.contains(&t.id))
                    .then_some(t.id)
                })
            };
        let prev = (0..anchor_idx)
            .rev()
            .find_map(sibling_at);
        let next = (anchor_idx..self.tabs.len())
            .find_map(sibling_at);
        prev.or(next).or_else(|| {
            closing_parent_id.filter(|p| !closing_ids.contains(p))
        })
    }

    fn close_tab(&mut self, tab_id: usize) -> Task<Message> {
        self.folded_tabs.remove(&tab_id);

        let Some(idx) = self.tabs.iter().position(|t| t.id == tab_id) else {
            return Task::none();
        };

        let tab_uuid = self.tabs[idx].uuid.clone();
        let _ = self.ckpt_store.close_tab(&tab_uuid).persist(&self.ckpt_store);

        let closing_parent_id = self.tabs[idx].parent_id;
        let closing_depth = self.tabs[idx].depth;

        // Promote the first child to take this tab's place in the hierarchy.
        let first_child_id = self.tabs.iter()
            .find(|t| t.parent_id == Some(tab_id))
            .map(|t| t.id);

        if let Some(promoted_id) = first_child_id {
            // Promoted child inherits the closing tab's parent and depth.
            if let Some(promoted) = self.tabs.iter_mut().find(|t| t.id == promoted_id) {
                promoted.parent_id = closing_parent_id;
                promoted.depth = closing_depth;
            }
            // Re-parent remaining children under the promoted child.
            for tab in self.tabs.iter_mut() {
                if tab.parent_id == Some(tab_id) && tab.id != promoted_id {
                    tab.parent_id = Some(promoted_id);
                }
            }
        }

        self.tabs.remove(idx);

        if self.prev_active_tab_id == Some(tab_id) {
            self.prev_active_tab_id = None;
        }

        if self.tabs.is_empty() {
            return iced::exit();
        }

        if self.active_tab_id == tab_id {
            let new_id = self
                .pick_focus_after_close(closing_parent_id, idx, &[tab_id])
                .unwrap_or_else(|| {
                    let fallback = idx.min(self.tabs.len() - 1);
                    self.tabs[fallback].id
                });
            self.focus_tab(new_id);
        }

        Task::none()
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::WindowResized(size) if self.window_size.is_none() => {
                self.window_size = Some(size);
                let home = std::env::var("HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|_| PathBuf::from("."));
                let mandelbot_dir = home.join(".mandelbot");
                let _ = std::fs::create_dir_all(&mandelbot_dir);
                let initialized = mandelbot_dir.join(".initialized");
                let first_run_prompt = if !initialized.exists() {
                    let _ = std::fs::write(&initialized, "");
                    Some("/mandelbot-features".to_string())
                } else {
                    None
                };
                let (id, task) = self.spawn_tab(true, AgentRank::Home, Some(home), None, first_run_prompt, None, None, None);
                self.focus_tab(id);
                if let Some(tab) = self.active_tab_mut() {
                    tab.title = Some("home".into());
                }
                task
            }
            Message::WindowResized(size) => {
                self.window_size = Some(size);
                let cw = self.config.char_width();
                let ch = self.config.char_height();
                let store = &self.ckpt_store;
                let cfg = &self.config;
                for tab in &mut self.tabs {
                    let reserved = crate::widget::timeline::pixel_height(store, tab, cfg);
                    let (rows, cols) = terminal_size_with_reserved(size, cw, ch, reserved);
                    tab.resize(rows, cols, size.width as u16, size.height as u16);
                    tab.set_window_size(alacritty_terminal::event::WindowSize {
                        num_lines: rows as u16,
                        num_cols: cols as u16,
                        cell_width: cw as u16,
                        cell_height: ch as u16,
                    });
                }
                Task::none()
            }
            Message::TabOutput(tab_id, bg_tasks, pr_number) => {
                let mut tasks: Vec<Task<Message>> = Vec::new();
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                    tab.background_tasks = bg_tasks;
                    tab.pr_scraped = pr_number;
                    if !tab.is_claude {
                        if let Some(title) = tab.take_osc_title() {
                            tab.title = Some(title);
                        }
                    }
                    if tab.take_bell() {
                        return self.bell_flashes.trigger(tab_id);
                    }

                    // Handle clipboard store requests (OSC 52 set).
                    for store in tab.take_clipboard_stores() {
                        let task = match store.clipboard_type {
                            alacritty_terminal::term::ClipboardType::Clipboard => {
                                iced::clipboard::write(store.text)
                            }
                            alacritty_terminal::term::ClipboardType::Selection => {
                                iced::clipboard::write_primary(store.text)
                            }
                        };
                        tasks.push(task);
                    }

                    // Handle clipboard load requests (OSC 52 query).
                    for load in tab.take_clipboard_loads() {
                        let task = match load.clipboard_type {
                            alacritty_terminal::term::ClipboardType::Clipboard => {
                                iced::clipboard::read()
                            }
                            alacritty_terminal::term::ClipboardType::Selection => {
                                iced::clipboard::read_primary()
                            }
                        };
                        let task = task.map(move |content| {
                            let response = content.map(|text| (load.formatter)(&text));
                            Message::ClipboardLoadResult(tab_id, response)
                        });
                        tasks.push(task);
                    }
                }
                Task::batch(tasks)
            }
            Message::ShellExited(tab_id, exit_code) => match exit_code {
                Some(0) | None => self.close_tab(tab_id),
                Some(_code) => {
                    // Exit hint is written by the tab thread.
                    if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                        tab.status = AgentStatus::Error;
                    }
                    Task::none()
                }
            }
            Message::SetTitle(tab_id, title) => {
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                    tab.title = Some(title);
                }
                Task::none()
            }
            Message::McpSpawnAgent(requesting_tab_id, working_directory, project_tab_id, prompt, branch, model_override, base) => {
                let requester = self.tabs.iter().find(|t| t.id == requesting_tab_id);
                let Some(requester) = requester else {
                    self.respond_to_tab(requesting_tab_id, serde_json::json!({"error": "unknown tab"}));
                    return Task::none();
                };

                let (rank, project_dir, parent_id) = match requester.rank {
                    AgentRank::Home => {
                        if let Some(ptid) = project_tab_id {
                            // Spawn a task under an existing project.
                            let project = self.tabs.iter().find(|t| t.id == ptid);
                            let Some(project) = project else {
                                self.respond_to_tab(requesting_tab_id, serde_json::json!({"error": "unknown project tab"}));
                                return Task::none();
                            };
                            if project.rank != AgentRank::Project {
                                self.respond_to_tab(requesting_tab_id, serde_json::json!({"error": "target tab is not a project agent"}));
                                return Task::none();
                            }
                            let dir = project.project_dir.clone();
                            (AgentRank::Task, dir, Some(ptid))
                        } else {
                            let Some(wd) = working_directory else {
                                self.respond_to_tab(requesting_tab_id, serde_json::json!({"error": "working_directory or project_tab_id required from home agent"}));
                                return Task::none();
                            };
                            let canonical = std::fs::canonicalize(&wd).unwrap_or(wd);
                            if let Some(existing) = self.find_project_for_dir(&canonical) {
                                self.respond_to_tab(requesting_tab_id, serde_json::json!({"tab_id": existing}));
                                self.focus_tab(existing);
                                return Task::none();
                            }
                            (AgentRank::Project, Some(canonical), Some(requesting_tab_id))
                        }
                    }
                    AgentRank::Project => {
                        let dir = requester.project_dir.clone();
                        (AgentRank::Task, dir, Some(requesting_tab_id))
                    }
                    AgentRank::Task => {
                        let dir = self.project_dir_for_tab(requesting_tab_id);
                        (AgentRank::Task, dir, Some(requesting_tab_id))
                    }
                };

                let (new_tab_id, task) = self.spawn_tab(true, rank, project_dir, parent_id, prompt, branch, model_override, base);
                self.respond_to_tab(requesting_tab_id, serde_json::json!({"tab_id": new_tab_id}));
                task
            }
            Message::SetStatus(tab_id, status) => {
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                    tab.status = status;
                }
                Task::none()
            }
            Message::SetPr(tab_id, pr) => {
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                    tab.pr_override = pr;
                }
                Task::none()
            }
            Message::WakeupAt(tab_id, epoch_ms) => {
                if let Some(tab) =
                    self.tabs.iter_mut().find(|t| t.id == tab_id)
                {
                    tab.next_wakeup_at_ms = Some(epoch_ms);
                }
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);
                let delay_ms = epoch_ms.saturating_sub(now_ms);
                Task::perform(
                    async move {
                        let (tx, rx) =
                            futures::channel::oneshot::channel();
                        std::thread::spawn(move || {
                            std::thread::sleep(
                                std::time::Duration::from_millis(
                                    delay_ms,
                                ),
                            );
                            let _ = tx.send(());
                        });
                        let _ = rx.await;
                    },
                    move |_| {
                        Message::WakeupExpired(tab_id, epoch_ms)
                    },
                )
            }
            Message::WakeupExpired(tab_id, epoch_ms) => {
                if let Some(tab) =
                    self.tabs.iter_mut().find(|t| t.id == tab_id)
                {
                    if tab.next_wakeup_at_ms == Some(epoch_ms) {
                        tab.next_wakeup_at_ms = None;
                    }
                }
                Task::none()
            }
            Message::Bell(tab_id) => self.bell_flashes.trigger(tab_id),
            Message::BellTick => self.bell_flashes.tick(),
            Message::PtyInput(bytes) => {
                if let Some(tab) = self.active_tab_mut() {
                    tab.write_input(&bytes);
                    // When the user presses Enter while an agent is in
                    // NeedsReview (e.g. submitting feedback on an
                    // ExitPlanMode permission dialog), transition to
                    // Working.  There is no Claude Code hook that fires
                    // on permission responses, so we detect the
                    // transition on the mandelbot side.  We check for
                    // bare \r (Enter without modifiers) to avoid false
                    // triggers from arrow keys or other navigation.
                    if tab.is_claude
                        && tab.status == AgentStatus::NeedsReview
                        && bytes == b"\r"
                    {
                        tab.status = AgentStatus::Working;
                    }
                }
                Task::none()
            }
            Message::Scroll(delta) => {
                if let Some(tab) = self.active_tab_mut() {
                    tab.scroll(delta);
                }
                Task::none()
            }
            Message::ScrollTo(offset) => {
                if let Some(tab) = self.active_tab_mut() {
                    tab.scroll_to(offset);
                }
                Task::none()
            }
            Message::NewTab => {
                let (id, task) = self.spawn_tab(false, AgentRank::Home, None, None, None, None, None, None);
                self.focus_tab(id);
                task
            }
            Message::SpawnAgent => {
                match self.active_rank() {
                    Some(AgentRank::Home) => {
                        // Create a pending project tab.
                        let Some(size) = self.window_size else {
                            return Task::none();
                        };
                        let (rows, cols) = terminal_size(size, self.config.char_width(), self.config.char_height());
                        let home_id = self.active_tab_id;
                        let id = self.next_tab_id;
                        self.next_tab_id += 1;
                        let tab = TerminalTab::new_pending(id, rows, cols, home_id);
                        self.tabs.push(tab);
                        self.focus_tab(id);
                        Task::none()
                    }
                    Some(AgentRank::Project | AgentRank::Task) => {
                        let parent_id = self.active_tab()
                            .and_then(|t| if t.rank == AgentRank::Task { t.parent_id } else { Some(t.id) });
                        let project_dir = self.project_dir_for_tab(self.active_tab_id);
                        if let (Some(pid), Some(dir)) = (parent_id, project_dir) {
                            let (id, task) = self.spawn_tab(true, AgentRank::Task, Some(dir), Some(pid), None, None, None, None);
                            self.focus_tab(id);
                            task
                        } else {
                            Task::none()
                        }
                    }
                    None => Task::none(),
                }
            }
            Message::SpawnChild => {
                match self.active_rank() {
                    Some(AgentRank::Home) => {
                        // Same as SpawnAgent from home: create a pending project tab.
                        let Some(size) = self.window_size else {
                            return Task::none();
                        };
                        let (rows, cols) = terminal_size(size, self.config.char_width(), self.config.char_height());
                        let home_id = self.active_tab_id;
                        let id = self.next_tab_id;
                        self.next_tab_id += 1;
                        let tab = TerminalTab::new_pending(id, rows, cols, home_id);
                        self.tabs.push(tab);
                        self.focus_tab(id);
                        Task::none()
                    }
                    Some(AgentRank::Project | AgentRank::Task) => {
                        if let Some(dir) = self.project_dir_for_tab(self.active_tab_id) {
                            let (id, task) = self.spawn_tab(true, AgentRank::Task, Some(dir), Some(self.active_tab_id), None, None, None, None);
                            self.focus_tab(id);
                            task
                        } else {
                            Task::none()
                        }
                    }
                    None => Task::none(),
                }
            }
            Message::NavigateSibling(delta) => {
                let order = self.tab_display_order();
                if let Some(idx) = order.iter().position(|&id| id == self.active_tab_id) {
                    let new_idx = (idx as i32 + delta)
                        .rem_euclid(order.len() as i32) as usize;
                    self.focus_tab(order[new_idx]);
                }
                Task::none()
            }
            Message::NavigateRank(delta) => {
                if delta > 0 {
                    // Go to first child.
                    if let Some(child) = self.first_child(self.active_tab_id) {
                        self.focus_tab(child);
                    }
                } else {
                    // Go to parent.
                    if let Some(tab) = self.active_tab() {
                        if let Some(pid) = tab.parent_id {
                            self.focus_tab(pid);
                        }
                    }
                }
                Task::none()
            }
            Message::FocusPreviousTab => {
                if let Some(prev) = self.prev_active_tab_id {
                    if self.tabs.iter().any(|t| t.id == prev) {
                        self.focus_tab(prev);
                    }
                }
                Task::none()
            }
            Message::NextIdle => {
                let order = self.tab_display_order();
                let cur = order.iter().position(|&id| id == self.active_tab_id).unwrap_or(0);

                // Search order rotated to start after current tab.
                let candidates: Vec<usize> = order.iter()
                    .copied()
                    .cycle()
                    .skip(cur + 1)
                    .take(order.len())
                    .collect();

                let status_of = |id: usize| -> Option<(AgentStatus, AgentRank)> {
                    self.tabs.iter().find(|t| t.id == id).map(|t| (t.status, t.rank))
                };

                // Priority: Blocked > NeedsReview > Idle Task > Idle Project.
                let target = candidates.iter().find(|&&id| matches!(status_of(id), Some((AgentStatus::Blocked, _))))
                    .or_else(|| candidates.iter().find(|&&id| matches!(status_of(id), Some((AgentStatus::NeedsReview, _)))))
                    .or_else(|| candidates.iter().find(|&&id| matches!(status_of(id), Some((AgentStatus::Idle, AgentRank::Task)))))
                    .or_else(|| candidates.iter().find(|&&id| matches!(status_of(id), Some((AgentStatus::Idle, AgentRank::Project)))));

                if let Some(&id) = target {
                    self.focus_tab(id);
                }
                Task::none()
            }
            Message::PendingInput(key) => {
                let tab_id = self.active_tab_id;
                let tab = self.tabs.iter_mut().find(|t| t.id == tab_id);
                let Some(tab) = tab else { return Task::none() };
                let Some(input) = &mut tab.pending_input else { return Task::none() };

                match key {
                    PendingKey::Char(c) => {
                        input.push(c);
                        Task::none()
                    }
                    PendingKey::Backspace => {
                        input.pop();
                        Task::none()
                    }
                    PendingKey::Cancel => {
                        self.close_tab(tab_id)
                    }
                    PendingKey::Submit => {
                        let raw_path = input.clone();
                        let expanded = if raw_path.starts_with('~') {
                            let home = std::env::var("HOME").unwrap_or_default();
                            raw_path.replacen('~', &home, 1)
                        } else {
                            raw_path
                        };
                        let path = PathBuf::from(&expanded);
                        let canonical = std::fs::canonicalize(&path)
                            .unwrap_or(path);

                        // Check if project already exists for this dir.
                        if let Some(existing) = self.find_project_for_dir(&canonical) {
                            self.focus_tab(existing);
                            return self.close_tab(tab_id);
                        }

                        // Replace pending tab with a real project agent.
                        let Some(idx) = self.tabs.iter().position(|t| t.id == tab_id) else {
                            return Task::none();
                        };
                        let parent_id = self.tabs[idx].parent_id;
                        self.tabs.remove(idx);

                        let (id, task) = self.spawn_tab(
                            true,
                            AgentRank::Project,
                            Some(canonical),
                            parent_id,
                            None,
                            None,
                            None,
                            None,
                        );
                        self.focus_tab(id);
                        task
                    }
                }
            }
            Message::CloseTab(tab_id) => self.close_tab(tab_id),
            Message::McpCloseTab(requesting_tab_id, target_tab_id) => {
                // Verify the requester is the target itself or an ancestor.
                let authorized = if requesting_tab_id == target_tab_id {
                    true
                } else {
                    let mut current = Some(target_tab_id);
                    let mut found = false;
                    while let Some(id) = current {
                        let tab = self.tabs.iter().find(|t| t.id == id);
                        match tab {
                            Some(t) => {
                                if t.parent_id == Some(requesting_tab_id) {
                                    found = true;
                                    break;
                                }
                                current = t.parent_id;
                            }
                            None => break,
                        }
                    }
                    found
                };

                if !authorized {
                    self.respond_to_tab(requesting_tab_id, serde_json::json!({
                        "error": "not authorized to close that tab"
                    }));
                    return Task::none();
                }

                // Collect the target and all its descendants.
                let mut to_close = vec![target_tab_id];
                let mut i = 0;
                while i < to_close.len() {
                    let parent = to_close[i];
                    for tab in &self.tabs {
                        if tab.parent_id == Some(parent) && !to_close.contains(&tab.id) {
                            to_close.push(tab.id);
                        }
                    }
                    i += 1;
                }

                for &id in &to_close {
                    self.folded_tabs.remove(&id);
                }
                if to_close.contains(&self.active_tab_id) {
                    // Prefer prev/next sibling of the subtree root, then its
                    // parent; fall back to nearest surviving tab by index.
                    let (root_idx, root_parent) = self.tabs.iter()
                        .position(|t| t.id == target_tab_id)
                        .map(|i| (i, self.tabs[i].parent_id))
                        .unwrap_or((0, None));
                    let new_id = self
                        .pick_focus_after_close(root_parent, root_idx, &to_close)
                        .or_else(|| {
                            self.tabs.iter()
                                .enumerate()
                                .filter(|(_, t)| !to_close.contains(&t.id))
                                .min_by_key(|(idx, _)| {
                                    (*idx as isize - root_idx as isize).unsigned_abs()
                                })
                                .map(|(_, t)| t.id)
                        });
                    if let Some(id) = new_id {
                        self.focus_tab(id);
                    }
                }
                if self.prev_active_tab_id.is_some_and(|id| to_close.contains(&id)) {
                    self.prev_active_tab_id = None;
                }

                let count = to_close.len();
                self.tabs.retain(|t| !to_close.contains(&t.id));

                if self.tabs.is_empty() {
                    return iced::exit();
                }

                self.respond_to_tab(requesting_tab_id, serde_json::json!({
                    "message": format!("Closed {count} tab(s)")
                }));
                Task::none()
            }
            Message::SelectTab(tab_id) => {
                if self.tabs.iter().any(|t| t.id == tab_id) {
                    self.focus_tab(tab_id);
                }
                Task::none()
            }
            Message::SelectTabByIndex(index) => {
                let assignments = self.tab_number_assignments();
                if let Some((&tab_id, _)) = assignments.iter().find(|&(_, &n)| n == index) {
                    self.focus_tab(tab_id);
                }
                Task::none()
            }
            Message::ToggleFoldTab(tab_id) => {
                let foldable = self.tabs.iter()
                    .find(|t| t.id == tab_id)
                    .is_some_and(|t| t.is_claude && t.rank != AgentRank::Home);
                if !foldable {
                    return Task::none();
                }
                if self.folded_tabs.contains(&tab_id) {
                    self.folded_tabs.remove(&tab_id);
                } else if self.has_claude_children(tab_id) {
                    self.folded_tabs.insert(tab_id);
                }
                Task::none()
            }
            Message::SetSelection(sel) => {
                if let Some(tab) = self.active_tab_mut() {
                    tab.set_selection(sel);
                }
                Task::none()
            }
            Message::UpdateSelection(point, side) => {
                if let Some(tab) = self.active_tab_mut() {
                    tab.update_selection(point, side);
                }
                Task::none()
            }
            Message::ClipboardLoadResult(tab_id, response) => {
                if let Some(response) = response {
                    if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                        tab.write_input(response.as_bytes());
                    }
                }
                Task::none()
            }
            Message::OpenPr(tab_id) => {
                if let Some(tab) = self.tabs.iter().find(|t| t.id == tab_id) {
                    if let (Some(pr), Some(dir)) = (tab.pr_number(), &tab.project_dir) {
                        if let Some(slug) = crate::links::github_slug_for_dir(dir) {
                            let url = format!("https://github.com/{slug}/pull/{pr}");
                            let _ = open::that(url);
                        }
                    }
                }
                Task::none()
            }
            Message::TabReady { tab_id, worktree_dir, session_id } => {
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                    tab.worktree_dir = worktree_dir;
                    tab.session_id = session_id;
                }
                Task::none()
            }
            Message::McpCheckpoint(requesting_tab_id) => {
                self.kick_checkpoint(requesting_tab_id, CheckpointReason::Mcp)
            }
            Message::McpReplace(requesting_tab_id, ckpt_id) => {
                self.handle_replace(requesting_tab_id, ckpt_id)
            }
            Message::McpFork(requesting_tab_id, ckpt_id, prompt) => {
                self.handle_fork(requesting_tab_id, ckpt_id, prompt)
            }
            Message::McpListTabs(requesting_tab_id) => {
                let is_home = self.tabs.iter()
                    .find(|t| t.id == requesting_tab_id)
                    .is_some_and(|t| t.rank == AgentRank::Home);

                let mut visible: Vec<usize> = vec![requesting_tab_id];
                if is_home {
                    visible = self.tabs.iter().map(|t| t.id).collect();
                } else {
                    let mut i = 0;
                    while i < visible.len() {
                        let parent = visible[i];
                        for t in &self.tabs {
                            if t.parent_id == Some(parent) && !visible.contains(&t.id) {
                                visible.push(t.id);
                            }
                        }
                        i += 1;
                    }
                }

                let tabs_json: Vec<serde_json::Value> = self.tabs.iter()
                    .filter(|t| visible.contains(&t.id))
                    .map(|t| {
                        let rank = match t.rank {
                            AgentRank::Home => "home",
                            AgentRank::Project => "project",
                            AgentRank::Task => "task",
                        };
                        let status = match t.status {
                            AgentStatus::Idle => "idle",
                            AgentStatus::Working => "working",
                            AgentStatus::Compacting => "compacting",
                            AgentStatus::Blocked => "blocked",
                            AgentStatus::NeedsReview => "needs_review",
                            AgentStatus::Error => "error",
                        };
                        serde_json::json!({
                            "id": t.id,
                            "parent_id": t.parent_id,
                            "title": t.title,
                            "rank": rank,
                            "status": status,
                            "is_claude": t.is_claude,
                            "project_dir": t.project_dir.as_ref().map(|p| p.display().to_string()),
                            "worktree_dir": t.worktree_dir.as_ref().map(|p| p.display().to_string()),
                            "pr": t.pr_number(),
                        })
                    })
                    .collect();

                self.respond_to_tab(requesting_tab_id, serde_json::json!({
                    "tabs": tabs_json,
                }));
                Task::none()
            }
            Message::AutoCheckpoint(tab_id) => {
                if self.config.auto_checkpoint {
                    self.kick_checkpoint(tab_id, CheckpointReason::Auto)
                } else {
                    Task::none()
                }
            }
            Message::ToggleTimeline(tab_id) => {
                let mut opened = false;
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                    tab.timeline_visible = !tab.timeline_visible;
                    if !tab.timeline_visible {
                        tab.timeline_cursor = None;
                    } else {
                        opened = true;
                    }
                }
                if !opened {
                    self.resize_tab_for_timeline(tab_id);
                    return Task::none();
                }
                // Snapshot the uncheckpointed tail on open so the tip
                // represents "now". dup-skip no-ops when nothing's
                // grown past the tip. When a snapshot is pending, its
                // completion handler scrolls; otherwise we scroll now.
                let need_ckpt = self
                    .tabs
                    .iter()
                    .find(|t| t.id == tab_id)
                    .and_then(|tab| {
                        self.ckpt_store.head_of(&tab.uuid).map(|tip| {
                            crate::widget::timeline::has_uncheckpointed_tail(
                                &self.ckpt_store,
                                tab,
                                tip,
                            )
                        })
                    })
                    .unwrap_or(false);
                let ckpt_task = if need_ckpt {
                    self.kick_checkpoint(tab_id, CheckpointReason::TimelineOpen)
                } else {
                    Task::none()
                };
                self.resize_tab_for_timeline(tab_id);
                let scroll_task = if need_ckpt {
                    Task::none()
                } else if let Some(tab) = self.tabs.iter().find(|t| t.id == tab_id) {
                    crate::widget::timeline::scroll_to_cursor(
                        &self.ckpt_store,
                        tab,
                        &self.config,
                    )
                } else {
                    Task::none()
                };
                Task::batch([ckpt_task, scroll_task])
            }
            Message::CheckpointDone { tab_id, reason, result } => {
                self.finish_checkpoint(tab_id, reason, *result)
            }
            Message::ForkDone { source_tab_id, action, result } => {
                self.finish_fork(source_tab_id, action, *result)
            }
            Message::BackgroundDone => Task::none(),
            Message::TimelineScrub(tab_id, dir) => {
                let next = self
                    .tabs
                    .iter()
                    .find(|t| t.id == tab_id)
                    .and_then(|tab| crate::widget::timeline::move_cursor(&self.ckpt_store, tab, dir));
                if let Some(id) = next {
                    if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                        tab.timeline_cursor = Some(id);
                        tab.redo_path.clear();
                    }
                    if let Some(tab) = self.tabs.iter().find(|t| t.id == tab_id) {
                        return crate::widget::timeline::scroll_to_cursor(
                            &self.ckpt_store,
                            tab,
                            &self.config,
                        );
                    }
                }
                Task::none()
            }
            Message::TimelineActivate(tab_id, mode) => {
                let Some(tab) = self.tabs.iter().find(|t| t.id == tab_id) else {
                    return Task::none();
                };
                let tip = self.ckpt_store.head_of(&tab.uuid).cloned();
                let Some(ckpt_id) =
                    crate::widget::timeline::effective_cursor(tab, tip.as_deref())
                else {
                    return Task::none();
                };
                match mode {
                    TimelineMode::Replace => self.handle_replace(tab_id, ckpt_id),
                    TimelineMode::Fork => self.handle_fork(tab_id, ckpt_id, None),
                }
            }
            Message::Undo(tab_id) => self.handle_undo(tab_id),
            Message::Redo(tab_id) => self.handle_redo(tab_id),
            Message::ShowToast { source_tab_id, message, prompt, target_tab_id } => {
                let id = self.next_toast_id;
                self.next_toast_id += 1;
                self.toasts.push(Toast {
                    id,
                    source_tab_id,
                    message,
                    prompt,
                    target_tab_id,
                });
                toast::schedule_dismiss(id)
            }
            Message::FocusFromToast(toast_id) => {
                let Some(idx) = self.toasts.iter().position(|t| t.id == toast_id) else {
                    return Task::none();
                };
                let toast = self.toasts.remove(idx);
                let Some(target) = toast.target_tab_id else {
                    return Task::none();
                };
                if self.tabs.iter().any(|t| t.id == target) {
                    self.focus_tab(target);
                }
                Task::none()
            }
            Message::DismissToast(toast_id) => {
                self.toasts.retain(|t| t.id != toast_id);
                Task::none()
            }
            Message::SpawnFromToast(toast_id) => {
                let Some(idx) = self.toasts.iter().position(|t| t.id == toast_id) else {
                    return Task::none();
                };
                let toast = self.toasts.remove(idx);
                let Some(prompt) = toast.prompt else {
                    return Task::none();
                };
                let Some(source) = self.tabs.iter().find(|t| t.id == toast.source_tab_id) else {
                    return Task::none();
                };
                let (rank, project_dir, parent_id) = match source.rank {
                    AgentRank::Home => return Task::none(),
                    AgentRank::Project => (AgentRank::Task, source.project_dir.clone(), Some(source.id)),
                    AgentRank::Task => {
                        let dir = self.project_dir_for_tab(source.id);
                        (AgentRank::Task, dir, Some(source.id))
                    }
                };
                let (new_tab_id, task) = self.spawn_tab(
                    true, rank, project_dir, parent_id, Some(prompt),
                    None, None, None,
                );
                self.focus_tab(new_tab_id);
                task
            }
        }
    }

    /// Resize a tab's PTY/term to account for its current timeline
    /// visibility. Called when the timeline is toggled so the agent's
    /// bottom rows don't end up hidden behind the strip.
    fn resize_tab_for_timeline(&mut self, tab_id: usize) {
        let Some(size) = self.window_size else {
            return;
        };
        let cw = self.config.char_width();
        let ch = self.config.char_height();
        let reserved = {
            let Some(tab) = self.tabs.iter().find(|t| t.id == tab_id) else {
                return;
            };
            crate::widget::timeline::pixel_height(&self.ckpt_store, tab, &self.config)
        };
        let (rows, cols) = terminal_size_with_reserved(size, cw, ch, reserved);
        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) else {
            return;
        };
        tab.resize(rows, cols, size.width as u16, size.height as u16);
        tab.set_window_size(alacritty_terminal::event::WindowSize {
            num_lines: rows as u16,
            num_cols: cols as u16,
            cell_width: cw as u16,
            cell_height: ch as u16,
        });
    }

    fn kick_checkpoint(
        &mut self,
        tab_id: usize,
        reason: CheckpointReason,
    ) -> Task<Message> {
        let prep = match self.prepare_checkpoint(tab_id) {
            Ok(p) => p,
            Err(e) => {
                if matches!(reason, CheckpointReason::Mcp) {
                    self.respond_to_tab(
                        tab_id,
                        serde_json::json!({"error": e.to_string()}),
                    );
                }
                return Task::none();
            }
        };
        spawn_blocking_task(
            move || checkpoint::run_checkpoint_blocking(prep),
            move |result| Message::CheckpointDone {
                tab_id,
                reason,
                result: Box::new(result),
            },
        )
    }

    fn prepare_checkpoint(
        &self,
        tab_id: usize,
    ) -> Result<checkpoint::CheckpointPrep, checkpoint::TimeTravelError> {
        use checkpoint::TimeTravelError as E;
        let tab = self
            .tabs
            .iter()
            .find(|t| t.id == tab_id)
            .ok_or(E::UnknownTab(tab_id))?;
        if tab.rank == AgentRank::Home {
            return Err(E::NotSupportedForRank(tab.rank));
        }
        let wt = tab.worktree_dir.clone().ok_or(E::NoWorktree)?;
        let session_id = tab.session_id.clone().ok_or(E::NoSessionId)?;
        let tab_uuid = tab.uuid.clone();
        let title = tab.title.clone();

        let parent_id = self.ckpt_store.head_of(&tab_uuid).cloned();
        let parent_commit = parent_id
            .as_deref()
            .and_then(|pid| self.ckpt_store.node(pid))
            .map(|n| n.shadow_commit.clone());
        let parent_line_count = parent_id
            .as_deref()
            .and_then(|pid| self.ckpt_store.node(pid))
            .map(|n| n.jsonl_line_count);
        // Root-on-demand: the setup script runs in the PTY after
        // TabReady, so the first checkpoint also synthesizes a root.
        let needs_root = parent_id.is_none();

        Ok(checkpoint::CheckpointPrep {
            wt,
            session_id,
            title,
            parent_id,
            parent_commit,
            parent_line_count,
            needs_root,
        })
    }

    fn finish_checkpoint(
        &mut self,
        tab_id: usize,
        reason: CheckpointReason,
        result: Result<checkpoint::CheckpointOutcome, String>,
    ) -> Task<Message> {
        let outcome = match result {
            Ok(o) => o,
            Err(e) => {
                if matches!(reason, CheckpointReason::Mcp) {
                    self.respond_to_tab(
                        tab_id,
                        serde_json::json!({"error": e}),
                    );
                }
                return Task::none();
            }
        };

        let tab_uuid = match self.tabs.iter().find(|t| t.id == tab_id) {
            Some(t) => t.uuid.clone(),
            None => return Task::none(),
        };

        if let Some(root) = outcome.root {
            let root_id = root.id.clone();
            self.ckpt_store.insert_node(root);
            self.ckpt_store.set_head(&tab_uuid, root_id);
        }

        let response = match outcome.new_node {
            None => serde_json::json!({
                "skipped": "duplicate_of_parent",
                "parent_id": outcome.parent_id,
                "jsonl_line_count": outcome.line_count,
            }),
            Some(node) => {
                let new_id = node.id.clone();
                let shadow_commit = node.shadow_commit.clone();
                let line_count = node.jsonl_line_count;
                self.ckpt_store.insert_node(node);
                self.ckpt_store.set_head(&tab_uuid, new_id.clone());
                if let Some(t) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                    t.redo_path.clear();
                }
                let extra_protected: HashSet<String> = self
                    .tabs
                    .iter()
                    .flat_map(|t| t.redo_path.iter().cloned())
                    .collect();
                self.ckpt_store.prune_tree(&new_id, &extra_protected);
                serde_json::json!({
                    "checkpoint_id": new_id,
                    "shadow_commit": shadow_commit,
                    "jsonl_line_count": line_count,
                })
            }
        };

        let save_task = match self.ckpt_store.head_of(&tab_uuid).cloned() {
            Some(head) => self.schedule_save_tree_at(&head),
            None => Task::none(),
        };

        if let CheckpointReason::Mcp = reason {
            self.respond_to_tab(tab_id, response);
        }
        let scroll_task = if matches!(reason, CheckpointReason::TimelineOpen)
            && let Some(tab) = self.tabs.iter().find(|t| t.id == tab_id)
        {
            crate::widget::timeline::scroll_to_cursor(
                &self.ckpt_store,
                tab,
                &self.config,
            )
        } else {
            Task::none()
        };
        Task::batch([save_task, scroll_task])
    }

    /// Snapshot the checkpoint store and persist the tree containing
    /// `any_id` off the UI thread.
    fn schedule_save_tree_at(&self, any_id: &str) -> Task<Message> {
        let store = self.ckpt_store.clone();
        let any_id = any_id.to_string();
        spawn_blocking_discard(move || {
            let _ = crate::checkpoint_store::save_tree(&store, &any_id);
        })
    }

    fn handle_undo(&mut self, tab_id: usize) -> Task<Message> {
        let Some(tab) = self.tabs.iter().find(|t| t.id == tab_id) else {
            return Task::none();
        };
        let tab_uuid = tab.uuid.clone();
        let Some(head) = self.ckpt_store.head_of(&tab_uuid).cloned() else {
            return Task::none();
        };
        let Some(parent) = self
            .ckpt_store
            .node(&head)
            .and_then(|n| n.parent.clone())
        else {
            return Task::none();
        };
        let mut new_redo = tab.redo_path.clone();
        new_redo.push(head);
        let max = crate::checkpoint_store::REDO_PATH_MAX;
        if new_redo.len() > max {
            new_redo.drain(..new_redo.len() - max);
        }
        self.kick_fork(
            tab_id,
            parent,
            ForkAction::Replace { new_redo },
        )
    }

    fn handle_redo(&mut self, tab_id: usize) -> Task<Message> {
        let Some(tab) = self.tabs.iter().find(|t| t.id == tab_id) else {
            return Task::none();
        };
        let mut new_redo = tab.redo_path.clone();
        let Some(target) = new_redo.pop() else {
            return Task::none();
        };
        if !self.ckpt_store.nodes.contains_key(&target) {
            return Task::none();
        }
        self.kick_fork(
            tab_id,
            target,
            ForkAction::Replace { new_redo },
        )
    }

    fn handle_replace(&mut self, tab_id: usize, ckpt_id: String) -> Task<Message> {
        if let Some(t) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
            t.redo_path.clear();
        }
        self.kick_fork(
            tab_id,
            ckpt_id,
            ForkAction::Replace { new_redo: Vec::new() },
        )
    }

    fn handle_fork(
        &mut self,
        tab_id: usize,
        ckpt_id: String,
        prompt: Option<String>,
    ) -> Task<Message> {
        if let Some(t) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
            t.redo_path.clear();
        }
        self.kick_fork(
            tab_id,
            ckpt_id,
            ForkAction::Fork { prompt },
        )
    }

    fn kick_fork(
        &mut self,
        tab_id: usize,
        ckpt_id: String,
        action: ForkAction,
    ) -> Task<Message> {
        let prep = match self.prepare_fork(tab_id, ckpt_id) {
            Ok(p) => p,
            Err(e) => {
                self.respond_to_tab(
                    tab_id,
                    serde_json::json!({"error": e.to_string()}),
                );
                return Task::none();
            }
        };
        spawn_blocking_task(
            move || checkpoint::run_fork_blocking(prep),
            move |result| Message::ForkDone {
                source_tab_id: tab_id,
                action,
                result: Box::new(result),
            },
        )
    }

    fn prepare_fork(
        &self,
        tab_id: usize,
        ckpt_id: String,
    ) -> Result<checkpoint::ForkPrep, checkpoint::TimeTravelError> {
        use checkpoint::TimeTravelError as E;
        let tab = self
            .tabs
            .iter()
            .find(|t| t.id == tab_id)
            .ok_or(E::UnknownTab(tab_id))?;
        let ckpt = self
            .ckpt_store
            .node(&ckpt_id)
            .cloned()
            .ok_or_else(|| E::CheckpointNotFound(ckpt_id.clone()))?;
        let project_dir = tab.project_dir.clone().ok_or(E::NoProjectDir)?;

        // Random suffix keeps the branch (and its derived worktree
        // path) unique when the same (tab, checkpoint) is forked more
        // than once — otherwise `git worktree add -b` rejects it.
        let suffix = &Uuid::new_v4().simple().to_string()[..6];
        let new_branch = format!(
            "fork-t{tab_id}-{}-{}",
            &ckpt.shadow_commit[..8],
            suffix,
        );
        let wt_path = crate::worktree::worktree_path(
            &project_dir,
            &self.config.worktree_location,
            &new_branch,
        );

        Ok(checkpoint::ForkPrep {
            project_dir,
            ckpt_id,
            ckpt_title: ckpt.title.clone(),
            ckpt_shadow_commit: ckpt.shadow_commit,
            ckpt_session_id: ckpt.session_id,
            ckpt_jsonl_line_count: ckpt.jsonl_line_count,
            src_worktree: ckpt.worktree_dir,
            new_branch,
            wt_path,
        })
    }

    fn finish_fork(
        &mut self,
        source_tab_id: usize,
        action: ForkAction,
        result: Result<checkpoint::ForkOutcome, String>,
    ) -> Task<Message> {
        let outcome = match result {
            Ok(o) => o,
            Err(e) => {
                self.respond_to_tab(
                    source_tab_id,
                    serde_json::json!({"error": e}),
                );
                return Task::none();
            }
        };

        // Tab list may have reordered while we were blocking, and the
        // source tab may be gone entirely.
        let Some((idx, rank, project_dir)) = self
            .tabs
            .iter()
            .enumerate()
            .find(|(_, t)| t.id == source_tab_id)
            .and_then(|(i, t)| {
                t.project_dir.clone().map(|d| (i, t.rank, d))
            })
        else {
            return Task::none();
        };

        let ckpt_title = outcome.ckpt_title.clone();

        let (prompt, keep_source, new_redo) = match action {
            ForkAction::Fork { prompt } => (prompt, true, Vec::new()),
            ForkAction::Replace { new_redo } => (None, false, new_redo),
        };

        let (new_tab_id, spawn_task) = self.spawn_tab_full(
            true,
            rank,
            Some(project_dir),
            Some(source_tab_id),
            prompt,
            Some(outcome.new_branch.clone()),
            None,
            None,
            outcome.resume_session_id.clone(),
            Some(outcome.wt_path.clone()),
            Some(idx + 1),
        );

        if let Some(new_tab) = self.tabs.iter_mut().find(|t| t.id == new_tab_id) {
            new_tab.worktree_dir = Some(outcome.wt_path.clone());
            if let Some(title) = ckpt_title {
                new_tab.title = Some(title);
            }
            new_tab.redo_path = new_redo;
            let new_tab_uuid = new_tab.uuid.clone();
            self.ckpt_store
                .set_head(&new_tab_uuid, outcome.ckpt_id.clone());
        }

        let save_task = self.schedule_save_tree_at(&outcome.ckpt_id);

        self.focus_tab(new_tab_id);
        self.respond_to_tab(
            source_tab_id,
            serde_json::json!({
                "new_tab_id": new_tab_id,
                "worktree": outcome.wt_path.to_string_lossy(),
            }),
        );

        let close_task = if keep_source {
            Task::none()
        } else {
            self.close_tab(source_tab_id)
        };

        Task::batch([spawn_task, save_task, close_task])
    }

    pub fn view(&self) -> Element<'_, Message> {
        let active_bg = self.terminal_theme.bg;

        let display_order = self.tab_display_order();
        let number_assignments = self.tab_number_assignments();
        let toast_elements: Vec<Element<'_, Message>> = self
            .toasts
            .iter()
            .map(|t| crate::widget::toast::view(t, &self.config))
            .collect();

        let tab_bar = crate::widget::tab_bar::TabBar {
            tabs: &self.tabs,
            active_tab_id: self.active_tab_id,
            display_order: &display_order,
            number_assignments: &number_assignments,
            bell_flashes: &self.bell_flashes,
            folded_tabs: &self.folded_tabs,
            terminal_theme: &self.terminal_theme,
            config: &self.config,
        }
        .view(toast_elements);

        let (term_element, timeline_element): (Element<'_, Message>, Option<Element<'_, Message>>) =
            if let Some(tab) = self.active_tab() {
                let term: Element<'_, Message> = TerminalWidget::new(tab, &self.config).into();
                if tab.timeline_visible {
                    let timeline = crate::widget::timeline::view(
                        &self.ckpt_store,
                        tab,
                        &self.terminal_theme,
                        &self.config,
                    );
                    (term, Some(timeline))
                } else {
                    (term, None)
                }
            } else {
                (Space::new().width(Fill).height(Fill).into(), None)
            };

        let terminal_pane = container(term_element)
            .padding(PADDING)
            .width(Fill)
            .height(Fill)
            .style(move |_theme| container::Style {
                background: Some(active_bg.into()),
                ..Default::default()
            });

        let right_side: Element<'_, Message> = if let Some(timeline) = timeline_element {
            column![terminal_pane, timeline].into()
        } else {
            terminal_pane.into()
        };

        row![tab_bar, right_side]
            .width(Fill)
            .height(Fill)
            .into()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        iced::window::resize_events().map(|(_, size)| Message::WindowResized(size))
    }

    pub fn theme(&self) -> Theme {
        if self.terminal_theme.is_dark {
            Theme::Dark
        } else {
            Theme::Light
        }
    }

}

impl Drop for App {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.parent_socket_dir);
    }
}

/// Stream that accepts connections on the parent socket and reads messages
/// from all connected MCP server instances.
fn parent_socket_stream(
    listener: unix::UnixListener,
    response_writers: ResponseWriters,
) -> impl iced::futures::Stream<Item = Message> {
    iced::stream::channel(
        64,
        |sender: iced::futures::channel::mpsc::Sender<Message>| async move {
            let (exit_sender, exit_receiver) = iced::futures::channel::oneshot::channel::<()>();

            std::thread::spawn(move || {
                // Accept connections in a loop — one per MCP server instance.
                for stream in listener.incoming() {
                    let Ok(stream) = stream else { break };
                    let mut sender = sender.clone();
                    let response_writers = Arc::clone(&response_writers);

                    std::thread::spawn(move || {
                        let writer = stream.try_clone().expect("failed to clone stream");
                        let reader = std::io::BufReader::new(stream);
                        for line in reader.lines() {
                            let Ok(line) = line else { break };
                            let Ok(msg) = serde_json::from_str::<serde_json::Value>(&line) else {
                                continue;
                            };
                            let tab_id = msg
                                .get("tab_id")
                                .and_then(|v| v.as_str())
                                .and_then(|s| s.parse::<usize>().ok())
                                .unwrap_or(0);
                            let msg_type = msg
                                .get("type")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            let message = match msg_type {
                                "set_title" => {
                                    let title = msg
                                        .get("title")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("")
                                        .to_string();
                                    Some(Message::SetTitle(tab_id, title))
                                }
                                "spawn_tab" => {
                                    let wd = msg
                                        .get("working_directory")
                                        .and_then(|v| v.as_str())
                                        .map(PathBuf::from);
                                    let project_tab_id = msg
                                        .get("project_tab_id")
                                        .and_then(|v| v.as_u64())
                                        .map(|v| v as usize);
                                    let prompt = msg
                                        .get("prompt")
                                        .and_then(|v| v.as_str())
                                        .map(String::from);
                                    let branch = msg
                                        .get("branch")
                                        .and_then(|v| v.as_str())
                                        .map(String::from);
                                    let model = msg
                                        .get("model")
                                        .and_then(|v| v.as_str())
                                        .map(String::from);
                                    let base = msg
                                        .get("base")
                                        .and_then(|v| v.as_str())
                                        .map(String::from);
                                    let resp_writer = writer.try_clone()
                                        .expect("failed to clone writer for response");
                                    response_writers.lock().unwrap()
                                        .insert(tab_id, resp_writer);
                                    Some(Message::McpSpawnAgent(tab_id, wd, project_tab_id, prompt, branch, model, base))
                                }
                                "close_tab" => {
                                    let target_tab_id = msg
                                        .get("target_tab_id")
                                        .and_then(|v| v.as_u64())
                                        .map(|v| v as usize)
                                        .unwrap_or(0);
                                    let resp_writer = writer.try_clone()
                                        .expect("failed to clone writer for response");
                                    response_writers.lock().unwrap()
                                        .insert(tab_id, resp_writer);
                                    Some(Message::McpCloseTab(tab_id, target_tab_id))
                                }
                                "set_status" => {
                                    msg.get("status")
                                        .and_then(|v| v.as_str())
                                        .and_then(AgentStatus::from_str)
                                        .map(|s| Message::SetStatus(tab_id, s))
                                }
                                "set_pr" => {
                                    // Missing field or an explicit null/0
                                    // clears the override.
                                    let pr = msg
                                        .get("pr")
                                        .and_then(|v| v.as_u64())
                                        .and_then(|n| u32::try_from(n).ok())
                                        .filter(|n| *n > 0);
                                    Some(Message::SetPr(tab_id, pr))
                                }
                                "checkpoint" => {
                                    let resp_writer = writer.try_clone()
                                        .expect("failed to clone writer for response");
                                    response_writers.lock().unwrap()
                                        .insert(tab_id, resp_writer);
                                    Some(Message::McpCheckpoint(tab_id))
                                }
                                "replace" => {
                                    let ckpt_id = msg.get("checkpoint_id")
                                        .and_then(|v| v.as_str())
                                        .map(String::from)
                                        .unwrap_or_default();
                                    let resp_writer = writer.try_clone()
                                        .expect("failed to clone writer for response");
                                    response_writers.lock().unwrap()
                                        .insert(tab_id, resp_writer);
                                    Some(Message::McpReplace(tab_id, ckpt_id))
                                }
                                "fork" => {
                                    let ckpt_id = msg.get("checkpoint_id")
                                        .and_then(|v| v.as_str())
                                        .map(String::from)
                                        .unwrap_or_default();
                                    let prompt = msg.get("prompt")
                                        .and_then(|v| v.as_str())
                                        .map(String::from);
                                    let resp_writer = writer.try_clone()
                                        .expect("failed to clone writer for response");
                                    response_writers.lock().unwrap()
                                        .insert(tab_id, resp_writer);
                                    Some(Message::McpFork(tab_id, ckpt_id, prompt))
                                }
                                "list_tabs" => {
                                    let resp_writer = writer.try_clone()
                                        .expect("failed to clone writer for response");
                                    response_writers.lock().unwrap()
                                        .insert(tab_id, resp_writer);
                                    Some(Message::McpListTabs(tab_id))
                                }
                                "notify" => {
                                    let message_text = msg
                                        .get("message")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("")
                                        .to_string();
                                    let prompt = msg
                                        .get("prompt")
                                        .and_then(|v| v.as_str())
                                        .map(String::from);
                                    let target_tab_id = msg
                                        .get("target_tab_id")
                                        .and_then(|v| v.as_str())
                                        .and_then(|s| s.parse::<usize>().ok());
                                    Some(Message::ShowToast {
                                        source_tab_id: tab_id,
                                        message: message_text,
                                        prompt,
                                        target_tab_id,
                                    })
                                }
                                _ => None,
                            };
                            if let Some(message) = message {
                                if futures::executor::block_on(sender.send(message)).is_err() {
                                    break;
                                }
                            }
                        }
                    });
                }
                let _ = exit_sender.send(());
            });

            let _ = exit_receiver.await;
        },
    )
}


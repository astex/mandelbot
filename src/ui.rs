use std::collections::{HashMap, HashSet};
use std::io::{BufRead, Write};
use std::os::unix::net as unix;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use futures::SinkExt;

use iced::widget::{button, column, container, row, text, Space};
use iced::{Alignment, Border, Color, Element, Fill, Font, Size, Subscription, Task, Theme};

use alacritty_terminal::index::{Point as GridPoint, Side};
use alacritty_terminal::selection::Selection;

use crate::animation::FlashState;
use crate::config::Config;
use crate::terminal::{AgentRank, AgentStatus, TerminalTab};
use crate::theme::TerminalTheme;
use crate::widget::fold_placeholder::FoldPlaceholderWidget;
use crate::widget::plan_review::PlanReviewWidget;
use crate::widget::terminal::{self, TerminalWidget};

const PADDING: f32 = 4.0;
const TAB_BAR_WIDTH: f32 = 400.0;
const TAB_GROUP_GAP: f32 = 28.0;
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

#[derive(Clone)]
pub enum DisplayEntry {
    Tab(usize),
    Fold { parent_id: usize, count: usize, depth: usize },
}

#[derive(Debug, Clone)]
pub enum Message {
    TerminalOutput(usize, Vec<u8>),
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
    McpSpawnAgent(usize, Option<PathBuf>, Option<usize>, Option<String>, Option<String>),
    SetTitle(usize, String),
    SetPlan(usize, PathBuf),
    TogglePlanView(usize),
    SetStatus(usize, AgentStatus),
    SetSelection(Option<Selection>),
    UpdateSelection(GridPoint, Side),
    Bell(usize),
    BellTick,
    FoldTab,
    UnfoldTab(usize),
    ClipboardLoadResult(usize, Option<String>),
}

fn terminal_size(window: Size, char_width: f32, char_height: f32) -> (usize, usize) {
    let cols = ((window.width - PADDING * 2.0 - TAB_BAR_WIDTH - terminal::SCROLLBAR_WIDTH) / char_width).floor() as usize;
    let rows = ((window.height - PADDING * 2.0) / char_height).floor() as usize;
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
    active_fold: Option<usize>,
}

impl App {
    pub fn boot() -> (Self, Task<Message>) {
        let config = Config::load();
        let terminal_theme = config.terminal_theme();

        let parent_socket_dir = crate::terminal::runtime_dir();
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
            active_fold: None,
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
        self.active_fold = None;
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
        let inherited_plan = parent.and_then(|p| p.plan_path.clone());
        let project_id = match rank {
            AgentRank::Home => None,
            AgentRank::Project => Some(id),
            AgentRank::Task => parent.and_then(|p| p.project_id),
        };
        let model = match rank {
            AgentRank::Home => &self.config.models.home,
            AgentRank::Project => &self.config.models.project,
            AgentRank::Task => &self.config.models.task,
        };
        let (tab, pty_task) = TerminalTab::spawn(
            id, rows, cols, is_claude, rank, project_dir, parent_id,
            depth, project_id,
            &self.config.shell, &self.config.workflow, &self.config.worktree_location,
            model, &self.parent_socket_path, prompt, branch,
        );
        self.tabs.push(tab);
        if let Some(plan) = inherited_plan
            && let Some(tab) = self.tabs.last_mut()
        {
            tab.plan_path = Some(plan);
        }
        // Initialize colors and window size for OSC query responses.
        if let Some(tab) = self.tabs.last() {
            tab.set_colors(
                self.terminal_theme.fg,
                self.terminal_theme.bg,
                self.terminal_theme.fg, // cursor color falls back to fg
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
        let fifo_path = crate::terminal::runtime_dir().join(format!("{id}.fifo"));
        let fifo_task = Task::run(
            crate::terminal::fifo_stream(id, fifo_path),
            |msg| msg,
        );
        (id, Task::batch([pty_task, fifo_task]))
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

    /// Returns display entries in tree order: Home, then projects with their
    /// tasks nested underneath (recursively), then shell tabs. Folded subtrees
    /// are replaced with a single `DisplayEntry::Fold` placeholder.
    fn tab_display_order(&self) -> Vec<DisplayEntry> {
        let mut order = Vec::new();

        // Home agent first, then its descendants depth-first.
        if let Some(home) = self.tabs.iter().find(|t| t.rank == AgentRank::Home) {
            order.push(DisplayEntry::Tab(home.id));
            self.collect_children(home.id, &mut order);
        }

        // Shell tabs at the end.
        for tab in self.tabs.iter().filter(|t| !t.is_claude) {
            order.push(DisplayEntry::Tab(tab.id));
        }

        order
    }

    fn collect_children(&self, parent_id: usize, order: &mut Vec<DisplayEntry>) {
        for tab in self.tabs.iter().filter(|t| t.parent_id == Some(parent_id) && t.is_claude) {
            order.push(DisplayEntry::Tab(tab.id));
            if self.folded_tabs.contains(&tab.id) {
                let count = self.count_descendants(tab.id);
                if count > 0 {
                    order.push(DisplayEntry::Fold {
                        parent_id: tab.id,
                        count,
                        depth: tab.depth + 1,
                    });
                }
            } else {
                self.collect_children(tab.id, order);
            }
        }
    }

    fn count_descendants(&self, parent_id: usize) -> usize {
        let mut count = 0;
        for tab in self.tabs.iter().filter(|t| t.parent_id == Some(parent_id) && t.is_claude) {
            count += 1;
            count += self.count_descendants(tab.id);
        }
        count
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
        self.active_fold = None;
    }

    fn select_display_entry(&mut self, entry: &DisplayEntry) {
        match entry {
            DisplayEntry::Tab(id) => self.focus_tab(*id),
            DisplayEntry::Fold { parent_id, .. } => {
                self.active_fold = Some(*parent_id);
            }
        }
    }

    fn current_display_index(&self, order: &[DisplayEntry]) -> Option<usize> {
        if let Some(fold_parent) = self.active_fold {
            order.iter().position(|e| matches!(e, DisplayEntry::Fold { parent_id, .. } if *parent_id == fold_parent))
        } else {
            order.iter().position(|e| matches!(e, DisplayEntry::Tab(id) if *id == self.active_tab_id))
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

    fn close_tab(&mut self, tab_id: usize) -> Task<Message> {
        self.folded_tabs.remove(&tab_id);

        let Some(idx) = self.tabs.iter().position(|t| t.id == tab_id) else {
            return Task::none();
        };

        // Clean up git worktree if this tab created one.
        if let Some(wt) = self.tabs[idx].worktree_path.take() {
            if let Some(dir) = self.tabs[idx].project_dir.as_deref() {
                crate::worktree::remove(dir, &wt);
            }
        }

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
            let new_idx = idx.min(self.tabs.len() - 1);
            self.focus_tab(self.tabs[new_idx].id);
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
                let (id, task) = self.spawn_tab(true, AgentRank::Home, Some(home), None, first_run_prompt, None);
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
                let (rows, cols) = terminal_size(size, cw, ch);
                for tab in &mut self.tabs {
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
            Message::TerminalOutput(tab_id, bytes) => {
                let mut tasks: Vec<Task<Message>> = Vec::new();
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                    tab.feed(&bytes);
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
                Some(0) | None => {
                    // TODO: None means child.wait() failed, which
                    // shouldn't happen since we own the Child. Log
                    // this once we have proper error monitoring.
                    self.close_tab(tab_id)
                }
                Some(code) => {
                    if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                        let hint = format!(
                            "\r\n[process exited with code {}; {} + w to close tab]\r\n",
                            code, self.config.control_prefix,
                        );
                        tab.feed(hint.as_bytes());
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
            Message::McpSpawnAgent(requesting_tab_id, working_directory, project_tab_id, prompt, branch) => {
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

                let (new_tab_id, task) = self.spawn_tab(true, rank, project_dir, parent_id, prompt, branch);
                self.respond_to_tab(requesting_tab_id, serde_json::json!({"tab_id": new_tab_id}));
                task
            }
            Message::SetPlan(tab_id, path) => {
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                    tab.plan_path = Some(path);
                    tab.plan_contents = None;
                }
                Task::none()
            }
            Message::TogglePlanView(tab_id) => {
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                    tab.plan_visible = !tab.plan_visible;
                    if tab.plan_visible {
                        tab.plan_contents = tab
                            .plan_path
                            .as_ref()
                            .and_then(|p| std::fs::read_to_string(p).ok());
                    }
                }
                Task::none()
            }
            Message::SetStatus(tab_id, status) => {
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                    tab.status = status;
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
                let (id, task) = self.spawn_tab(false, AgentRank::Home, None, None, None, None);
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
                            let (id, task) = self.spawn_tab(true, AgentRank::Task, Some(dir), Some(pid), None, None);
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
                            let (id, task) = self.spawn_tab(true, AgentRank::Task, Some(dir), Some(self.active_tab_id), None, None);
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
                if let Some(idx) = self.current_display_index(&order) {
                    let new_idx = (idx as i32 + delta)
                        .rem_euclid(order.len() as i32) as usize;
                    let entry = order[new_idx].clone();
                    self.select_display_entry(&entry);
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
                let cur = self.current_display_index(&order).unwrap_or(0);

                // Search order rotated to start after current tab (only real tabs).
                let candidates: Vec<usize> = order.iter()
                    .cycle()
                    .skip(cur + 1)
                    .take(order.len())
                    .filter_map(|e| match e {
                        DisplayEntry::Tab(id) => Some(*id),
                        DisplayEntry::Fold { .. } => None,
                    })
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
                        );
                        self.focus_tab(id);
                        task
                    }
                }
            }
            Message::CloseTab(tab_id) => self.close_tab(tab_id),
            Message::SelectTab(tab_id) => {
                if self.tabs.iter().any(|t| t.id == tab_id) {
                    self.focus_tab(tab_id);
                }
                Task::none()
            }
            Message::SelectTabByIndex(index) => {
                let display_order = self.tab_display_order();
                // Tab numbers skip fold entries, so find the Nth real tab.
                let entry = display_order.iter()
                    .filter(|e| matches!(e, DisplayEntry::Tab(_)))
                    .nth(index)
                    .cloned();
                if let Some(entry) = entry {
                    self.select_display_entry(&entry);
                }
                Task::none()
            }
            Message::FoldTab => {
                let tab_id = self.active_tab_id;
                if self.tabs.iter().any(|t| t.parent_id == Some(tab_id) && t.is_claude) {
                    self.folded_tabs.insert(tab_id);
                }
                Task::none()
            }
            Message::UnfoldTab(parent_id) => {
                self.folded_tabs.remove(&parent_id);
                self.active_fold = None;
                // Focus the first child of the unfolded tab.
                if let Some(first_child) = self.tabs.iter()
                    .find(|t| t.parent_id == Some(parent_id) && t.is_claude)
                    .map(|t| t.id)
                {
                    self.focus_tab(first_child);
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
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let active_bg = self.terminal_theme.bg;
        let inactive_bg = self.terminal_theme.black;
        let fg = self.terminal_theme.fg;

        let mut tab_col = column![];
        let display_order = self.tab_display_order();

        let active_fold = self.active_fold;
        let tab_button = |tab: &TerminalTab, display_index: usize, active_tab_id: usize, indent: f32| {
            let is_active = tab.id == active_tab_id && active_fold.is_none();
            let tab_id = tab.id;

            let base_bg = if is_active { active_bg } else { inactive_bg };
            let bg = self.bell_flashes.blend(tab_id, base_bg, self.terminal_theme.yellow);

            let label_text: String = if tab.is_pending() {
                "new project...".into()
            } else if let Some(title) = &tab.title {
                if !tab.is_claude {
                    let cw = self.config.char_width();
                    let avail = TAB_BAR_WIDTH - indent - PADDING * 2.0 - cw * 3.0;
                    let max_chars = (avail / cw) as usize;
                    format_shell_title(title, max_chars)
                } else {
                    title.clone()
                }
            } else if tab.rank == AgentRank::Project {
                if let Some(dir) = &tab.project_dir {
                    dir.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| dir.to_string_lossy().into_owned())
                } else {
                    String::new()
                }
            } else if !tab.is_claude {
                "shell".into()
            } else {
                String::new()
            };

            let number_text = if display_index <= 9 {
                format!("{}", display_index)
            } else {
                " ".into()
            };

            let label = text(label_text)
                .size(self.config.font_size)
                .font(Font::MONOSPACE)
                .color(fg);
            let number = text(number_text)
                .size(self.config.font_size)
                .font(Font::MONOSPACE)
                .color(fg);

            let label = container(label).width(Fill).clip(true);

            let mut suffix = row![].align_y(Alignment::Center);
            if tab.is_claude && tab.background_tasks > 0 {
                let bg_label = format!("+{}", tab.background_tasks);
                suffix = suffix
                    .push(text(bg_label).size(self.config.font_size * 0.75).font(Font::MONOSPACE).color(self.terminal_theme.cyan))
                    .push(Space::new().width(6));
            }
            {
                let dot_size = self.config.font_size * 0.6;
                let dot_char = if tab.status == AgentStatus::Idle { "○" } else { "●" };
                let dot_color = status_dot_color(tab.status, fg);
                suffix = suffix
                    .push(text(dot_char).size(dot_size).color(dot_color))
                    .push(Space::new().width(4));
            }
            let suffix = suffix.push(number);

            let content = row![Space::new().width(PADDING), label, suffix, Space::new().width(PADDING)]
                .align_y(Alignment::Center);

            let has_plan = tab.plan_path.is_some();
            let plan_btn_width = 20.0_f32;
            let main_width = if has_plan {
                TAB_BAR_WIDTH - indent - plan_btn_width
            } else {
                TAB_BAR_WIDTH - indent
            };

            let btn = button(content)
                .on_press(Message::SelectTab(tab_id))
                .width(main_width)
                .style(move |_theme, _status| button::Style {
                    background: Some(bg.into()),
                    border: Border::default(),
                    ..Default::default()
                });

            let main_element: Element<'_, Message> = if has_plan {
                let plan_label = text("P")
                    .size(self.config.font_size)
                    .font(Font::MONOSPACE)
                    .color(fg);
                let plan_btn = button(container(plan_label).center_x(Fill))
                    .on_press(Message::TogglePlanView(tab_id))
                    .width(plan_btn_width)
                    .style(move |_theme, _status| button::Style {
                        background: Some(bg.into()),
                        border: Border::default(),
                        ..Default::default()
                    });
                row![btn, plan_btn].width(TAB_BAR_WIDTH - indent).into()
            } else {
                Element::from(btn)
            };

            if indent > 0.0 {
                row![Space::new().width(indent), main_element].width(TAB_BAR_WIDTH).into()
            } else {
                main_element
            }
        };

        // Determine whether to show group separator lines.
        let has_shells = self.tabs.iter().any(|t| !t.is_claude);
        let has_agents = self.tabs.iter().any(|t| t.is_claude);
        let project_count = self.tabs.iter().filter(|t| t.rank == AgentRank::Project).count();
        let show_separators = (has_agents && has_shells) || project_count > 1;

        let separator = || -> Element<'_, Message> {
            let muted = Color { a: 0.25, ..fg };
            container(Space::new())
                .width(TAB_BAR_WIDTH)
                .height(1)
                .style(move |_theme: &Theme| container::Style {
                    background: Some(muted.into()),
                    ..Default::default()
                })
                .into()
        };

        let indent_step = 20.0_f32;
        let fold_button = |parent_id: usize, count: usize, depth: usize| -> Element<'_, Message> {
            let is_active = active_fold == Some(parent_id);
            let bg = if is_active { active_bg } else { inactive_bg };
            let indent = depth as f32 * indent_step;

            let label_text = format!("... {} tabs ...", count);
            let muted_fg = Color { a: 0.5, ..fg };

            let label = text(label_text)
                .size(self.config.font_size)
                .font(Font::MONOSPACE)
                .color(muted_fg);
            let label = container(label).width(Fill).clip(true);

            let content = row![Space::new().width(PADDING), label, Space::new().width(PADDING)]
                .align_y(Alignment::Center);

            let btn = button(content)
                .on_press(Message::UnfoldTab(parent_id))
                .width(TAB_BAR_WIDTH - indent)
                .style(move |_theme, _status| button::Style {
                    background: Some(bg.into()),
                    border: Border::default(),
                    ..Default::default()
                });

            if indent > 0.0 {
                row![Space::new().width(indent), btn].width(TAB_BAR_WIDTH).into()
            } else {
                Element::from(btn)
            }
        };

        // Build a mapping from display_order index → tab number (skipping folds).
        let mut tab_numbers: Vec<Option<usize>> = Vec::new();
        let mut next_number = 0_usize;
        for entry in &display_order {
            match entry {
                DisplayEntry::Tab(_) => {
                    tab_numbers.push(Some(next_number));
                    next_number += 1;
                }
                DisplayEntry::Fold { .. } => {
                    tab_numbers.push(None);
                }
            }
        }

        // Agent tree: Home → Projects → Tasks.
        tab_col = tab_col.push(Space::new().width(TAB_BAR_WIDTH).height(TAB_GROUP_GAP / 2.0));
        for (i, entry) in display_order.iter().enumerate() {
            match entry {
                DisplayEntry::Tab(tab_id) => {
                    let Some(tab) = self.tabs.iter().find(|t| t.id == *tab_id) else { continue };
                    if !tab.is_claude { continue; }
                    if show_separators && tab.rank == AgentRank::Project {
                        tab_col = tab_col.push(Space::new().width(TAB_BAR_WIDTH).height(TAB_GROUP_GAP / 2.0));
                        tab_col = tab_col.push(separator());
                        tab_col = tab_col.push(Space::new().width(TAB_BAR_WIDTH).height(TAB_GROUP_GAP / 2.0));
                    }
                    let indent = tab.depth as f32 * indent_step;
                    let num = tab_numbers[i].unwrap_or(usize::MAX);
                    tab_col = tab_col.push(tab_button(tab, num, self.active_tab_id, indent));
                }
                DisplayEntry::Fold { parent_id, count, depth } => {
                    tab_col = tab_col.push(fold_button(*parent_id, *count, *depth));
                }
            }
        }

        // Shell tabs (flat).
        let mut first_shell = true;
        for (i, entry) in display_order.iter().enumerate() {
            let DisplayEntry::Tab(tab_id) = entry else { continue };
            let Some(tab) = self.tabs.iter().find(|t| t.id == *tab_id) else { continue };
            if tab.is_claude { continue; }
            if first_shell {
                first_shell = false;
                if show_separators {
                    tab_col = tab_col.push(Space::new().width(TAB_BAR_WIDTH).height(TAB_GROUP_GAP / 2.0));
                    tab_col = tab_col.push(separator());
                    tab_col = tab_col.push(Space::new().width(TAB_BAR_WIDTH).height(TAB_GROUP_GAP / 2.0));
                } else if has_agents {
                    tab_col = tab_col.push(Space::new().width(TAB_BAR_WIDTH).height(TAB_GROUP_GAP));
                }
            }
            let num = tab_numbers[i].unwrap_or(usize::MAX);
            tab_col = tab_col.push(tab_button(tab, num, self.active_tab_id, 0.0));
        }

        let tab_bar = container(tab_col)
            .width(TAB_BAR_WIDTH)
            .height(Fill)
            .style(move |_theme| container::Style {
                background: Some(inactive_bg.into()),
                ..Default::default()
            });

        let terminal_content: Element<'_, Message> = if let Some(fold_parent_id) = self.active_fold {
            let fold_count = display_order.iter()
                .find_map(|e| match e {
                    DisplayEntry::Fold { parent_id, count, .. } if *parent_id == fold_parent_id => Some(*count),
                    _ => None,
                })
                .unwrap_or(0);
            FoldPlaceholderWidget::new(fold_parent_id, fold_count, &self.config).into()
        } else if let Some(tab) = self.active_tab() {
            if tab.plan_visible && tab.plan_path.is_some() {
                let contents = tab.plan_contents.as_deref().unwrap_or("");
                PlanReviewWidget::new(tab.id, contents, &self.config).into()
            } else {
                TerminalWidget::new(tab, &self.config).into()
            }
        } else {
            Space::new().width(Fill).height(Fill).into()
        };

        let terminal_pane = container(terminal_content)
            .padding(PADDING)
            .width(Fill)
            .height(Fill)
            .style(move |_theme| container::Style {
                background: Some(active_bg.into()),
                ..Default::default()
            });

        row![tab_bar, terminal_pane]
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

fn status_dot_color(status: AgentStatus, fg: Color) -> Color {
    match status {
        AgentStatus::Idle => fg,
        AgentStatus::Working => Color::from_rgb8(0x50, 0xc8, 0x50),
        AgentStatus::Blocked => Color::from_rgb8(0xe8, 0xb8, 0x30),
        AgentStatus::NeedsReview => Color::from_rgb8(0x40, 0xa0, 0xe0),
        AgentStatus::Error => Color::from_rgb8(0xe0, 0x40, 0x40),
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
                                    let resp_writer = writer.try_clone()
                                        .expect("failed to clone writer for response");
                                    response_writers.lock().unwrap()
                                        .insert(tab_id, resp_writer);
                                    Some(Message::McpSpawnAgent(tab_id, wd, project_tab_id, prompt, branch))
                                }
                                "set_plan" => {
                                    msg.get("path")
                                        .and_then(|v| v.as_str())
                                        .map(|p| Message::SetPlan(tab_id, PathBuf::from(p)))
                                }
                                "set_status" => {
                                    msg.get("status")
                                        .and_then(|v| v.as_str())
                                        .and_then(AgentStatus::from_str)
                                        .map(|s| Message::SetStatus(tab_id, s))
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

/// Format a shell tab title from a `"<cwd>\t<prompt>\t<command>"` OSC string.
///
/// Walks up the directory chain from longest to shortest until the title fits
/// within `max_chars`, stopping at `~` or `/`. The prompt character varies by
/// shell (`%` for zsh, `$` for bash, `#` for root).
fn format_shell_title(raw: &str, max_chars: usize) -> String {
    if !raw.contains('\u{a0}') {
        return raw.to_string();
    }
    let mut fields = raw.splitn(3, '\u{a0}');
    let cwd = fields.next().unwrap();
    let prompt = fields.next().unwrap_or("$");
    let cmd = fields.next().unwrap_or("");

    // Collect slash positions so we can try progressively shorter prefixes.
    // Walking backwards: full path, then drop one leading component, etc.
    let slash_positions: Vec<usize> = cwd
        .char_indices()
        .filter_map(|(i, c)| if c == '/' { Some(i) } else { None })
        .collect();

    // Candidates from longest to shortest: full cwd, then after each slash.
    let nbsp = '\u{a0}';
    let suffix = if cmd.is_empty() {
        format!("{nbsp}{prompt}{nbsp}")
    } else {
        format!("{nbsp}{prompt}{nbsp}{cmd}")
    };

    // Try the full cwd first.
    let candidate = format!("{cwd}{suffix}");
    if candidate.len() <= max_chars {
        return candidate.trim_end().to_string();
    }

    // Try progressively shorter: drop leading components one at a time.
    for &pos in &slash_positions {
        let dir = &cwd[pos + 1..];
        let candidate = format!("{dir}{suffix}");
        if candidate.len() <= max_chars {
            return candidate.trim_end().to_string();
        }
    }

    // Nothing fits with a directory — just the prompt (+ command).
    if cmd.is_empty() {
        prompt.to_string()
    } else {
        format!("{prompt}{nbsp}{cmd}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_shell_title_idle() {
        // At prompt — show as much dir as fits + prompt char.
        assert_eq!(
            format_shell_title("~/src/mandelbot\u{a0}%\u{a0}", 40),
            "~/src/mandelbot\u{a0}%",
        );
        assert_eq!(
            format_shell_title("~/src/mandelbot\u{a0}%\u{a0}", 18),
            "src/mandelbot\u{a0}%",
        );
        assert_eq!(
            format_shell_title("~/src/mandelbot\u{a0}%\u{a0}", 14),
            "mandelbot\u{a0}%",
        );
        assert_eq!(
            format_shell_title("~\u{a0}%\u{a0}", 40),
            "~\u{a0}%",
        );
    }

    #[test]
    fn format_shell_title_with_command_zsh() {
        // Plenty of room: full path + command.
        assert_eq!(
            format_shell_title("~/src/mandelbot\u{a0}%\u{a0}vim", 40),
            "~/src/mandelbot\u{a0}%\u{a0}vim",
        );
        // Less room: drop leading components.
        assert_eq!(
            format_shell_title("~/src/mandelbot\u{a0}%\u{a0}vim", 22),
            "src/mandelbot\u{a0}%\u{a0}vim",
        );
        assert_eq!(
            format_shell_title("~/src/mandelbot\u{a0}%\u{a0}vim", 18),
            "mandelbot\u{a0}%\u{a0}vim",
        );
        // Very tight: just the command.
        assert_eq!(
            format_shell_title("~/src/mandelbot\u{a0}%\u{a0}vim", 10),
            "%\u{a0}vim",
        );
    }

    #[test]
    fn format_shell_title_with_command_bash() {
        assert_eq!(
            format_shell_title("~/src/mandelbot\u{a0}$\u{a0}vim", 40),
            "~/src/mandelbot\u{a0}$\u{a0}vim",
        );
    }

    #[test]
    fn format_shell_title_root() {
        assert_eq!(
            format_shell_title("/etc/nginx\u{a0}#\u{a0}nginx -t", 40),
            "/etc/nginx\u{a0}#\u{a0}nginx -t",
        );
    }

    #[test]
    fn format_shell_title_home() {
        assert_eq!(
            format_shell_title("~\u{a0}$\u{a0}ls", 40),
            "~\u{a0}$\u{a0}ls",
        );
    }

    #[test]
    fn format_shell_title_no_tab_passthrough() {
        // Legacy/unstructured titles pass through unchanged.
        assert_eq!(format_shell_title("zsh", 40), "zsh");
    }

}

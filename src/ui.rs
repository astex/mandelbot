use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use iced::Size;

use alacritty_terminal::index::{Point as GridPoint, Side};
use alacritty_terminal::selection::Selection;

use crate::config::Config;
use crate::effect::Effect;
use crate::tab::{AgentRank, AgentStatus, TerminalTab};
use crate::theme::TerminalTheme;

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

#[derive(Clone)]
pub enum DisplayEntry {
    Tab(usize),
    Fold { parent_id: usize, count: usize, depth: usize },
}

#[derive(Debug, Clone)]
pub enum Message {
    TabOutput(usize, usize),
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
    McpCloseTab(usize, usize),
    SetTitle(usize, String),
    SetStatus(usize, AgentStatus),
    SetSelection(Option<Selection>),
    UpdateSelection(GridPoint, Side),
    Bell(usize),
    BellTick,
    FoldTab,
    UnfoldTab(usize),
    ClipboardLoadResult(usize, Option<String>),
}

pub(crate) fn terminal_size(window: Size, char_width: f32, char_height: f32) -> (usize, usize) {
    let cols = ((window.width - PADDING * 2.0 - TAB_BAR_WIDTH - crate::widget::terminal::SCROLLBAR_WIDTH) / char_width).floor() as usize;
    let rows = ((window.height - PADDING * 2.0) / char_height).floor() as usize;
    (rows.max(1), cols.max(1))
}

pub struct App {
    config: Config,
    tabs: Vec<TerminalTab>,
    active_tab_id: usize,
    prev_active_tab_id: Option<usize>,
    next_tab_id: usize,
    terminal_theme: TerminalTheme,
    window_size: Option<Size>,
    parent_socket_path: PathBuf,
    folded_tabs: HashSet<usize>,
    active_fold: Option<usize>,
}

impl App {
    pub fn new(config: Config, parent_socket_path: PathBuf) -> Self {
        let terminal_theme = config.terminal_theme();
        Self {
            config,
            tabs: Vec::new(),
            active_tab_id: 0,
            prev_active_tab_id: None,
            next_tab_id: 0,
            terminal_theme,
            window_size: None,
            parent_socket_path,
            folded_tabs: HashSet::new(),
            active_fold: None,
        }
    }

    // --- Read-only accessors for IcedHost / HeadlessHost ---

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn tabs(&self) -> &[TerminalTab] {
        &self.tabs
    }

    pub fn active_tab_id(&self) -> usize {
        self.active_tab_id
    }

    pub fn terminal_theme(&self) -> &TerminalTheme {
        &self.terminal_theme
    }

    pub fn active_fold(&self) -> Option<usize> {
        self.active_fold
    }

    pub fn active_tab(&self) -> Option<&TerminalTab> {
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
    ) -> (usize, Vec<Effect>) {
        // Expand any folded ancestors so the new tab is visible.
        if let Some(pid) = parent_id {
            self.unfold_ancestors(pid);
        }

        let Some(size) = self.window_size else {
            return (0, vec![]);
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
        let model = match rank {
            AgentRank::Home => &self.config.models.home,
            AgentRank::Project => &self.config.models.project,
            AgentRank::Task => &self.config.models.task,
        };

        let mut tab = TerminalTab::new(
            id, rows, cols, is_claude, rank,
            project_dir.clone(), parent_id, depth, project_id,
        );

        // Create event channel: main thread → tab thread.
        let (event_tx, event_rx) = std::sync::mpsc::channel();
        let pty_event_tx = event_tx.clone();
        tab.set_event_tx(event_tx);

        self.tabs.push(tab);

        // Initialize colors and window size for OSC query responses.
        if let Some(tab) = self.tabs.last() {
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
            model: model.clone(),
            parent_socket: self.parent_socket_path.clone(),
            prompt,
            branch,
            control_prefix: self.config.control_prefix.to_string(),
        };

        let tab = self.tabs.last().unwrap();
        let fifo_path = crate::tab::runtime_dir().join(format!("{id}.fifo"));

        let effects = vec![Effect::StartTab {
            tab_id: id,
            params,
            event_rx,
            pty_event_tx,
            term: tab.term_arc(),
            listener: tab.listener(),
            fifo_path,
        }];

        (id, effects)
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
    pub fn tab_display_order(&self) -> Vec<DisplayEntry> {
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

    /// Returns a mapping `tab_id -> number` (0..=9) for which visible tabs
    /// get digit shortcuts. The 10 slots follow the active tab's neighborhood:
    /// Home + first shell + ancestors + active tab's siblings, then filled
    /// outward by rank order. Numbers are always assigned in display order
    /// top-to-bottom.
    pub fn tab_number_assignments(&self) -> HashMap<usize, usize> {
        let display_order = self.tab_display_order();
        let visible: Vec<usize> = display_order.iter()
            .filter_map(|e| match e {
                DisplayEntry::Tab(id) => Some(*id),
                _ => None,
            })
            .collect();
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

        // 5. Everything else in rank order: Home, Project, Task, then shells.
        //    Within each rank, visit in display order.
        for rank in [AgentRank::Home, AgentRank::Project, AgentRank::Task] {
            if eligible.len() >= 10 { break; }
            for &id in &visible {
                if eligible.len() >= 10 { break; }
                if let Some(t) = self.tabs.iter().find(|t| t.id == id) {
                    if t.is_claude && t.rank == rank {
                        eligible.insert(id);
                    }
                }
            }
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

    fn close_tab(&mut self, tab_id: usize) -> Vec<Effect> {
        self.folded_tabs.remove(&tab_id);

        let Some(idx) = self.tabs.iter().position(|t| t.id == tab_id) else {
            return vec![];
        };

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
            return vec![Effect::Exit];
        }

        if self.active_tab_id == tab_id {
            let new_idx = idx.min(self.tabs.len() - 1);
            self.focus_tab(self.tabs[new_idx].id);
        }

        vec![]
    }

    pub fn update(&mut self, message: Message) -> Vec<Effect> {
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
                let (id, effects) = self.spawn_tab(true, AgentRank::Home, Some(home), None, first_run_prompt, None);
                self.focus_tab(id);
                if let Some(tab) = self.active_tab_mut() {
                    tab.title = Some("home".into());
                }
                effects
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
                vec![]
            }
            Message::TabOutput(tab_id, bg_tasks) => {
                let mut effects: Vec<Effect> = Vec::new();
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                    tab.background_tasks = bg_tasks;
                    if !tab.is_claude {
                        if let Some(title) = tab.take_osc_title() {
                            tab.title = Some(title);
                        }
                    }
                    if tab.take_bell() {
                        return vec![Effect::TriggerBell(tab_id)];
                    }

                    // Handle clipboard store requests (OSC 52 set).
                    for store in tab.take_clipboard_stores() {
                        let effect = match store.clipboard_type {
                            alacritty_terminal::term::ClipboardType::Clipboard => {
                                Effect::WriteClipboard(store.text)
                            }
                            alacritty_terminal::term::ClipboardType::Selection => {
                                Effect::WritePrimaryClipboard(store.text)
                            }
                        };
                        effects.push(effect);
                    }

                    // Handle clipboard load requests (OSC 52 query).
                    for load in tab.take_clipboard_loads() {
                        let effect = match load.clipboard_type {
                            alacritty_terminal::term::ClipboardType::Clipboard => {
                                Effect::ReadClipboard {
                                    tab_id,
                                    formatter: load.formatter,
                                }
                            }
                            alacritty_terminal::term::ClipboardType::Selection => {
                                Effect::ReadPrimaryClipboard {
                                    tab_id,
                                    formatter: load.formatter,
                                }
                            }
                        };
                        effects.push(effect);
                    }
                }
                effects
            }
            Message::ShellExited(tab_id, exit_code) => match exit_code {
                Some(0) | None => self.close_tab(tab_id),
                Some(_code) => {
                    // Exit hint is written by the tab thread.
                    if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                        tab.status = AgentStatus::Error;
                    }
                    vec![]
                }
            }
            Message::SetTitle(tab_id, title) => {
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                    tab.title = Some(title);
                }
                vec![]
            }
            Message::McpSpawnAgent(requesting_tab_id, working_directory, project_tab_id, prompt, branch) => {
                let requester = self.tabs.iter().find(|t| t.id == requesting_tab_id);
                let Some(requester) = requester else {
                    return vec![Effect::RespondToTab {
                        tab_id: requesting_tab_id,
                        response: serde_json::json!({"error": "unknown tab"}),
                    }];
                };

                let (rank, project_dir, parent_id) = match requester.rank {
                    AgentRank::Home => {
                        if let Some(ptid) = project_tab_id {
                            // Spawn a task under an existing project.
                            let project = self.tabs.iter().find(|t| t.id == ptid);
                            let Some(project) = project else {
                                return vec![Effect::RespondToTab {
                                    tab_id: requesting_tab_id,
                                    response: serde_json::json!({"error": "unknown project tab"}),
                                }];
                            };
                            if project.rank != AgentRank::Project {
                                return vec![Effect::RespondToTab {
                                    tab_id: requesting_tab_id,
                                    response: serde_json::json!({"error": "target tab is not a project agent"}),
                                }];
                            }
                            let dir = project.project_dir.clone();
                            (AgentRank::Task, dir, Some(ptid))
                        } else {
                            let Some(wd) = working_directory else {
                                return vec![Effect::RespondToTab {
                                    tab_id: requesting_tab_id,
                                    response: serde_json::json!({"error": "working_directory or project_tab_id required from home agent"}),
                                }];
                            };
                            let canonical = std::fs::canonicalize(&wd).unwrap_or(wd);
                            if let Some(existing) = self.find_project_for_dir(&canonical) {
                                self.focus_tab(existing);
                                return vec![Effect::RespondToTab {
                                    tab_id: requesting_tab_id,
                                    response: serde_json::json!({"tab_id": existing}),
                                }];
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

                let (new_tab_id, mut effects) = self.spawn_tab(true, rank, project_dir, parent_id, prompt, branch);
                effects.push(Effect::RespondToTab {
                    tab_id: requesting_tab_id,
                    response: serde_json::json!({"tab_id": new_tab_id}),
                });
                effects
            }
            Message::SetStatus(tab_id, status) => {
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                    tab.status = status;
                }
                vec![]
            }
            Message::Bell(tab_id) => vec![Effect::TriggerBell(tab_id)],
            Message::BellTick => {
                // Host-only message — App should not receive this, but return
                // empty effects if it does.
                vec![]
            }
            Message::PtyInput(bytes) => {
                if let Some(tab) = self.active_tab_mut() {
                    tab.write_input(&bytes);
                    if tab.is_claude
                        && tab.status == AgentStatus::NeedsReview
                        && bytes == b"\r"
                    {
                        tab.status = AgentStatus::Working;
                    }
                }
                vec![]
            }
            Message::Scroll(delta) => {
                if let Some(tab) = self.active_tab_mut() {
                    tab.scroll(delta);
                }
                vec![]
            }
            Message::ScrollTo(offset) => {
                if let Some(tab) = self.active_tab_mut() {
                    tab.scroll_to(offset);
                }
                vec![]
            }
            Message::NewTab => {
                let (id, effects) = self.spawn_tab(false, AgentRank::Home, None, None, None, None);
                self.focus_tab(id);
                effects
            }
            Message::SpawnAgent => {
                match self.active_rank() {
                    Some(AgentRank::Home) => {
                        // Create a pending project tab.
                        let Some(size) = self.window_size else {
                            return vec![];
                        };
                        let (rows, cols) = terminal_size(size, self.config.char_width(), self.config.char_height());
                        let home_id = self.active_tab_id;
                        let id = self.next_tab_id;
                        self.next_tab_id += 1;
                        let tab = TerminalTab::new_pending(id, rows, cols, home_id);
                        self.tabs.push(tab);
                        self.focus_tab(id);
                        vec![]
                    }
                    Some(AgentRank::Project | AgentRank::Task) => {
                        let parent_id = self.active_tab()
                            .and_then(|t| if t.rank == AgentRank::Task { t.parent_id } else { Some(t.id) });
                        let project_dir = self.project_dir_for_tab(self.active_tab_id);
                        if let (Some(pid), Some(dir)) = (parent_id, project_dir) {
                            let (id, effects) = self.spawn_tab(true, AgentRank::Task, Some(dir), Some(pid), None, None);
                            self.focus_tab(id);
                            effects
                        } else {
                            vec![]
                        }
                    }
                    None => vec![],
                }
            }
            Message::SpawnChild => {
                match self.active_rank() {
                    Some(AgentRank::Home) => {
                        let Some(size) = self.window_size else {
                            return vec![];
                        };
                        let (rows, cols) = terminal_size(size, self.config.char_width(), self.config.char_height());
                        let home_id = self.active_tab_id;
                        let id = self.next_tab_id;
                        self.next_tab_id += 1;
                        let tab = TerminalTab::new_pending(id, rows, cols, home_id);
                        self.tabs.push(tab);
                        self.focus_tab(id);
                        vec![]
                    }
                    Some(AgentRank::Project | AgentRank::Task) => {
                        if let Some(dir) = self.project_dir_for_tab(self.active_tab_id) {
                            let (id, effects) = self.spawn_tab(true, AgentRank::Task, Some(dir), Some(self.active_tab_id), None, None);
                            self.focus_tab(id);
                            effects
                        } else {
                            vec![]
                        }
                    }
                    None => vec![],
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
                vec![]
            }
            Message::NavigateRank(delta) => {
                if delta > 0 {
                    if let Some(child) = self.first_child(self.active_tab_id) {
                        self.focus_tab(child);
                    }
                } else if let Some(tab) = self.active_tab() {
                    if let Some(pid) = tab.parent_id {
                        self.focus_tab(pid);
                    }
                }
                vec![]
            }
            Message::FocusPreviousTab => {
                if let Some(prev) = self.prev_active_tab_id {
                    if self.tabs.iter().any(|t| t.id == prev) {
                        self.focus_tab(prev);
                    }
                }
                vec![]
            }
            Message::NextIdle => {
                let order = self.tab_display_order();
                let cur = self.current_display_index(&order).unwrap_or(0);

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

                let target = candidates.iter().find(|&&id| matches!(status_of(id), Some((AgentStatus::Blocked, _))))
                    .or_else(|| candidates.iter().find(|&&id| matches!(status_of(id), Some((AgentStatus::NeedsReview, _)))))
                    .or_else(|| candidates.iter().find(|&&id| matches!(status_of(id), Some((AgentStatus::Idle, AgentRank::Task)))))
                    .or_else(|| candidates.iter().find(|&&id| matches!(status_of(id), Some((AgentStatus::Idle, AgentRank::Project)))));

                if let Some(&id) = target {
                    self.focus_tab(id);
                }
                vec![]
            }
            Message::PendingInput(key) => {
                let tab_id = self.active_tab_id;
                let tab = self.tabs.iter_mut().find(|t| t.id == tab_id);
                let Some(tab) = tab else { return vec![] };
                let Some(input) = &mut tab.pending_input else { return vec![] };

                match key {
                    PendingKey::Char(c) => {
                        input.push(c);
                        vec![]
                    }
                    PendingKey::Backspace => {
                        input.pop();
                        vec![]
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
                            return vec![];
                        };
                        let parent_id = self.tabs[idx].parent_id;
                        self.tabs.remove(idx);

                        let (id, effects) = self.spawn_tab(
                            true,
                            AgentRank::Project,
                            Some(canonical),
                            parent_id,
                            None,
                            None,
                        );
                        self.focus_tab(id);
                        effects
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
                    return vec![Effect::RespondToTab {
                        tab_id: requesting_tab_id,
                        response: serde_json::json!({
                            "error": "not authorized to close that tab"
                        }),
                    }];
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
                    // Find the nearest surviving tab.
                    let old_idx = self.tabs.iter()
                        .position(|t| t.id == self.active_tab_id)
                        .unwrap_or(0);
                    let new_id = self.tabs.iter()
                        .enumerate()
                        .filter(|(_, t)| !to_close.contains(&t.id))
                        .min_by_key(|(idx, _)| (*idx as isize - old_idx as isize).unsigned_abs())
                        .map(|(_, t)| t.id);
                    if let Some(id) = new_id {
                        self.focus_tab(id);
                    }
                }
                if self.prev_active_tab_id.is_some_and(|id| to_close.contains(&id)) {
                    self.prev_active_tab_id = None;
                }

                let count = to_close.len();
                self.tabs.retain(|t| !to_close.contains(&t.id));

                let mut effects = vec![Effect::RespondToTab {
                    tab_id: requesting_tab_id,
                    response: serde_json::json!({
                        "message": format!("Closed {count} tab(s)")
                    }),
                }];
                if self.tabs.is_empty() {
                    effects.push(Effect::Exit);
                }
                effects
            }
            Message::SelectTab(tab_id) => {
                if self.tabs.iter().any(|t| t.id == tab_id) {
                    self.focus_tab(tab_id);
                }
                vec![]
            }
            Message::SelectTabByIndex(index) => {
                let assignments = self.tab_number_assignments();
                if let Some((&tab_id, _)) = assignments.iter().find(|&(_, &n)| n == index) {
                    self.focus_tab(tab_id);
                }
                vec![]
            }
            Message::FoldTab => {
                let tab_id = self.active_tab_id;
                if self.tabs.iter().any(|t| t.parent_id == Some(tab_id) && t.is_claude) {
                    self.folded_tabs.insert(tab_id);
                }
                vec![]
            }
            Message::UnfoldTab(parent_id) => {
                self.folded_tabs.remove(&parent_id);
                self.active_fold = None;
                if let Some(first_child) = self.tabs.iter()
                    .find(|t| t.parent_id == Some(parent_id) && t.is_claude)
                    .map(|t| t.id)
                {
                    self.focus_tab(first_child);
                }
                vec![]
            }
            Message::SetSelection(sel) => {
                if let Some(tab) = self.active_tab_mut() {
                    tab.set_selection(sel);
                }
                vec![]
            }
            Message::UpdateSelection(point, side) => {
                if let Some(tab) = self.active_tab_mut() {
                    tab.update_selection(point, side);
                }
                vec![]
            }
            Message::ClipboardLoadResult(tab_id, response) => {
                if let Some(response) = response {
                    if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                        tab.write_input(response.as_bytes());
                    }
                }
                vec![]
            }
        }
    }

    pub fn headless_snapshot(&self, label: Option<String>) -> HeadlessSnapshot {
        let tabs = self
            .tabs
            .iter()
            .map(|t| HeadlessTab {
                id: t.id,
                title: t.title.clone(),
                rank: t.rank,
                status: t.status,
                depth: t.depth,
                parent_id: t.parent_id,
                is_claude: t.is_claude,
                is_pending: t.is_pending(),
            })
            .collect();

        let mut folded: Vec<usize> = self.folded_tabs.iter().copied().collect();
        folded.sort_unstable();

        HeadlessSnapshot {
            label,
            active_tab_id: self.active_tab_id,
            prev_active_tab_id: self.prev_active_tab_id,
            tabs,
            folded,
        }
    }
}

/// JSON-serializable snapshot of `App` state produced by `App::headless_snapshot`.
/// The shape is stable enough for scenario tests to assert against.
#[derive(Debug, Clone, serde::Serialize)]
pub struct HeadlessSnapshot {
    pub label: Option<String>,
    pub active_tab_id: usize,
    pub prev_active_tab_id: Option<usize>,
    pub tabs: Vec<HeadlessTab>,
    pub folded: Vec<usize>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct HeadlessTab {
    pub id: usize,
    pub title: Option<String>,
    pub rank: AgentRank,
    pub status: AgentStatus,
    pub depth: usize,
    pub parent_id: Option<usize>,
    pub is_claude: bool,
    pub is_pending: bool,
}

/// Format a shell tab title from a `"<cwd>\t<prompt>\t<command>"` OSC string.
///
/// Walks up the directory chain from longest to shortest until the title fits
/// within `max_chars`, stopping at `~` or `/`. The prompt character varies by
/// shell (`%` for zsh, `$` for bash, `#` for root).
pub(crate) fn format_shell_title(raw: &str, max_chars: usize) -> String {
    if !raw.contains('\u{a0}') {
        return raw.to_string();
    }
    let mut fields = raw.splitn(3, '\u{a0}');
    let cwd = fields.next().unwrap();
    let prompt = fields.next().unwrap_or("$");
    let cmd = fields.next().unwrap_or("");

    let slash_positions: Vec<usize> = cwd
        .char_indices()
        .filter_map(|(i, c)| if c == '/' { Some(i) } else { None })
        .collect();

    let nbsp = '\u{a0}';
    let suffix = if cmd.is_empty() {
        format!("{nbsp}{prompt}{nbsp}")
    } else {
        format!("{nbsp}{prompt}{nbsp}{cmd}")
    };

    let candidate = format!("{cwd}{suffix}");
    if candidate.len() <= max_chars {
        return candidate.trim_end().to_string();
    }

    for &pos in &slash_positions {
        let dir = &cwd[pos + 1..];
        let candidate = format!("{dir}{suffix}");
        if candidate.len() <= max_chars {
            return candidate.trim_end().to_string();
        }
    }

    if cmd.is_empty() {
        prompt.to_string()
    } else {
        format!("{prompt}{nbsp}{cmd}")
    }
}

pub(crate) fn status_dot_color(status: AgentStatus, fg: iced::Color) -> iced::Color {
    match status {
        AgentStatus::Idle => fg,
        AgentStatus::Working => iced::Color::from_rgb8(0x50, 0xc8, 0x50),
        AgentStatus::Blocked => iced::Color::from_rgb8(0xe8, 0xb8, 0x30),
        AgentStatus::NeedsReview => iced::Color::from_rgb8(0x40, 0xa0, 0xe0),
        AgentStatus::Error => iced::Color::from_rgb8(0xe0, 0x40, 0x40),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_shell_title_idle() {
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
        assert_eq!(
            format_shell_title("~/src/mandelbot\u{a0}%\u{a0}vim", 40),
            "~/src/mandelbot\u{a0}%\u{a0}vim",
        );
        assert_eq!(
            format_shell_title("~/src/mandelbot\u{a0}%\u{a0}vim", 22),
            "src/mandelbot\u{a0}%\u{a0}vim",
        );
        assert_eq!(
            format_shell_title("~/src/mandelbot\u{a0}%\u{a0}vim", 18),
            "mandelbot\u{a0}%\u{a0}vim",
        );
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
        assert_eq!(format_shell_title("zsh", 40), "zsh");
    }
}

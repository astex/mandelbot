use std::io::BufRead;
use std::os::unix::net as unix;
use std::path::{Path, PathBuf};

use iced::widget::{button, column, container, row, text, Space};
use iced::{Border, Element, Fill, Size, Subscription, Task, Theme};

use alacritty_terminal::index::{Point as GridPoint, Side};
use alacritty_terminal::selection::Selection;

use crate::config::Config;
use crate::terminal::{AgentRank, TerminalTab};
use crate::theme::TerminalTheme;
use crate::widget::terminal::{self, TerminalWidget};

const PADDING: f32 = 4.0;
const TAB_BAR_WIDTH: f32 = 320.0;
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

#[derive(Debug, Clone)]
pub enum Message {
    TerminalOutput(usize, Vec<u8>),
    ShellExited(usize),
    PtyInput(Vec<u8>),
    Scroll(i32),
    ScrollTo(usize),
    WindowResized(Size),
    NewTab,
    SpawnAgent,
    CloseTab(usize),
    SelectTab(usize),
    SelectTabByIndex(usize),
    NavigateSibling(i32),
    NavigateRank(i32),
    PendingInput(PendingKey),
    SetTitle(usize, String),
    SetSelection(Option<Selection>),
    UpdateSelection(GridPoint, Side),
}

fn terminal_size(window: Size, char_width: f32, char_height: f32) -> (usize, usize) {
    let cols = ((window.width - PADDING * 2.0 - TAB_BAR_WIDTH - terminal::SCROLLBAR_WIDTH) / char_width).floor() as usize;
    let rows = ((window.height - PADDING * 2.0) / char_height).floor() as usize;
    (rows.max(1), cols.max(1))
}

pub struct App {
    config: Config,
    tabs: Vec<TerminalTab>,
    active_tab_id: usize,
    next_tab_id: usize,
    terminal_theme: TerminalTheme,
    window_size: Option<Size>,
    parent_socket_dir: PathBuf,
    parent_socket_path: PathBuf,
}

impl App {
    pub fn boot() -> (Self, Task<Message>) {
        let config = Config::load();
        let terminal_theme = config.terminal_theme();

        let parent_socket_dir =
            std::env::temp_dir().join(format!("mandelbot-{}", std::process::id()));
        std::fs::create_dir_all(&parent_socket_dir).expect("failed to create socket dir");
        let parent_socket_path = parent_socket_dir.join("parent.sock");

        // Bind the listener before any tabs can spawn MCP servers.
        let listener = unix::UnixListener::bind(&parent_socket_path)
            .expect("failed to bind parent socket");

        let listen_task = Task::run(parent_socket_stream(listener), |msg| msg);

        let app = Self {
            config,
            tabs: Vec::new(),
            active_tab_id: 0,
            next_tab_id: 0,
            terminal_theme,
            window_size: None,
            parent_socket_dir,
            parent_socket_path,
        };

        (app, listen_task)
    }

    fn active_tab(&self) -> Option<&TerminalTab> {
        self.tabs.iter().find(|t| t.id == self.active_tab_id)
    }

    fn active_tab_mut(&mut self) -> Option<&mut TerminalTab> {
        self.tabs.iter_mut().find(|t| t.id == self.active_tab_id)
    }

    fn spawn_tab(
        &mut self,
        is_claude: bool,
        rank: AgentRank,
        project_dir: Option<PathBuf>,
        parent_id: Option<usize>,
    ) -> Task<Message> {
        let Some(size) = self.window_size else {
            return Task::none();
        };
        let (rows, cols) = terminal_size(size, self.config.char_width(), self.config.char_height());
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        let (tab, task) = TerminalTab::spawn(
            id, rows, cols, is_claude, rank, project_dir, parent_id,
            &self.config.shell, &self.parent_socket_path,
        );
        self.tabs.push(tab);
        self.active_tab_id = id;
        task
    }

    fn active_rank(&self) -> Option<AgentRank> {
        self.active_tab().map(|t| t.rank)
    }

    fn project_for_tab(&self, tab_id: usize) -> Option<usize> {
        let tab = self.tabs.iter().find(|t| t.id == tab_id)?;
        match tab.rank {
            AgentRank::Project => Some(tab.id),
            AgentRank::Task => tab.parent_id,
            AgentRank::Home => None,
        }
    }

    fn first_child(&self, tab_id: usize) -> Option<usize> {
        self.tabs.iter()
            .find(|t| t.parent_id == Some(tab_id) && t.is_claude)
            .map(|t| t.id)
    }

    /// Returns tab IDs in tree display order: Home, then projects with their
    /// tasks nested underneath, then shell tabs.
    fn tab_display_order(&self) -> Vec<usize> {
        let mut order = Vec::new();

        // Home agent first.
        if let Some(home) = self.tabs.iter().find(|t| t.rank == AgentRank::Home) {
            order.push(home.id);

            // Projects under home.
            for proj in self.tabs.iter().filter(|t| t.rank == AgentRank::Project) {
                order.push(proj.id);

                // Tasks under this project.
                for task in self.tabs.iter().filter(|t| {
                    t.rank == AgentRank::Task && t.parent_id == Some(proj.id)
                }) {
                    order.push(task.id);
                }
            }
        }

        // Shell tabs at the end.
        for tab in self.tabs.iter().filter(|t| !t.is_claude) {
            order.push(tab.id);
        }

        order
    }

    fn find_project_for_dir(&self, dir: &Path) -> Option<usize> {
        self.tabs.iter()
            .find(|t| t.rank == AgentRank::Project && t.project_dir.as_deref() == Some(dir))
            .map(|t| t.id)
    }

    fn close_tab(&mut self, tab_id: usize) -> Task<Message> {
        let Some(idx) = self.tabs.iter().position(|t| t.id == tab_id) else {
            return Task::none();
        };
        self.tabs.remove(idx);

        if self.tabs.is_empty() {
            return iced::exit();
        }

        if self.active_tab_id == tab_id {
            let new_idx = idx.min(self.tabs.len() - 1);
            self.active_tab_id = self.tabs[new_idx].id;
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
                let task = self.spawn_tab(true, AgentRank::Home, Some(home), None);
                if let Some(tab) = self.active_tab_mut() {
                    tab.title = Some("home".into());
                }
                task
            }
            Message::WindowResized(size) => {
                self.window_size = Some(size);
                let (rows, cols) = terminal_size(size, self.config.char_width(), self.config.char_height());
                for tab in &mut self.tabs {
                    tab.resize(rows, cols, size.width as u16, size.height as u16);
                }
                Task::none()
            }
            Message::TerminalOutput(tab_id, bytes) => {
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                    tab.feed(&bytes);
                }
                Task::none()
            }
            Message::ShellExited(tab_id) => self.close_tab(tab_id),
            Message::SetTitle(tab_id, title) => {
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                    tab.title = Some(title);
                }
                Task::none()
            }
            Message::PtyInput(bytes) => {
                if let Some(tab) = self.active_tab_mut() {
                    tab.write_input(&bytes);
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
            Message::NewTab => self.spawn_tab(false, AgentRank::Home, None, None),
            Message::SpawnAgent => {
                match self.active_rank() {
                    Some(AgentRank::Home) => {
                        // Create a pending project tab.
                        let Some(size) = self.window_size else {
                            return Task::none();
                        };
                        let (rows, cols) = terminal_size(
                            size, self.config.char_width(), self.config.char_height(),
                        );
                        let home_id = self.active_tab_id;
                        let id = self.next_tab_id;
                        self.next_tab_id += 1;
                        let tab = TerminalTab::new_pending(id, rows, cols, home_id);
                        self.tabs.push(tab);
                        self.active_tab_id = id;
                        Task::none()
                    }
                    Some(AgentRank::Project | AgentRank::Task) => {
                        let project_id = self.project_for_tab(self.active_tab_id);
                        let project_dir = project_id.and_then(|pid| {
                            self.tabs.iter().find(|t| t.id == pid)
                                .and_then(|t| t.project_dir.clone())
                        });
                        if let (Some(pid), Some(dir)) = (project_id, project_dir) {
                            self.spawn_tab(true, AgentRank::Task, Some(dir), Some(pid))
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
                    self.active_tab_id = order[new_idx];
                }
                Task::none()
            }
            Message::NavigateRank(delta) => {
                if delta > 0 {
                    // Go to first child.
                    if let Some(child) = self.first_child(self.active_tab_id) {
                        self.active_tab_id = child;
                    }
                } else {
                    // Go to parent.
                    if let Some(tab) = self.active_tab() {
                        if let Some(pid) = tab.parent_id {
                            self.active_tab_id = pid;
                        }
                    }
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
                            self.active_tab_id = existing;
                            return self.close_tab(tab_id);
                        }

                        // Replace pending tab with a real project agent.
                        let Some(idx) = self.tabs.iter().position(|t| t.id == tab_id) else {
                            return Task::none();
                        };
                        let parent_id = self.tabs[idx].parent_id;
                        self.tabs.remove(idx);

                        self.spawn_tab(
                            true,
                            AgentRank::Project,
                            Some(canonical),
                            parent_id,
                        )
                    }
                }
            }
            Message::CloseTab(tab_id) => self.close_tab(tab_id),
            Message::SelectTab(tab_id) => {
                if self.tabs.iter().any(|t| t.id == tab_id) {
                    self.active_tab_id = tab_id;
                }
                Task::none()
            }
            Message::SelectTabByIndex(index) => {
                let display_order = self.tab_display_order();
                if let Some(&tab_id) = display_order.get(index) {
                    self.active_tab_id = tab_id;
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
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let active_bg = self.terminal_theme.bg;
        let inactive_bg = self.terminal_theme.black;
        let fg = self.terminal_theme.fg;

        let mut tab_col = column![];
        let display_order = self.tab_display_order();

        let tab_button = |tab: &TerminalTab, display_index: usize, active_tab_id: usize, indent: f32| {
            let is_active = tab.id == active_tab_id;
            let bg = if is_active { active_bg } else { inactive_bg };
            let tab_id = tab.id;

            let label_text = if tab.is_pending() {
                "new project...".to_string()
            } else if let Some(title) = &tab.title {
                title.clone()
            } else if tab.rank == AgentRank::Project {
                if let Some(dir) = &tab.project_dir {
                    dir.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| dir.to_string_lossy().into_owned())
                } else {
                    String::new()
                }
            } else {
                String::new()
            };

            let number_text = format!("{}", display_index + 1);

            let label = text(label_text)
                .size(self.config.font_size)
                .color(fg);
            let number = text(number_text)
                .size(self.config.font_size)
                .color(fg);

            let content = row![label, Space::new().width(Fill), number];

            let btn = button(content)
                .on_press(Message::SelectTab(tab_id))
                .width(TAB_BAR_WIDTH - indent)
                .style(move |_theme, _status| button::Style {
                    background: Some(bg.into()),
                    text_color: fg,
                    border: Border::default(),
                    ..Default::default()
                });

            if indent > 0.0 {
                row![Space::new().width(indent), btn].width(TAB_BAR_WIDTH).into()
            } else {
                Element::from(btn)
            }
        };

        // Agent tree: Home → Projects → Tasks.
        let indent_step = 20.0_f32;
        let mut has_agents = false;
        for (display_idx, &tab_id) in display_order.iter().enumerate() {
            let Some(tab) = self.tabs.iter().find(|t| t.id == tab_id) else { continue };
            if !tab.is_claude { continue; }
            has_agents = true;
            let indent = match tab.rank {
                AgentRank::Home => 0.0,
                AgentRank::Project => indent_step,
                AgentRank::Task => indent_step * 2.0,
            };
            tab_col = tab_col.push(tab_button(tab, display_idx, self.active_tab_id, indent));
        }

        // Gap between agent tree and shell tabs.
        let has_shells = self.tabs.iter().any(|t| !t.is_claude);
        if has_agents && has_shells {
            tab_col = tab_col.push(Space::new().width(TAB_BAR_WIDTH).height(TAB_GROUP_GAP));
        }

        // Shell tabs (flat).
        for (display_idx, &tab_id) in display_order.iter().enumerate() {
            let Some(tab) = self.tabs.iter().find(|t| t.id == tab_id) else { continue };
            if tab.is_claude { continue; }
            tab_col = tab_col.push(tab_button(tab, display_idx, self.active_tab_id, 0.0));
        }

        let tab_bar = container(tab_col)
            .width(TAB_BAR_WIDTH)
            .height(Fill)
            .style(move |_theme| container::Style {
                background: Some(inactive_bg.into()),
                ..Default::default()
            });

        let terminal_content: Element<'_, Message> = if let Some(tab) = self.active_tab() {
            TerminalWidget::new(tab, &self.config).into()
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
        let mcp_config_dir =
            std::env::temp_dir().join(format!("mandelbot-mcp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(mcp_config_dir);
    }
}

/// Stream that accepts connections on the parent socket and reads messages
/// from all connected MCP server instances.
fn parent_socket_stream(
    listener: unix::UnixListener,
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

                    std::thread::spawn(move || {
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
                                _ => None,
                            };
                            if let Some(message) = message {
                                if sender.try_send(message).is_err() {
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

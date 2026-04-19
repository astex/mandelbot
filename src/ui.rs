use std::collections::{HashMap, HashSet};
use std::io::BufRead;
use std::os::unix::net as unix;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use futures::SinkExt;

use iced::widget::{column, container, row, Space};
use iced::{Element, Fill, Size, Subscription, Task, Theme};

use alacritty_terminal::index::{Point as GridPoint, Side};
use alacritty_terminal::selection::Selection;

use crate::animation::FlashState;
use crate::config::Config;
use crate::tab::{AgentStatus, TerminalTab};
use crate::theme::TerminalTheme;
use crate::toast::Toast;
use crate::widget::terminal::{self, TerminalWidget};

pub mod handlers;

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
        result: Box<Result<crate::checkpoint::CheckpointOutcome, String>>,
    },
    ForkDone {
        source_tab_id: usize,
        action: ForkAction,
        result: Box<Result<crate::checkpoint::ForkOutcome, String>>,
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

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::WindowResized(size) => self.handle_window_resized(size),
            Message::TabOutput(tab_id, bg_tasks, pr_number) => {
                self.handle_tab_output(tab_id, bg_tasks, pr_number)
            }
            Message::ShellExited(tab_id, exit_code) => self.handle_shell_exited(tab_id, exit_code),
            Message::SetTitle(tab_id, title) => self.handle_set_title(tab_id, title),
            Message::McpSpawnAgent(rtid, wd, ptid, prompt, branch, model, base) => {
                self.handle_mcp_spawn_agent(rtid, wd, ptid, prompt, branch, model, base)
            }
            Message::SetStatus(tab_id, status) => self.handle_set_status(tab_id, status),
            Message::SetPr(tab_id, pr) => self.handle_set_pr(tab_id, pr),
            Message::WakeupAt(tab_id, epoch_ms) => self.handle_wakeup_at(tab_id, epoch_ms),
            Message::WakeupExpired(tab_id, epoch_ms) => {
                self.handle_wakeup_expired(tab_id, epoch_ms)
            }
            Message::Bell(tab_id) => self.bell_flashes.trigger(tab_id),
            Message::BellTick => self.bell_flashes.tick(),
            Message::PtyInput(bytes) => self.handle_pty_input(bytes),
            Message::Scroll(delta) => self.handle_scroll(delta),
            Message::ScrollTo(offset) => self.handle_scroll_to(offset),
            Message::NewTab => self.handle_new_tab(),
            Message::SpawnAgent => self.handle_spawn_agent(),
            Message::SpawnChild => self.handle_spawn_child(),
            Message::NavigateSibling(delta) => self.handle_navigate_sibling(delta),
            Message::NavigateRank(delta) => self.handle_navigate_rank(delta),
            Message::FocusPreviousTab => self.handle_focus_previous_tab(),
            Message::NextIdle => self.handle_next_idle(),
            Message::PendingInput(key) => self.handle_pending_input(key),
            Message::CloseTab(tab_id) => self.close_tab(tab_id),
            Message::McpCloseTab(rtid, target) => self.handle_mcp_close_tab(rtid, target),
            Message::SelectTab(tab_id) => self.handle_select_tab(tab_id),
            Message::SelectTabByIndex(index) => self.handle_select_tab_by_index(index),
            Message::ToggleFoldTab(tab_id) => self.handle_toggle_fold_tab(tab_id),
            Message::SetSelection(sel) => self.handle_set_selection(sel),
            Message::UpdateSelection(point, side) => self.handle_update_selection(point, side),
            Message::ClipboardLoadResult(tab_id, response) => {
                self.handle_clipboard_load_result(tab_id, response)
            }
            Message::OpenPr(tab_id) => self.handle_open_pr(tab_id),
            Message::TabReady { tab_id, worktree_dir, session_id } => {
                self.handle_tab_ready(tab_id, worktree_dir, session_id)
            }
            Message::McpCheckpoint(tab_id) => {
                self.kick_checkpoint(tab_id, CheckpointReason::Mcp)
            }
            Message::McpReplace(tab_id, ckpt_id) => self.handle_replace(tab_id, ckpt_id),
            Message::McpFork(tab_id, ckpt_id, prompt) => {
                self.handle_fork(tab_id, ckpt_id, prompt)
            }
            Message::McpListTabs(tab_id) => self.handle_mcp_list_tabs(tab_id),
            Message::AutoCheckpoint(tab_id) => self.handle_auto_checkpoint(tab_id),
            Message::ToggleTimeline(tab_id) => self.handle_toggle_timeline(tab_id),
            Message::CheckpointDone { tab_id, reason, result } => {
                self.finish_checkpoint(tab_id, reason, *result)
            }
            Message::ForkDone { source_tab_id, action, result } => {
                self.finish_fork(source_tab_id, action, *result)
            }
            Message::BackgroundDone => Task::none(),
            Message::TimelineScrub(tab_id, dir) => self.handle_timeline_scrub(tab_id, dir),
            Message::TimelineActivate(tab_id, mode) => self.handle_timeline_activate(tab_id, mode),
            Message::Undo(tab_id) => self.handle_undo(tab_id),
            Message::Redo(tab_id) => self.handle_redo(tab_id),
            Message::ShowToast { source_tab_id, message, prompt, target_tab_id } => {
                self.handle_show_toast(source_tab_id, message, prompt, target_tab_id)
            }
            Message::FocusFromToast(toast_id) => self.handle_focus_from_toast(toast_id),
            Message::DismissToast(toast_id) => self.handle_dismiss_toast(toast_id),
            Message::SpawnFromToast(toast_id) => self.handle_spawn_from_toast(toast_id),
        }
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


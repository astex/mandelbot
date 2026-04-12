use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::os::unix::net as unix;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use futures::SinkExt;

use iced::widget::{button, column, container, row, text, Space};
use iced::{Alignment, Border, Color, Element, Fill, Font, Subscription, Task, Theme};

use crate::animation::FlashState;
use crate::config::Config;
use crate::effect::Effect;
use crate::tab::{AgentRank, AgentStatus, TerminalTab};
use crate::ui::{
    format_shell_title, status_dot_color, App, DisplayEntry, Message,
    PADDING, TAB_BAR_WIDTH, TAB_GROUP_GAP,
};
use crate::widget::fold_placeholder::FoldPlaceholderWidget;
use crate::widget::terminal::TerminalWidget;

/// Writers for parent socket connections awaiting a response, keyed by tab ID.
type ResponseWriters = Arc<Mutex<HashMap<usize, unix::UnixStream>>>;

pub struct IcedHost {
    app: App,
    bell_flashes: FlashState,
    parent_socket_dir: PathBuf,
    response_writers: ResponseWriters,
}

impl IcedHost {
    pub fn boot() -> (Self, Task<Message>) {
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

        let config = Config::load();
        let app = App::new(config, parent_socket_path.clone());

        let host = Self {
            app,
            bell_flashes: FlashState::default(),
            parent_socket_dir,
            response_writers,
        };
        (host, listen_task)
    }

    pub fn update(&mut self, msg: Message) -> Task<Message> {
        // BellTick is purely a host animation concern — don't forward to App.
        if matches!(msg, Message::BellTick) {
            return self.bell_flashes.tick();
        }

        let effects = self.app.update(msg);
        Task::batch(
            effects
                .into_iter()
                .map(|e| self.run_effect(e))
                .collect::<Vec<_>>(),
        )
    }

    fn run_effect(&mut self, effect: Effect) -> Task<Message> {
        match effect {
            Effect::StartTab {
                tab_id: _,
                params,
                event_rx,
                pty_event_tx,
                term,
                listener,
                fifo_path,
            } => {
                let tab_task = Task::run(
                    crate::tab::tab_stream(params, event_rx, pty_event_tx, term, listener),
                    |msg| msg,
                );

                crate::tab::create_fifo(&fifo_path);
                let id = fifo_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(0);
                let fifo_task = Task::run(
                    crate::tab::fifo_stream(id, fifo_path),
                    |msg| msg,
                );

                Task::batch([tab_task, fifo_task])
            }
            Effect::WriteClipboard(text) => iced::clipboard::write(text),
            Effect::WritePrimaryClipboard(text) => iced::clipboard::write_primary(text),
            Effect::ReadClipboard { tab_id, formatter } => {
                iced::clipboard::read().map(move |content| {
                    let response = content.map(|text| (formatter)(&text));
                    Message::ClipboardLoadResult(tab_id, response)
                })
            }
            Effect::ReadPrimaryClipboard { tab_id, formatter } => {
                iced::clipboard::read_primary().map(move |content| {
                    let response = content.map(|text| (formatter)(&text));
                    Message::ClipboardLoadResult(tab_id, response)
                })
            }
            Effect::TriggerBell(tab_id) => self.bell_flashes.trigger(tab_id),
            Effect::RespondToTab { tab_id, response } => {
                if let Some(mut stream) = self.response_writers.lock().unwrap().remove(&tab_id) {
                    let mut msg = serde_json::to_string(&response).unwrap();
                    msg.push('\n');
                    let _ = stream.write_all(msg.as_bytes());
                    let _ = stream.flush();
                }
                Task::none()
            }
            Effect::Exit => iced::exit(),
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let active_bg = self.app.terminal_theme().bg;
        let inactive_bg = self.app.terminal_theme().black;
        let fg = self.app.terminal_theme().fg;
        let config = self.app.config();

        let mut tab_col = column![];
        let display_order = self.app.tab_display_order();

        let active_fold = self.app.active_fold();
        let active_tab_id = self.app.active_tab_id();
        let tabs = self.app.tabs();

        let tab_button = |tab: &TerminalTab, display_index: usize, active_tab_id: usize, indent: f32| {
            let is_active = tab.id == active_tab_id && active_fold.is_none();
            let tab_id = tab.id;

            let base_bg = if is_active { active_bg } else { inactive_bg };
            let bg = self.bell_flashes.blend(tab_id, base_bg, self.app.terminal_theme().yellow);

            let label_text: String = if tab.is_pending() {
                "new project...".into()
            } else if let Some(title) = &tab.title {
                if !tab.is_claude {
                    let cw = config.char_width();
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
                .size(config.font_size)
                .font(Font::MONOSPACE)
                .color(fg);
            let number = text(number_text)
                .size(config.font_size)
                .font(Font::MONOSPACE)
                .color(fg);

            let label = container(label).width(Fill).clip(true);

            let mut suffix = row![].align_y(Alignment::Center);
            if tab.is_claude && tab.background_tasks > 0 {
                let bg_label = format!("+{}", tab.background_tasks);
                suffix = suffix
                    .push(text(bg_label).size(config.font_size * 0.75).font(Font::MONOSPACE).color(self.app.terminal_theme().cyan))
                    .push(Space::new().width(6));
            }
            {
                let dot_size = config.font_size * 0.6;
                let dot_char = if tab.status == AgentStatus::Idle { "○" } else { "●" };
                let dot_color = status_dot_color(tab.status, fg);
                suffix = suffix
                    .push(text(dot_char).size(dot_size).color(dot_color))
                    .push(Space::new().width(4));
            }
            let suffix = suffix.push(number);

            let content = row![Space::new().width(PADDING), label, suffix, Space::new().width(PADDING)]
                .align_y(Alignment::Center);

            let btn = button(content)
                .on_press(Message::SelectTab(tab_id))
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

        // Determine whether to show group separator lines.
        let has_shells = tabs.iter().any(|t| !t.is_claude);
        let has_agents = tabs.iter().any(|t| t.is_claude);
        let project_count = tabs.iter().filter(|t| t.rank == AgentRank::Project).count();
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
                .size(config.font_size)
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
                    let Some(tab) = tabs.iter().find(|t| t.id == *tab_id) else { continue };
                    if !tab.is_claude { continue; }
                    if show_separators && tab.rank == AgentRank::Project {
                        tab_col = tab_col.push(Space::new().width(TAB_BAR_WIDTH).height(TAB_GROUP_GAP / 2.0));
                        tab_col = tab_col.push(separator());
                        tab_col = tab_col.push(Space::new().width(TAB_BAR_WIDTH).height(TAB_GROUP_GAP / 2.0));
                    }
                    let indent = tab.depth as f32 * indent_step;
                    let num = tab_numbers[i].unwrap_or(usize::MAX);
                    tab_col = tab_col.push(tab_button(tab, num, active_tab_id, indent));
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
            let Some(tab) = tabs.iter().find(|t| t.id == *tab_id) else { continue };
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
            tab_col = tab_col.push(tab_button(tab, num, active_tab_id, 0.0));
        }

        let tab_bar = container(tab_col)
            .width(TAB_BAR_WIDTH)
            .height(Fill)
            .style(move |_theme| container::Style {
                background: Some(inactive_bg.into()),
                ..Default::default()
            });

        let terminal_content: Element<'_, Message> = if let Some(fold_parent_id) = active_fold {
            let fold_count = display_order.iter()
                .find_map(|e| match e {
                    DisplayEntry::Fold { parent_id, count, .. } if *parent_id == fold_parent_id => Some(*count),
                    _ => None,
                })
                .unwrap_or(0);
            FoldPlaceholderWidget::new(fold_parent_id, fold_count, config).into()
        } else if let Some(tab) = self.app.active_tab() {
            TerminalWidget::new(tab, config).into()
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
        if self.app.terminal_theme().is_dark {
            Theme::Dark
        } else {
            Theme::Light
        }
    }
}

impl Drop for IcedHost {
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

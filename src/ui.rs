use iced::widget::{button, column, container, row, text, Space};
use iced::{Border, Element, Fill, Size, Subscription, Task, Theme};

use crate::config::Config;
use crate::terminal::TerminalTab;
use crate::theme::TerminalTheme;
use crate::widget::terminal::TerminalWidget;

const PADDING: f32 = 4.0;
const TAB_BAR_WIDTH: f32 = 36.0;
const INITIAL_ROWS: u16 = 24;
const INITIAL_COLS: u16 = 80;

pub fn initial_window_size(config: &Config) -> Size {
    Size {
        width: INITIAL_COLS as f32 * config.char_width() + PADDING * 2.0 + TAB_BAR_WIDTH,
        height: INITIAL_ROWS as f32 * config.char_height() + PADDING * 2.0,
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    TerminalOutput(usize, Vec<u8>),
    ShellExited(usize),
    PtyInput(Vec<u8>),
    Scroll(i32),
    WindowResized(Size),
    NewTab,
    CloseTab(usize),
    SelectTab(usize),
}

fn terminal_size(window: Size, char_width: f32, char_height: f32) -> (usize, usize) {
    let cols = ((window.width - PADDING * 2.0 - TAB_BAR_WIDTH) / char_width).floor() as usize;
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
}

impl App {
    pub fn boot() -> (Self, Task<Message>) {
        let config = Config::load();
        let terminal_theme = config.terminal_theme();

        let app = Self {
            config,
            tabs: Vec::new(),
            active_tab_id: 0,
            next_tab_id: 0,
            terminal_theme,
            window_size: None,
        };

        (app, Task::none())
    }

    fn active_tab(&self) -> Option<&TerminalTab> {
        self.tabs.iter().find(|t| t.id == self.active_tab_id)
    }

    fn active_tab_mut(&mut self) -> Option<&mut TerminalTab> {
        self.tabs.iter_mut().find(|t| t.id == self.active_tab_id)
    }

    fn spawn_tab(&mut self) -> Task<Message> {
        let Some(size) = self.window_size else {
            return Task::none();
        };
        let (rows, cols) = terminal_size(size, self.config.char_width(), self.config.char_height());
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        let (tab, task) = TerminalTab::new(id, rows, cols);
        self.tabs.push(tab);
        self.active_tab_id = id;
        task
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
                self.spawn_tab()
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
            Message::NewTab => self.spawn_tab(),
            Message::CloseTab(tab_id) => self.close_tab(tab_id),
            Message::SelectTab(tab_id) => {
                if self.tabs.iter().any(|t| t.id == tab_id) {
                    self.active_tab_id = tab_id;
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
        for (i, tab) in self.tabs.iter().enumerate() {
            let is_active = tab.id == self.active_tab_id;
            let bg = if is_active { active_bg } else { inactive_bg };
            let tab_id = tab.id;

            let label = text(format!("{}", i + 1))
                .size(self.config.font_size)
                .color(fg)
                .center();

            let btn = button(label)
                .on_press(Message::SelectTab(tab_id))
                .width(TAB_BAR_WIDTH)
                .style(move |_theme, _status| button::Style {
                    background: Some(bg.into()),
                    text_color: fg,
                    border: Border::default(),
                    ..Default::default()
                });
            tab_col = tab_col.push(btn);
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

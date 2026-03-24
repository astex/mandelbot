use iced::widget::container;
use iced::{Element, Fill, Size, Subscription, Task, Theme};

use crate::config::Config;
use crate::terminal::TerminalTab;
use crate::theme::TerminalTheme;
use crate::widget::terminal::TerminalWidget;

const PADDING: f32 = 4.0;
const INITIAL_ROWS: u16 = 24;
const INITIAL_COLS: u16 = 80;

pub fn initial_window_size(config: &Config) -> Size {
    Size {
        width: INITIAL_COLS as f32 * config.char_width() + PADDING * 2.0,
        height: INITIAL_ROWS as f32 * config.char_height() + PADDING * 2.0,
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    TerminalOutput(Vec<u8>),
    ShellExited,
    PtyInput(Vec<u8>),
    Scroll(i32),
    WindowResized(Size),
}

fn terminal_size(window: Size, char_width: f32, char_height: f32) -> (usize, usize) {
    let cols = ((window.width - PADDING * 2.0) / char_width).floor() as usize;
    let rows = ((window.height - PADDING * 2.0) / char_height).floor() as usize;
    (rows.max(1), cols.max(1))
}

pub struct App {
    config: Config,
    tab: Option<TerminalTab>,
    terminal_theme: TerminalTheme,
}

impl App {
    pub fn boot() -> (Self, Task<Message>) {
        let config = Config::load();
        let terminal_theme = config.terminal_theme();

        let app = Self {
            config,
            tab: None,
            terminal_theme,
        };

        (app, Task::none())
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            // First resize: spawn terminal at actual window dimensions.
            Message::WindowResized(size) if self.tab.is_none() => {
                let (rows, cols) = terminal_size(size, self.config.char_width(), self.config.char_height());
                let (tab, task) = TerminalTab::new(rows, cols);
                self.tab = Some(tab);
                task
            }
            Message::TerminalOutput(bytes) => {
                if let Some(tab) = &mut self.tab {
                    tab.feed(&bytes);
                }
                Task::none()
            }
            Message::ShellExited => iced::exit(),
            Message::PtyInput(bytes) => {
                if let Some(tab) = &mut self.tab {
                    tab.write_input(&bytes);
                }
                Task::none()
            }
            Message::Scroll(delta) => {
                if let Some(tab) = &mut self.tab {
                    tab.scroll(delta);
                }
                Task::none()
            }
            Message::WindowResized(size) => {
                let (rows, cols) = terminal_size(size, self.config.char_width(), self.config.char_height());
                if let Some(tab) = &mut self.tab {
                    tab.resize(rows, cols, size.width as u16, size.height as u16);
                }
                Task::none()
            }
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let content: Element<'_, Message> = if let Some(tab) = &self.tab {
            TerminalWidget::new(tab, &self.config)
                .into()
        } else {
            iced::widget::Space::new().width(Fill).height(Fill).into()
        };

        container(content)
            .padding(PADDING)
            .width(Fill)
            .height(Fill)
            .style(move |_theme| container::Style {
                background: Some(self.terminal_theme.bg.into()),
                ..Default::default()
            })
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

use std::io::{Read, Write};

use iced::widget::{container, rich_text};
use iced::{Element, Fill, Font, Size, Subscription, Task, Theme};
use portable_pty::{MasterPty, PtySize};

use crate::config::Config;
use crate::keys;
use crate::pty;
use crate::terminal::TerminalBuffer;
use crate::theme::{self, TerminalTheme};

const LINE_HEIGHT: f32 = 1.3;
const PADDING: f32 = 4.0;
const INITIAL_ROWS: u16 = 24;
const INITIAL_COLS: u16 = 80;

pub fn initial_window_size(config: &Config) -> Size {
    let char_width = config.font_size * 0.6;
    let char_height = config.font_size * LINE_HEIGHT;
    Size {
        width: INITIAL_COLS as f32 * char_width + PADDING * 2.0,
        height: INITIAL_ROWS as f32 * char_height + PADDING * 2.0,
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    TerminalOutput(Vec<u8>),
    ShellExited,
    KeyEvent(iced::keyboard::Event),
    MouseEvent(iced::mouse::Event),
    WindowResized(Size),
}

pub enum Terminal {
    WaitingForSize {
        config: Config,
    },
    Running {
        terminal_buffer: TerminalBuffer,
        master: Box<dyn MasterPty + Send>,
        writer: Box<dyn Write + Send>,
        pty_cols: usize,
        terminal_theme: TerminalTheme,
        font_size: f32,
        char_width: f32,
        char_height: f32,
        is_dark: bool,
    },
}

impl Terminal {
    pub fn boot() -> (Self, Task<Message>) {
        let config = Config::load();
        (Self::WaitingForSize { config }, Task::none())
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match self {
            Self::WaitingForSize { config } => {
                let Message::WindowResized(size) = message else {
                    return Task::none();
                };

                let font_size = config.font_size;
                let char_width = font_size * 0.6;
                let char_height = font_size * LINE_HEIGHT;
                let is_dark = config.is_dark();
                let terminal_theme = if is_dark {
                    theme::solarized_dark()
                } else {
                    theme::solarized_light()
                };

                let cols = ((size.width - PADDING * 2.0) / char_width).floor() as usize;
                let rows = ((size.height - PADDING * 2.0) / char_height).floor() as usize;
                let cols = cols.max(1);
                let rows = rows.max(1);

                let (master, _child) =
                    pty::spawn_shell("/bin/bash", rows as u16, cols as u16)
                        .expect("failed to spawn PTY");

                let reader = master.try_clone_reader().expect("failed to clone reader");
                let writer = master.take_writer().expect("failed to take writer");

                *self = Self::Running {
                    terminal_buffer: TerminalBuffer::new(rows, cols),
                    master,
                    writer,
                    pty_cols: cols,
                    terminal_theme,
                    font_size,
                    char_width,
                    char_height,
                    is_dark,
                };

                Task::run(pty_stream(reader), |message| message)
            }
            _ => self.update_running(message),
        }
    }

    fn update_running(&mut self, message: Message) -> Task<Message> {
        let Self::Running {
            terminal_buffer,
            master,
            writer,
            pty_cols,
            char_width,
            char_height,
            ..
        } = self
        else {
            unreachable!()
        };

        let char_width = *char_width;
        let char_height = *char_height;

        match message {
            Message::TerminalOutput(bytes) => {
                terminal_buffer.feed(&bytes);
                terminal_buffer.scroll_to_bottom();
                Task::none()
            }
            Message::ShellExited => iced::exit(),
            Message::KeyEvent(event) => {
                if let iced::keyboard::Event::KeyPressed { key, text, modifiers, .. } = event {
                    use iced::keyboard::key::Named;
                    use iced::keyboard::Key;

                    let bytes: Vec<u8> = match (key, text) {
                        (Key::Named(Named::Enter), _) if modifiers.shift() => vec![b'\n'],
                        (Key::Named(Named::Enter), _) => vec![b'\r'],
                        (Key::Named(Named::Backspace), _) => vec![keys::DEL],
                        (Key::Named(Named::Space), _) => vec![keys::SPACE],
                        (Key::Named(Named::Tab), _) => vec![keys::TAB],
                        (Key::Named(Named::Escape), _) => vec![keys::ESCAPE],
                        (Key::Named(Named::ArrowUp), _) => keys::ARROW_UP.to_vec(),
                        (Key::Named(Named::ArrowDown), _) => keys::ARROW_DOWN.to_vec(),
                        (Key::Named(Named::ArrowRight), _) => keys::ARROW_RIGHT.to_vec(),
                        (Key::Named(Named::ArrowLeft), _) => keys::ARROW_LEFT.to_vec(),
                        (Key::Character(c), _) if modifiers.control() && c.as_ref() == "c" => {
                            vec![keys::CTRL_C]
                        }
                        (Key::Named(Named::PageUp), _) if modifiers.shift() => {
                            terminal_buffer.scroll(-(terminal_buffer.rows() as i32));
                            return Task::none();
                        }
                        (Key::Named(Named::PageDown), _) if modifiers.shift() => {
                            terminal_buffer.scroll(terminal_buffer.rows() as i32);
                            return Task::none();
                        }
                        (Key::Named(_), _) => return Task::none(),
                        (_, Some(chars)) if !chars.is_empty() => {
                            chars.to_string().into_bytes()
                        }
                        _ => return Task::none(),
                    };

                    let _ = writer.write_all(&bytes);
                    let _ = writer.flush();
                }
                Task::none()
            }
            Message::MouseEvent(iced::mouse::Event::WheelScrolled { delta }) => {
                let lines = match delta {
                    iced::mouse::ScrollDelta::Lines { y, .. } => y as i32,
                    iced::mouse::ScrollDelta::Pixels { y, .. } => (y / char_height) as i32,
                };
                if lines != 0 {
                    terminal_buffer.scroll(lines);
                }
                Task::none()
            }
            Message::MouseEvent(_) => Task::none(),
            Message::WindowResized(size) => {
                let cols = ((size.width - PADDING * 2.0) / char_width).floor() as usize;
                let rows = ((size.height - PADDING * 2.0) / char_height).floor() as usize;
                let cols = cols.max(1);
                let rows = rows.max(1);

                if rows == terminal_buffer.rows() && cols == *pty_cols {
                    return Task::none();
                }

                terminal_buffer.resize(rows, cols);
                *pty_cols = cols;

                let _ = master.resize(PtySize {
                    rows: rows as u16,
                    cols: cols as u16,
                    pixel_width: size.width as u16,
                    pixel_height: size.height as u16,
                });

                Task::none()
            }
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let (spans, font_size, bg) = match self {
            Self::WaitingForSize { config } => {
                let theme = if config.is_dark() {
                    theme::solarized_dark()
                } else {
                    theme::solarized_light()
                };
                (vec![], config.font_size, theme.bg)
            }
            Self::Running {
                terminal_buffer,
                terminal_theme,
                font_size,
                ..
            } => (
                terminal_buffer.screen_spans(terminal_theme),
                *font_size,
                terminal_theme.bg,
            ),
        };

        container(
            rich_text(spans)
                .font(Font::MONOSPACE)
                .size(font_size),
        )
        .padding(PADDING)
        .width(Fill)
        .height(Fill)
        .style(move |_theme| container::Style {
            background: Some(bg.into()),
            ..Default::default()
        })
        .into()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            iced::keyboard::listen().map(Message::KeyEvent),
            iced::event::listen_with(|event, _, _| {
                if let iced::Event::Mouse(mouse_event) = event {
                    Some(Message::MouseEvent(mouse_event))
                } else {
                    None
                }
            }),
            iced::window::resize_events().map(|(_, size)| Message::WindowResized(size)),
        ])
    }

    pub fn theme(&self) -> Theme {
        match self {
            Self::WaitingForSize { config } if !config.is_dark() => Theme::Light,
            Self::Running { is_dark: false, .. } => Theme::Light,
            _ => Theme::Dark,
        }
    }
}

fn pty_stream(mut reader: Box<dyn Read + Send>) -> impl iced::futures::Stream<Item = Message> {
    iced::stream::channel(
        32,
        |mut sender: iced::futures::channel::mpsc::Sender<Message>| async move {
            let (exit_sender, exit_receiver) = iced::futures::channel::oneshot::channel::<()>();

            std::thread::spawn(move || {
                let mut read_buffer = [0u8; 4096];
                loop {
                    match reader.read(&mut read_buffer) {
                        Ok(0) | Err(_) => break,
                        Ok(bytes_read) => {
                            let bytes = read_buffer[..bytes_read].to_vec();
                            if sender.try_send(Message::TerminalOutput(bytes)).is_err() {
                                break;
                            }
                        }
                    }
                }
                let _ = sender.try_send(Message::ShellExited);
                let _ = exit_sender.send(());
            });

            let _ = exit_receiver.await;
        },
    )
}

use std::io::{Read, Write};

use iced::widget::{container, rich_text};
use iced::{Color, Element, Fill, Font, Size, Subscription, Task, Theme};
use portable_pty::{MasterPty, PtySize};

use crate::keys;
use crate::pty;
use crate::terminal::TerminalBuffer;

const FONT_SIZE: f32 = 14.0;
const LINE_HEIGHT: f32 = 1.3;
const CHAR_WIDTH: f32 = FONT_SIZE * 0.6;
const CHAR_HEIGHT: f32 = FONT_SIZE * LINE_HEIGHT;
const PADDING: f32 = 4.0;
const INITIAL_ROWS: u16 = 24;
const INITIAL_COLS: u16 = 80;

pub const INITIAL_WINDOW_SIZE: Size = Size {
    width: INITIAL_COLS as f32 * CHAR_WIDTH + PADDING * 2.0,
    height: INITIAL_ROWS as f32 * CHAR_HEIGHT + PADDING * 2.0,
};

#[derive(Debug, Clone)]
pub enum Message {
    TerminalOutput(Vec<u8>),
    ShellExited,
    KeyEvent(iced::keyboard::Event),
    WindowResized(Size),
}

fn terminal_size(window: Size) -> (usize, usize) {
    let cols = ((window.width - PADDING * 2.0) / CHAR_WIDTH).floor() as usize;
    let rows = ((window.height - PADDING * 2.0) / CHAR_HEIGHT).floor() as usize;
    (rows.max(1), cols.max(1))
}

pub enum Terminal {
    WaitingForSize,
    Running {
        terminal_buffer: TerminalBuffer,
        master: Box<dyn MasterPty + Send>,
        writer: Box<dyn Write + Send>,
        pty_cols: usize,
    },
}

impl Terminal {
    pub fn boot() -> (Self, Task<Message>) {
        (Self::WaitingForSize, Task::none())
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match self {
            Self::WaitingForSize => {
                let Message::WindowResized(size) = message else {
                    return Task::none();
                };
                let (rows, cols) = terminal_size(size);

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
        } = self
        else {
            unreachable!()
        };

        match message {
            Message::TerminalOutput(bytes) => {
                terminal_buffer.feed(&bytes);
                Task::none()
            }
            Message::ShellExited => iced::exit(),
            Message::KeyEvent(event) => {
                if let iced::keyboard::Event::KeyPressed { key, text, modifiers, .. } = event {
                    use iced::keyboard::key::Named;
                    use iced::keyboard::Key;

                    let bytes: Vec<u8> = match (key, text) {
                        (Key::Named(Named::Enter), _) => vec![b'\r'],
                        (Key::Named(Named::Backspace), _) => vec![keys::DEL],
                        (Key::Named(Named::Space), _) => vec![keys::SPACE],
                        (Key::Named(Named::Tab), _) => vec![keys::TAB],
                        (Key::Named(Named::ArrowUp), _) => keys::ARROW_UP.to_vec(),
                        (Key::Named(Named::ArrowDown), _) => keys::ARROW_DOWN.to_vec(),
                        (Key::Named(Named::ArrowRight), _) => keys::ARROW_RIGHT.to_vec(),
                        (Key::Named(Named::ArrowLeft), _) => keys::ARROW_LEFT.to_vec(),
                        (Key::Character(c), _) if modifiers.control() && c.as_ref() == "c" => {
                            vec![keys::CTRL_C]
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
            Message::WindowResized(size) => {
                let (rows, cols) = terminal_size(size);

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
        let spans = match self {
            Self::WaitingForSize => vec![],
            Self::Running { terminal_buffer, .. } => terminal_buffer.screen_spans(),
        };

        container(
            rich_text(spans)
                .font(Font::MONOSPACE)
                .size(FONT_SIZE),
        )
        .padding(PADDING)
        .width(Fill)
        .height(Fill)
        .style(|_theme| container::Style {
            background: Some(Color::from_rgb(0.12, 0.12, 0.12).into()),
            ..Default::default()
        })
        .into()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            iced::keyboard::listen().map(Message::KeyEvent),
            iced::window::resize_events().map(|(_, size)| Message::WindowResized(size)),
        ])
    }

    pub fn theme(&self) -> Theme {
        Theme::Dark
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

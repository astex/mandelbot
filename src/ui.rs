use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use iced::widget::{container, text};
use iced::{Color, Element, Fill, Font, Size, Subscription, Task, Theme};
use portable_pty::{MasterPty, PtySize};
use vte::Parser;

use crate::keys;
use crate::pty;
use crate::terminal::TerminalBuffer;

const CHAR_WIDTH: f32 = 7.0;
const CHAR_HEIGHT: f32 = 18.4;
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
        parser: Parser,
        terminal_buffer: TerminalBuffer,
        screen: String,
        master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
        writer: Arc<Mutex<Box<dyn Write + Send>>>,
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
                    parser: Parser::new(),
                    terminal_buffer: TerminalBuffer::new(rows),
                    screen: String::new(),
                    master: Arc::new(Mutex::new(master)),
                    writer: Arc::new(Mutex::new(writer)),
                    pty_cols: cols,
                };

                Task::run(pty_stream(reader), |message| message)
            }
            _ => self.update_running(message),
        }
    }

    fn update_running(&mut self, message: Message) -> Task<Message> {
        let Self::Running {
            parser,
            terminal_buffer,
            screen,
            master,
            writer,
            pty_cols,
        } = self
        else {
            unreachable!()
        };

        match message {
            Message::TerminalOutput(bytes) => {
                for &byte in &bytes {
                    parser.advance(terminal_buffer, byte);
                }
                *screen = terminal_buffer.screen_text();
                Task::none()
            }
            Message::ShellExited => iced::exit(),
            Message::KeyEvent(event) => {
                if let iced::keyboard::Event::KeyPressed { key, text, .. } = event {
                    use iced::keyboard::key::Named;
                    use iced::keyboard::Key;

                    let bytes = match (key, text) {
                        (Key::Named(Named::Enter), _) => vec![b'\r'],
                        (Key::Named(Named::Backspace), _) => vec![keys::DEL],
                        (Key::Named(Named::Space), _) => vec![keys::SPACE],
                        // TODO: arrow keys, tab, function keys, etc.
                        (Key::Named(_), _) => return Task::none(),
                        (_, Some(chars)) if !chars.is_empty() => {
                            chars.to_string().into_bytes()
                        }
                        _ => return Task::none(),
                    };

                    let mut writer = writer.lock().unwrap();
                    let _ = writer.write_all(&bytes);
                    let _ = writer.flush();
                }
                Task::none()
            }
            Message::WindowResized(size) => {
                let (rows, cols) = terminal_size(size);

                if rows == terminal_buffer.rows && cols == *pty_cols {
                    return Task::none();
                }

                terminal_buffer.rows = rows;
                *pty_cols = cols;

                let master = master.lock().unwrap();
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
        let screen = match self {
            Self::WaitingForSize => "",
            Self::Running { screen, .. } => screen,
        };

        container(
            text(screen)
                .font(Font::MONOSPACE)
                .size(14)
                .color(Color::from_rgb(0.83, 0.83, 0.83)),
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

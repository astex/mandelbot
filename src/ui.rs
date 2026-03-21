use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use iced::widget::{container, text};
use iced::{Color, Element, Fill, Font, Subscription, Task, Theme};

use crate::keys;
use crate::pty;
use crate::terminal::TerminalBuffer;

#[derive(Debug, Clone)]
pub enum Message {
    TerminalOutput(Vec<u8>),
    ShellExited,
    KeyEvent(iced::keyboard::Event),
}

pub struct Terminal {
    terminal_buffer: TerminalBuffer,
    screen: String,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
}

impl Terminal {
    pub fn boot() -> (Self, Task<Message>) {
        let mut pty_handle =
            pty::PtyHandle::spawn("/bin/bash", 24, 80).expect("failed to spawn PTY");

        let reader = pty_handle.take_reader();
        let writer = pty_handle.take_writer();

        let terminal = Self {
            terminal_buffer: TerminalBuffer::new(24, 80),
            screen: String::new(),
            writer: Arc::new(Mutex::new(writer)),
        };

        let task = Task::run(pty_stream(reader), |message| message);

        (terminal, task)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::TerminalOutput(bytes) => {
                self.terminal_buffer.feed(&bytes);
                self.screen = self.terminal_buffer.screen_text();
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
                        // TODO: arrow keys, tab, function keys, etc.
                        (Key::Named(_), _) => return Task::none(),
                        (_, Some(chars)) if !chars.is_empty() => {
                            chars.to_string().into_bytes()
                        }
                        _ => return Task::none(),
                    };

                    let mut writer = self.writer.lock().unwrap();
                    let _ = writer.write_all(&bytes);
                    let _ = writer.flush();
                }
                Task::none()
            }
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        container(
            text(&self.screen)
                .font(Font::MONOSPACE)
                .size(14)
                .color(Color::from_rgb(0.83, 0.83, 0.83)),
        )
        .padding(4)
        .width(Fill)
        .height(Fill)
        .style(|_theme| container::Style {
            background: Some(Color::from_rgb(0.12, 0.12, 0.12).into()),
            ..Default::default()
        })
        .into()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        iced::keyboard::listen().map(Message::KeyEvent)
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

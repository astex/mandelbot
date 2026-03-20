use std::io::{Read, Write};
use std::sync::{Arc, Mutex, OnceLock};

use iced::futures::{SinkExt, StreamExt};
use iced::widget::{container, text};
use iced::{Color, Element, Fill, Font, Subscription, Theme};

use crate::terminal::SharedBuffer;

static PTY_READER: OnceLock<Mutex<Option<Box<dyn Read + Send>>>> = OnceLock::new();
static PTY_BUFFER: OnceLock<SharedBuffer> = OnceLock::new();
static PTY_WRITER: OnceLock<Arc<Mutex<Box<dyn Write + Send>>>> = OnceLock::new();

#[derive(Debug, Clone)]
pub enum Message {
    TerminalOutput(String),
    KeyEvent(iced::keyboard::Event),
}

pub fn init(buffer: SharedBuffer, writer: Box<dyn Write + Send>, reader: Box<dyn Read + Send>) {
    PTY_BUFFER.set(buffer).ok();
    PTY_WRITER.set(Arc::new(Mutex::new(writer))).ok();
    PTY_READER.set(Mutex::new(Some(reader))).ok();
}

pub struct Terminal {
    screen: String,
}

impl Terminal {
    pub fn boot() -> (Self, iced::Task<Message>) {
        (Self { screen: String::new() }, iced::Task::none())
    }

    pub fn update(&mut self, message: Message) {
        match message {
            Message::TerminalOutput(s) => {
                self.screen = s;
            }
            Message::KeyEvent(event) => {
                if let iced::keyboard::Event::KeyPressed { key, text, .. } = event {
                    let bytes = match key {
                        iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter) => {
                            b"\r".to_vec()
                        }
                        iced::keyboard::Key::Named(iced::keyboard::key::Named::Backspace) => {
                            vec![0x7f]
                        }
                        _ => match text {
                            Some(s) if !s.is_empty() => s.to_string().into_bytes(),
                            _ => return,
                        },
                    };

                    let writer = PTY_WRITER.get().expect("writer not initialized");
                    if let Ok(mut w) = writer.lock() {
                        let _ = w.write_all(&bytes);
                        let _ = w.flush();
                    }
                }
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
        Subscription::batch([
            iced::keyboard::listen().map(Message::KeyEvent),
            Subscription::run(pty_stream),
        ])
    }

    pub fn theme(&self) -> Theme {
        Theme::Dark
    }
}

fn pty_stream() -> impl iced::futures::Stream<Item = Message> {
    iced::stream::channel(32, |mut sender: iced::futures::channel::mpsc::Sender<Message>| async move {
        let (tx, mut rx) = iced::futures::channel::mpsc::channel::<String>(32);

        let mut reader = PTY_READER
            .get()
            .and_then(|r| r.lock().ok()?.take())
            .expect("PTY reader not initialized");

        let buffer = PTY_BUFFER.get().expect("PTY buffer not initialized").clone();

        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let text = {
                            let mut tb = buffer.lock().unwrap();
                            tb.feed(&buf[..n]);
                            tb.screen_text()
                        };
                        if tx.clone().try_send(text).is_err() {
                            break;
                        }
                    }
                }
            }
        });

        loop {
            match rx.next().await {
                Some(text) => {
                    let _ = sender.send(Message::TerminalOutput(text)).await;
                }
                None => break,
            }
        }
    })
}

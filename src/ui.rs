use std::io::{Read, Write};

use iced::widget::container;
use iced::{Element, Fill, Size, Subscription, Task, Theme};
use portable_pty::{MasterPty, PtySize};

use crate::config::Config;
use crate::pty;
use crate::terminal::TerminalBuffer;
use crate::theme::TerminalTheme;
use crate::widget::terminal::TerminalWidget;

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
    PtyInput(Vec<u8>),
    Scroll(i32),
    WindowResized(Size),
}

fn terminal_size(window: Size, char_width: f32, char_height: f32) -> (usize, usize) {
    let cols = ((window.width - PADDING * 2.0) / char_width).floor() as usize;
    let rows = ((window.height - PADDING * 2.0) / char_height).floor() as usize;
    (rows.max(1), cols.max(1))
}

pub struct Terminal {
    config: Config,
    terminal_buffer: Option<TerminalBuffer>,
    master: Option<Box<dyn MasterPty + Send>>,
    writer: Option<Box<dyn Write + Send>>,
    terminal_theme: TerminalTheme,
    char_width: f32,
    char_height: f32,
    pty_cols: usize,
}

impl Terminal {
    pub fn boot() -> (Self, Task<Message>) {
        let config = Config::load();
        let char_width = config.font_size * 0.6;
        let char_height = config.font_size * LINE_HEIGHT;
        let terminal_theme = config.terminal_theme();

        let terminal = Self {
            config,
            terminal_buffer: None,
            master: None,
            writer: None,
            terminal_theme,
            char_width,
            char_height,
            pty_cols: INITIAL_COLS as usize,
        };

        (terminal, Task::none())
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::WindowResized(size) if self.terminal_buffer.is_none() => {
                let (rows, cols) = terminal_size(size, self.char_width, self.char_height);

                let (master, _child) =
                    pty::spawn_shell("/bin/bash", rows as u16, cols as u16)
                        .expect("failed to spawn PTY");

                let reader = master.try_clone_reader().expect("failed to clone reader");
                let writer = master.take_writer().expect("failed to take writer");

                self.terminal_buffer = Some(TerminalBuffer::new(rows, cols));
                self.master = Some(master);
                self.writer = Some(writer);
                self.pty_cols = cols;

                Task::run(pty_stream(reader), |message| message)
            }
            Message::TerminalOutput(bytes) => {
                if let Some(buf) = &mut self.terminal_buffer {
                    buf.feed(&bytes);
                    buf.scroll_to_bottom();
                }
                Task::none()
            }
            Message::ShellExited => iced::exit(),
            Message::PtyInput(bytes) => {
                if let Some(writer) = &mut self.writer {
                    let _ = writer.write_all(&bytes);
                    let _ = writer.flush();
                }
                Task::none()
            }
            Message::Scroll(delta) => {
                if let Some(buf) = &mut self.terminal_buffer {
                    buf.scroll(delta);
                }
                Task::none()
            }
            Message::WindowResized(size) => {
                let (rows, cols) = terminal_size(size, self.char_width, self.char_height);

                if let Some(buf) = &mut self.terminal_buffer {
                    if rows == buf.rows() && cols == self.pty_cols {
                        return Task::none();
                    }

                    buf.resize(rows, cols);
                    self.pty_cols = cols;

                    if let Some(master) = &self.master {
                        let _ = master.resize(PtySize {
                            rows: rows as u16,
                            cols: cols as u16,
                            pixel_width: size.width as u16,
                            pixel_height: size.height as u16,
                        });
                    }
                }

                Task::none()
            }
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let content: Element<'_, Message> = if let Some(buf) = &self.terminal_buffer {
            TerminalWidget::new(buf, &self.terminal_theme, &self.config)
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

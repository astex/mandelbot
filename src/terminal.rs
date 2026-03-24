use std::io::{Read, Write};

use alacritty_terminal::event::VoidListener;
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::term::cell::Cell;
use alacritty_terminal::term::{Config, TermMode};
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::vte::ansi;
use alacritty_terminal::{Grid, Term};
use portable_pty::{MasterPty, PtySize};

use crate::pty;
use crate::ui::Message;

pub struct TerminalTab {
    term: Term<VoidListener>,
    parser: ansi::Processor,
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    pty_cols: usize,
}

impl TerminalTab {
    pub fn new(rows: usize, cols: usize, channel_socket_path: &str) -> (Self, iced::Task<Message>) {
        let size = TermSize::new(cols, rows);
        let term = Term::new(Config::default(), &size, VoidListener);

        let (master, _child) =
            pty::spawn_shell(
                "claude",
                &["--dangerously-load-development-channels", "server:mandelbot"],
                &[("MANDELBOT_SOCKET", channel_socket_path)],
                rows as u16,
                cols as u16,
            )
            .expect("failed to spawn PTY");

        let reader = master.try_clone_reader().expect("failed to clone reader");
        let writer = master.take_writer().expect("failed to take writer");

        let tab = Self {
            term,
            parser: ansi::Processor::new(),
            master,
            writer,
            pty_cols: cols,
        };

        let task = iced::Task::run(pty_stream(reader), |msg| msg);
        (tab, task)
    }

    pub fn feed(&mut self, data: &[u8]) {
        let was_at_bottom = self.term.grid().display_offset() == 0;
        self.parser.advance(&mut self.term, data);
        if was_at_bottom {
            self.term.scroll_display(Scroll::Bottom);
        }
    }

    pub fn write_input(&mut self, bytes: &[u8]) {
        let _ = self.writer.write_all(bytes);
        let _ = self.writer.flush();
    }

    pub fn scroll(&mut self, delta: i32) {
        self.term.scroll_display(Scroll::Delta(delta));
    }

    pub fn resize(&mut self, rows: usize, cols: usize, pixel_width: u16, pixel_height: u16) {
        if rows == self.term.screen_lines() && cols == self.pty_cols {
            return;
        }

        let size = TermSize::new(cols, rows);
        self.term.resize(size);
        self.pty_cols = cols;

        let _ = self.master.resize(PtySize {
            rows: rows as u16,
            cols: cols as u16,
            pixel_width,
            pixel_height,
        });
    }

    pub fn rows(&self) -> usize {
        self.term.screen_lines()
    }

    pub fn grid(&self) -> &Grid<Cell> {
        self.term.grid()
    }

    pub fn mode(&self) -> TermMode {
        *self.term.mode()
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

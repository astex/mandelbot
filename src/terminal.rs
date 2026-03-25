use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

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
    pub id: usize,
    term: Term<VoidListener>,
    parser: ansi::Processor,
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    pty_cols: usize,
    mcp_config_dir: Option<PathBuf>,
}

impl TerminalTab {
    pub fn new(
        id: usize,
        rows: usize,
        cols: usize,
        parent_socket: &Path,
    ) -> (Self, iced::Task<Message>) {
        let size = TermSize::new(cols, rows);
        let term = Term::new(Config::default(), &size, VoidListener);

        let mcp_config_dir = write_mcp_config(id, parent_socket);

        let shell_config = pty::ShellConfig {
            command: "claude",
            args: &[],
            env: HashMap::new(),
            cwd: Some(&mcp_config_dir),
            rows: rows as u16,
            cols: cols as u16,
        };

        let (master, _child) = pty::spawn_shell(&shell_config).expect("failed to spawn PTY");

        let reader = master.try_clone_reader().expect("failed to clone reader");
        let writer = master.take_writer().expect("failed to take writer");

        let tab = Self {
            id,
            term,
            parser: ansi::Processor::new(),
            master,
            writer,
            pty_cols: cols,
            mcp_config_dir: Some(mcp_config_dir),
        };

        let task = iced::Task::run(pty_stream(id, reader), |msg| msg);
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

    pub fn scroll_to(&mut self, offset: usize) {
        let current = self.term.grid().display_offset() as i32;
        let delta = offset as i32 - current;
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

    pub fn history_size(&self) -> usize {
        self.term.grid().history_size()
    }

    pub fn mode(&self) -> TermMode {
        *self.term.mode()
    }
}

impl Drop for TerminalTab {
    fn drop(&mut self) {
        if let Some(dir) = self.mcp_config_dir.take() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

/// Write a .mcp.json to a temp directory that tells Claude how to spawn the
/// MCP server for this session.
fn write_mcp_config(session_id: usize, parent_socket: &Path) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "mandelbot-mcp-{}-{}",
        std::process::id(),
        session_id,
    ));
    std::fs::create_dir_all(&dir).expect("failed to create mcp config dir");

    let exe = std::env::current_exe()
        .expect("failed to get current exe")
        .to_string_lossy()
        .into_owned();

    let config = serde_json::json!({
        "mcpServers": {
            "mandelbot": {
                "command": exe,
                "args": [
                    "--mcp-server",
                    "--session-id", session_id.to_string(),
                    "--parent-socket", parent_socket.to_string_lossy(),
                ],
            },
        },
    });

    let config_path = dir.join(".mcp.json");
    std::fs::write(&config_path, serde_json::to_string_pretty(&config).unwrap())
        .expect("failed to write .mcp.json");

    dir
}

fn pty_stream(tab_id: usize, mut reader: Box<dyn Read + Send>) -> impl iced::futures::Stream<Item = Message> {
    iced::stream::channel(
        32,
        move |mut sender: iced::futures::channel::mpsc::Sender<Message>| async move {
            let (exit_sender, exit_receiver) = iced::futures::channel::oneshot::channel::<()>();

            std::thread::spawn(move || {
                let mut read_buffer = [0u8; 4096];
                loop {
                    match reader.read(&mut read_buffer) {
                        Ok(0) | Err(_) => break,
                        Ok(bytes_read) => {
                            let bytes = read_buffer[..bytes_read].to_vec();
                            if sender.try_send(Message::TerminalOutput(tab_id, bytes)).is_err() {
                                break;
                            }
                        }
                    }
                }
                let _ = sender.try_send(Message::ShellExited(tab_id));
                let _ = exit_sender.send(());
            });

            let _ = exit_receiver.await;
        },
    )
}

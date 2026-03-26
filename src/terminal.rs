use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use alacritty_terminal::event::VoidListener;
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionRange};
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::{Config, TermMode};
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::vte::ansi;
use alacritty_terminal::{Grid, Term};
use portable_pty::{MasterPty, PtySize};

use crate::pty;
use crate::ui::Message;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentRank {
    Home,
    Project,
    Task,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AgentStatus {
    #[default]
    Idle,
    Working,
    Blocked,
    NeedsReview,
    Error,
}

impl AgentStatus {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "idle" => Some(Self::Idle),
            "working" => Some(Self::Working),
            "blocked" => Some(Self::Blocked),
            "needs_review" => Some(Self::NeedsReview),
            "error" => Some(Self::Error),
            _ => None,
        }
    }
}

pub struct TerminalTab {
    pub id: usize,
    pub is_claude: bool,
    pub rank: AgentRank,
    pub project_dir: Option<PathBuf>,
    pub parent_id: Option<usize>,
    pub title: Option<String>,
    pub status: AgentStatus,
    pub pending_input: Option<String>,
    term: Term<VoidListener>,
    parser: ansi::Processor,
    master: Option<Box<dyn MasterPty + Send>>,
    writer: Option<Box<dyn Write + Send>>,
    pty_cols: usize,
}

impl TerminalTab {
    pub fn spawn(
        id: usize,
        rows: usize,
        cols: usize,
        is_claude: bool,
        rank: AgentRank,
        project_dir: Option<PathBuf>,
        parent_id: Option<usize>,
        shell: &str,
        parent_socket: &Path,
    ) -> (Self, iced::Task<Message>) {
        let size = TermSize::new(cols, rows);
        let term = Term::new(Config::default(), &size, VoidListener);

        let config_dir = write_mcp_config();
        let mcp_config_flag = config_dir.join("mcp-config.json").to_string_lossy().into_owned();
        let system_prompt_path = write_system_prompt(&config_dir, rank);
        let system_prompt_flag = system_prompt_path.to_string_lossy().into_owned();
        let hooks_settings_path = write_hooks_settings(&config_dir);
        let hooks_settings_flag = hooks_settings_path.to_string_lossy().into_owned();

        let (command, args_vec, env, cwd);
        if is_claude {
            command = "claude";
            let mut args = vec![
                "--mcp-config", &mcp_config_flag,
                "--append-system-prompt-file", &system_prompt_flag,
                "--settings", &hooks_settings_flag,
            ];
            if rank == AgentRank::Task {
                args.push("-w");
            }
            args_vec = args;
            env = HashMap::from([
                ("MANDELBOT_TAB_ID", id.to_string()),
                ("MANDELBOT_PARENT_SOCKET", parent_socket.to_string_lossy().into_owned()),
            ]);
            cwd = project_dir.as_deref();
        } else {
            let parts: Vec<&str> = shell.split_whitespace().collect();
            let (cmd, rest) = parts.split_first().expect("shell config must not be empty");
            command = cmd;
            args_vec = rest.to_vec();
            env = HashMap::new();
            cwd = None;
        }

        let shell_config = pty::ShellConfig {
            command,
            args: &args_vec,
            env,
            cwd,
            rows: rows as u16,
            cols: cols as u16,
        };

        let (master, _child) = pty::spawn_shell(&shell_config).expect("failed to spawn PTY");

        let reader = master.try_clone_reader().expect("failed to clone reader");
        let writer = master.take_writer().expect("failed to take writer");

        let tab = Self {
            id,
            is_claude,
            rank,
            project_dir,
            parent_id,
            title: None,
            status: AgentStatus::default(),
            pending_input: None,
            term,
            parser: ansi::Processor::new(),
            master: Some(master),
            writer: Some(writer),
            pty_cols: cols,
        };

        let task = iced::Task::run(pty_stream(id, reader), |msg| msg);
        (tab, task)
    }

    pub fn new_pending(id: usize, rows: usize, cols: usize, parent_id: usize) -> Self {
        let size = TermSize::new(cols, rows);
        let term = Term::new(Config::default(), &size, VoidListener);

        Self {
            id,
            is_claude: true,
            rank: AgentRank::Project,
            project_dir: None,
            parent_id: Some(parent_id),
            title: None,
            status: AgentStatus::default(),
            pending_input: Some(String::new()),
            term,
            parser: ansi::Processor::new(),
            master: None,
            writer: None,
            pty_cols: cols,
        }
    }

    pub fn is_pending(&self) -> bool {
        self.pending_input.is_some()
    }

    pub fn feed(&mut self, data: &[u8]) {
        let was_at_bottom = self.term.grid().display_offset() == 0;
        self.parser.advance(&mut self.term, data);
        if was_at_bottom {
            self.term.scroll_display(Scroll::Bottom);
        }
    }

    pub fn write_input(&mut self, bytes: &[u8]) {
        if let Some(writer) = &mut self.writer {
            let _ = writer.write_all(bytes);
            let _ = writer.flush();
        }
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

        if let Some(master) = &self.master {
            let _ = master.resize(PtySize {
                rows: rows as u16,
                cols: cols as u16,
                pixel_width,
                pixel_height,
            });
        }
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

    pub fn set_selection(&mut self, selection: Option<Selection>) {
        self.term.selection = selection;
    }

    pub fn update_selection(&mut self, point: Point, side: Side) {
        if let Some(sel) = self.term.selection.as_mut() {
            sel.update(point, side);
        }
    }

    pub fn selection_to_string(&self) -> Option<String> {
        self.term.selection_to_string()
    }

    pub fn selection_range(&self) -> Option<SelectionRange> {
        self.term.selection.as_ref()?.to_range(&self.term)
    }

    /// Extract the logical line (possibly spanning multiple wrapped rows)
    /// that contains the given grid line. Returns the concatenated text and
    /// metadata needed to map between character offsets and grid positions.
    pub fn logical_line_at(&self, line: Line) -> LogicalLine {
        let grid = self.term.grid();
        let cols = grid.columns();
        let topmost = Line(-(grid.history_size() as i32));
        let bottommost = Line(grid.screen_lines() as i32 - 1);

        // Walk backwards to find the first row of this logical line.
        // A row wraps into the next if its last cell has WRAPLINE set.
        let mut start = line;
        loop {
            let prev = Line(start.0 + 1);
            if prev > bottommost {
                break;
            }
            if grid[prev][Column(cols - 1)].flags.contains(Flags::WRAPLINE) {
                start = prev;
            } else {
                break;
            }
        }

        // Walk forward collecting text until we find a row without WRAPLINE.
        let mut text = String::new();
        let mut current = start;
        loop {
            for col in 0..cols {
                text.push(grid[current][Column(col)].c);
            }
            if current <= topmost {
                break;
            }
            if grid[current][Column(cols - 1)].flags.contains(Flags::WRAPLINE) {
                current = Line(current.0 - 1);
            } else {
                break;
            }
        }

        LogicalLine { text, start_line: start, cols }
    }
}

pub struct LogicalLine {
    pub text: String,
    pub start_line: Line,
    pub cols: usize,
}

impl LogicalLine {
    /// Convert a grid point to a character offset in the logical line text.
    pub fn char_offset(&self, line: Line, col: usize) -> usize {
        let row_offset = (self.start_line.0 - line.0) as usize;
        row_offset * self.cols + col
    }

    /// Convert a character offset back to a grid (line, col) pair.
    pub fn grid_position(&self, char_offset: usize) -> (Line, usize) {
        let row_offset = char_offset / self.cols;
        let col = char_offset % self.cols;
        (Line(self.start_line.0 - row_offset as i32), col)
    }
}

/// Write config files to a temp directory for Claude. Returns the directory
/// path. The MCP config and hooks settings are static — tab ID and parent
/// socket path are passed via environment variables so that every tab sees
/// the same commands and Claude only prompts for approval once.
fn write_mcp_config() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mandelbot-mcp-{}", std::process::id()));
    let config_path = dir.join("mcp-config.json");

    if config_path.exists() {
        return dir;
    }

    std::fs::create_dir_all(&dir).expect("failed to create mcp config dir");

    let exe = std::env::current_exe()
        .expect("failed to get current exe")
        .to_string_lossy()
        .into_owned();

    let config = serde_json::json!({
        "mcpServers": {
            "mandelbot": {
                "command": exe,
                "args": ["--mcp-server"],
            },
        },
    });

    std::fs::write(&config_path, serde_json::to_string_pretty(&config).unwrap())
        .expect("failed to write mcp config");

    dir
}

fn write_hooks_settings(dir: &Path) -> PathBuf {
    let path = dir.join("hooks-settings.json");

    let exe = std::env::current_exe()
        .expect("failed to get current exe")
        .to_string_lossy()
        .into_owned();

    let set_status = |status: &str| -> serde_json::Value {
        serde_json::json!({
            "type": "command",
            "command": format!("{exe} --set-status {status}"),
        })
    };

    let settings = serde_json::json!({
        "hooks": {
            "UserPromptSubmit": [{
                "hooks": [set_status("working")],
            }],
            "PreToolUse": [{
                "matcher": "",
                "hooks": [set_status("working")],
            }],
            "PermissionRequest": [
                {
                    "hooks": [set_status("blocked")],
                },
                {
                    "matcher": "ExitPlanMode",
                    "hooks": [set_status("needs_review")],
                },
            ],
            "PostToolUse": [{
                "matcher": "",
                "hooks": [set_status("working")],
            }],
            "PostToolUseFailure": [{
                "hooks": [set_status("working")],
            }],
            "Stop": [{
                "hooks": [set_status("idle")],
            }],
            "StopFailure": [{
                "hooks": [set_status("error")],
            }],
        },
    });

    std::fs::write(&path, serde_json::to_string_pretty(&settings).unwrap())
        .expect("failed to write hooks settings");

    path
}

const SYSTEM_PROMPT: &str = include_str!("agents/PROMPT.md");
const HOME_PROMPT: &str = include_str!("agents/HOME_PROMPT.md");
const PROJECT_PROMPT: &str = include_str!("agents/PROJECT_PROMPT.md");

fn write_system_prompt(dir: &Path, rank: AgentRank) -> PathBuf {
    let (filename, content) = match rank {
        AgentRank::Home => ("home-prompt.md", HOME_PROMPT),
        AgentRank::Project => ("project-prompt.md", PROJECT_PROMPT),
        AgentRank::Task => ("system-prompt.md", SYSTEM_PROMPT),
    };
    let path = dir.join(filename);
    if !path.exists() {
        std::fs::write(&path, content).expect("failed to write system prompt");
    }
    path
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

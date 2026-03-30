use std::collections::HashMap;
use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionRange};
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::{Config, TermMode};
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::vte::ansi;
use alacritty_terminal::{Grid, Term};
use portable_pty::{MasterPty, PtySize};

use futures::SinkExt;

use crate::pty;
use crate::ui::Message;

#[derive(Clone)]
struct TermEventListener {
    title: Arc<Mutex<Option<String>>>,
}

impl TermEventListener {
    fn new() -> Self {
        Self { title: Arc::new(Mutex::new(None)) }
    }
}

impl EventListener for TermEventListener {
    fn send_event(&self, event: Event) {
        if let Event::Title(t) = event {
            *self.title.lock().unwrap() = Some(t);
        } else if let Event::ResetTitle = event {
            *self.title.lock().unwrap() = None;
        }
    }
}

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
    pub depth: usize,
    pub project_id: Option<usize>,
    pub title: Option<String>,
    pub status: AgentStatus,
    pub pending_input: Option<String>,
    term: Term<TermEventListener>,
    listener: TermEventListener,
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
        depth: usize,
        project_id: Option<usize>,
        shell: &str,
        parent_socket: &Path,
        prompt: Option<String>,
    ) -> (Self, iced::Task<Message>) {
        let size = TermSize::new(cols, rows);
        let listener = TermEventListener::new();
        let term = Term::new(Config::default(), &size, listener.clone());

        let config_dir = write_mcp_config();
        let mcp_config_flag = config_dir.join("mcp-config.json").to_string_lossy().into_owned();
        let system_prompt_path = write_system_prompt(&config_dir, rank);
        let system_prompt_flag = system_prompt_path.to_string_lossy().into_owned();
        let hooks_settings_path = write_hooks_settings(&config_dir);
        let hooks_settings_flag = hooks_settings_path.to_string_lossy().into_owned();

        let prompt_flag = prompt.unwrap_or_default();
        let (command, args_vec, env, cwd);
        let wrapped_cmd; // holds the shell -c argument for Claude tabs
        if is_claude {
            // Spawn Claude inside a login shell so that shell profiles and
            // direnv are evaluated before the process starts.
            let shell_parts: Vec<&str> = shell.split_whitespace().collect();
            command = shell_parts[0];

            let mut claude_args = format!(
                "claude --mcp-config {} --append-system-prompt-file {} --settings {}",
                pty::shell_quote(&mcp_config_flag),
                pty::shell_quote(&system_prompt_flag),
                pty::shell_quote(&hooks_settings_flag),
            );
            if rank == AgentRank::Task {
                claude_args.push_str(" -w");
            }
            let plugin_dir = write_plugin_dir(&config_dir);
            claude_args.push_str(&format!(
                " --plugin-dir {}",
                pty::shell_quote(&plugin_dir.to_string_lossy()),
            ));
            let home = std::env::var("HOME").unwrap_or_default();
            let mandelbot_dir = PathBuf::from(home).join(".mandelbot");
            claude_args.push_str(&format!(
                " --add-dir {}",
                pty::shell_quote(&mandelbot_dir.to_string_lossy()),
            ));
            if !prompt_flag.is_empty() {
                claude_args.push_str(" -- ");
                claude_args.push_str(&pty::shell_quote(&prompt_flag));
            }

            wrapped_cmd = format!("exec {claude_args}");
            args_vec = vec!["-l", "-i", "-c", &wrapped_cmd];
            let fifo_path = runtime_dir().join(format!("{id}.fifo"));
            create_fifo(&fifo_path);
            env = HashMap::from([
                ("MANDELBOT_TAB_ID".to_string(), id.to_string()),
                ("MANDELBOT_PARENT_SOCKET".to_string(), parent_socket.to_string_lossy().into_owned()),
                ("MANDELBOT_FIFO".to_string(), fifo_path.to_string_lossy().into_owned()),
            ]);
            cwd = project_dir.as_deref();
        } else {
            let parts: Vec<&str> = shell.split_whitespace().collect();
            let (cmd, rest) = parts.split_first().expect("shell config must not be empty");
            command = cmd;
            args_vec = rest.to_vec();
            env = shell_integration_env(command);
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
            depth,
            project_id,
            title: None,
            status: AgentStatus::default(),
            pending_input: None,
            term,
            listener,
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
        let listener = TermEventListener::new();
        let term = Term::new(Config::default(), &size, listener.clone());

        Self {
            id,
            is_claude: true,
            rank: AgentRank::Project,
            project_dir: None,
            parent_id: Some(parent_id),
            depth: 1,
            project_id: Some(id),
            title: None,
            status: AgentStatus::default(),
            pending_input: Some(String::new()),
            term,
            listener,
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

    /// Take the latest title set via OSC escape sequences, if any.
    pub fn take_osc_title(&self) -> Option<String> {
        self.listener.title.lock().unwrap().take()
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

/// Return the runtime directory for this process, using `$XDG_RUNTIME_DIR`
/// when available and falling back to `~/.mandelbot/run/`.
pub fn runtime_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_RUNTIME_DIR") {
        PathBuf::from(xdg)
    } else {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home).join(".mandelbot").join("run")
    }
    .join(format!("mandelbot-{}", std::process::id()))
}

/// Return the current executable path, stripping the " (deleted)" suffix that
/// Linux appends to `/proc/self/exe` when the binary has been replaced on disk
/// (e.g. after a rebuild while the app is still running).
fn current_exe_path() -> String {
    std::env::current_exe()
        .expect("failed to get current exe")
        .to_string_lossy()
        .trim_end_matches(" (deleted)")
        .to_owned()
}

fn create_fifo(path: &Path) {
    let c_path = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
    let ret = unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) };
    if ret != 0 {
        let err = std::io::Error::last_os_error();
        if err.kind() != std::io::ErrorKind::AlreadyExists {
            panic!("mkfifo({}) failed: {err}", path.display());
        }
    }
}

/// Write config files to a temp directory for Claude. Returns the directory
/// path. The MCP config and hooks settings are static — tab ID and parent
/// socket path are passed via environment variables so that every tab sees
/// the same commands and Claude only prompts for approval once.
fn write_mcp_config() -> PathBuf {
    let dir = runtime_dir().join("mcp");
    let config_path = dir.join("mcp-config.json");

    if config_path.exists() {
        return dir;
    }

    std::fs::create_dir_all(&dir).expect("failed to create mcp config dir");

    let exe = current_exe_path();

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

    let set_status = |status: &str| -> serde_json::Value {
        serde_json::json!({
            "type": "command",
            "command": format!("echo {status} > $MANDELBOT_FIFO"),
        })
    };

    // A conditional variant that only sets status when the tool_name is NOT
    // ExitPlanMode. This avoids a race between the catch-all "blocked" hook
    // and the ExitPlanMode-specific "needs_review" hook, which both fire in
    // parallel on an ExitPlanMode permission request.
    let set_status_unless_exit_plan = |status: &str| -> serde_json::Value {
        serde_json::json!({
            "type": "command",
            "command": format!(
                r#"grep -q '"tool_name":"ExitPlanMode"\|"tool_name": "ExitPlanMode"' || echo {status} > $MANDELBOT_FIFO"#,
            ),
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
                    "hooks": [set_status_unless_exit_plan("blocked")],
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

const SKILL_DELEGATE: &str = include_str!("agents/skills/mandelbot-delegate/SKILL.md");
const SKILL_DELEGATE_TEMPLATE: &str = include_str!("agents/skills/mandelbot-delegate/template.md");
const SKILL_DELEGATE_WATCH: &str = include_str!("agents/skills/mandelbot-delegate/watch.sh");
const SKILL_WORK_AS_SUBTASK: &str = include_str!("agents/skills/mandelbot-work-as-subtask/SKILL.md");
const SKILL_MANDELBOT_CONFIG: &str = include_str!("agents/skills/mandelbot-config/SKILL.md");
const SKILL_MANDELBOT_KEYBINDINGS: &str =
    include_str!("agents/skills/mandelbot-keybindings/SKILL.md");

const SHELL_INTEGRATION_ZSH: &str = r#"
# Mandelbot shell integration — sets tab title to the running command.
_mandelbot_preexec() { printf '\e]0;%s\a' "$1" }
_mandelbot_precmd()  { printf '\e]0;%s\a' "${ZSH_NAME:-zsh}" }
autoload -Uz add-zsh-hook
add-zsh-hook preexec _mandelbot_preexec
add-zsh-hook precmd  _mandelbot_precmd
"#;

const SHELL_INTEGRATION_BASH: &str = r#"
# Mandelbot shell integration — sets tab title to the running command.
_mandelbot_preexec() {
  if [ -z "$MANDELBOT_IN_PROMPT" ]; then
    printf '\e]0;%s\a' "$BASH_COMMAND"
  fi
}
_mandelbot_precmd() {
  MANDELBOT_IN_PROMPT=1
  printf '\e]0;%s\a' "bash"
  unset MANDELBOT_IN_PROMPT
}
trap '_mandelbot_preexec' DEBUG
PROMPT_COMMAND="_mandelbot_precmd${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
"#;

/// Write shell integration scripts and return env vars to source them.
fn shell_integration_env(shell_command: &str) -> HashMap<String, String> {
    let dir = runtime_dir().join("shell");
    std::fs::create_dir_all(&dir).expect("failed to create shell integration dir");

    let mut env = HashMap::new();
    env.insert("TERM_PROGRAM".to_string(), "mandelbot".to_string());

    if shell_command.contains("zsh") {
        let path = dir.join("mandelbot.zsh");
        if !path.exists() {
            std::fs::write(&path, SHELL_INTEGRATION_ZSH).expect("failed to write zsh integration");
        }
        // ZDOTDIR trick: create a .zshrc that sources the user's real config then ours.
        let zdotdir = dir.join("zdotdir");
        std::fs::create_dir_all(&zdotdir).expect("failed to create zdotdir");
        let user_home = std::env::var("HOME").unwrap_or_default();
        let zshrc = zdotdir.join(".zshrc");
        let content = format!(
            "[ -f \"{user_home}/.zshenv\" ] && source \"{user_home}/.zshenv\"\n\
             [ -f \"{user_home}/.zshrc\" ] && source \"{user_home}/.zshrc\"\n\
             source \"{}\"\n",
            path.to_string_lossy()
        );
        std::fs::write(&zshrc, content).expect("failed to write zdotdir .zshrc");
        // Also create .zshenv to prevent double-sourcing of /etc/zshenv via ZDOTDIR
        let zshenv = zdotdir.join(".zshenv");
        if !zshenv.exists() {
            std::fs::write(&zshenv, "").expect("failed to write zdotdir .zshenv");
        }
        env.insert("ZDOTDIR".to_string(), zdotdir.to_string_lossy().into_owned());
    } else if shell_command.contains("bash") {
        let path = dir.join("mandelbot.bash");
        if !path.exists() {
            std::fs::write(&path, SHELL_INTEGRATION_BASH)
                .expect("failed to write bash integration");
        }
        // For bash, use --rcfile or ENV. We'll set ENV for non-login shells.
        // Since we source user's bashrc too, write a wrapper.
        let wrapper = dir.join("bashrc_wrapper");
        let user_home = std::env::var("HOME").unwrap_or_default();
        let content = format!(
            "[ -f \"{user_home}/.bashrc\" ] && source \"{user_home}/.bashrc\"\n\
             source \"{}\"\n",
            path.to_string_lossy()
        );
        std::fs::write(&wrapper, content).expect("failed to write bash wrapper");
        env.insert("ENV".to_string(), wrapper.to_string_lossy().into_owned());
    }

    env
}

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

fn write_plugin_dir(dir: &Path) -> PathBuf {
    let plugin_dir = dir.join("plugins");

    let delegate_dir = plugin_dir.join("skills").join("mandelbot-delegate");
    std::fs::create_dir_all(&delegate_dir).expect("failed to create mandelbot-delegate skill dir");

    let subtask_dir = plugin_dir.join("skills").join("mandelbot-work-as-subtask");
    std::fs::create_dir_all(&subtask_dir).expect("failed to create mandelbot-work-as-subtask skill dir");

    let config_dir = plugin_dir.join("skills").join("mandelbot-config");
    std::fs::create_dir_all(&config_dir).expect("failed to create mandelbot-config skill dir");

    let keybindings_dir = plugin_dir.join("skills").join("mandelbot-keybindings");
    std::fs::create_dir_all(&keybindings_dir)
        .expect("failed to create mandelbot-keybindings skill dir");

    let skill_path = delegate_dir.join("SKILL.md");
    if !skill_path.exists() {
        std::fs::write(&skill_path, SKILL_DELEGATE).expect("failed to write delegate skill");
        std::fs::write(delegate_dir.join("template.md"), SKILL_DELEGATE_TEMPLATE)
            .expect("failed to write delegate template");
        std::fs::write(delegate_dir.join("watch.sh"), SKILL_DELEGATE_WATCH)
            .expect("failed to write delegate watch script");
    }

    let skill_path = subtask_dir.join("SKILL.md");
    if !skill_path.exists() {
        std::fs::write(&skill_path, SKILL_WORK_AS_SUBTASK)
            .expect("failed to write work-as-subtask skill");
    }

    let skill_path = config_dir.join("SKILL.md");
    if !skill_path.exists() {
        std::fs::write(&skill_path, SKILL_MANDELBOT_CONFIG)
            .expect("failed to write mandelbot-config skill");
    }

    let skill_path = keybindings_dir.join("SKILL.md");
    if !skill_path.exists() {
        std::fs::write(&skill_path, SKILL_MANDELBOT_KEYBINDINGS)
            .expect("failed to write mandelbot-keybindings skill");
    }

    plugin_dir
}

/// Read status updates from a FIFO and emit `SetStatus` messages.
/// Opens the FIFO with O_RDWR to avoid EOF when writers close.
pub fn fifo_stream(tab_id: usize, fifo_path: PathBuf) -> impl iced::futures::Stream<Item = Message> {
    iced::stream::channel(
        16,
        move |mut sender: iced::futures::channel::mpsc::Sender<Message>| async move {
            let (exit_sender, exit_receiver) = iced::futures::channel::oneshot::channel::<()>();

            std::thread::spawn(move || {
                // Open O_RDWR so the read side stays open even when no writers
                // are connected (avoids repeated EOF).
                let file = match std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&fifo_path)
                {
                    Ok(f) => f,
                    Err(_) => return,
                };
                let reader = std::io::BufReader::new(file);
                for line in reader.lines() {
                    let Ok(line) = line else { break };
                    let status = line.trim().to_string();
                    if let Some(s) = AgentStatus::from_str(&status) {
                        if futures::executor::block_on(sender.send(Message::SetStatus(tab_id, s))).is_err() {
                            break;
                        }
                    }
                }
                let _ = exit_sender.send(());
            });

            let _ = exit_receiver.await;
        },
    )
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
                            if futures::executor::block_on(sender.send(Message::TerminalOutput(tab_id, bytes))).is_err() {
                                break;
                            }
                        }
                    }
                }
                let _ = futures::executor::block_on(sender.send(Message::ShellExited(tab_id)));
                let _ = exit_sender.send(());
            });

            let _ = exit_receiver.await;
        },
    )
}

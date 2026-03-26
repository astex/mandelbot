use std::collections::HashMap;
use std::path::Path;

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

/// Augment PATH with common directories that may not be present when launched
/// from Finder (which doesn't source shell profiles).
fn augmented_path() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/unknown".to_string());
    let extra_dirs = [
        format!("{home}/.local/bin"),
        format!("{home}/.cargo/bin"),
        "/opt/homebrew/bin".to_string(),
        "/usr/local/bin".to_string(),
    ];
    let current = std::env::var("PATH").unwrap_or_default();
    let mut parts: Vec<&str> = current.split(':').collect();
    for dir in &extra_dirs {
        if !parts.contains(&dir.as_str()) {
            parts.insert(0, dir);
        }
    }
    parts.join(":")
}

pub struct ShellConfig<'a> {
    pub command: &'a str,
    pub args: &'a [&'a str],
    pub env: HashMap<String, String>,
    pub cwd: Option<&'a Path>,
    pub rows: u16,
    pub cols: u16,
}

pub fn spawn_shell(
    config: &ShellConfig,
) -> Result<
    (
        Box<dyn MasterPty + Send>,
        Box<dyn Child + Send + Sync>,
    ),
    Box<dyn std::error::Error>,
> {
    let pty_system = native_pty_system();
    let pair = pty_system.openpty(PtySize {
        rows: config.rows,
        cols: config.cols,
        pixel_width: 0,
        pixel_height: 0,
    })?;

    let mut cmd = CommandBuilder::new(config.command);
    cmd.args(config.args);
    cmd.env("TERM", "xterm-256color");
    cmd.env("PROMPT_EOL_MARK", "");
    cmd.env("PATH", augmented_path());
    for (k, v) in &config.env {
        cmd.env(k, v);
    }
    if let Some(cwd) = config.cwd {
        cmd.cwd(cwd);
    }

    let child = pair.slave.spawn_command(cmd)?;
    drop(pair.slave);

    Ok((pair.master, child))
}

/// Shell-escape a string by wrapping it in single quotes.
pub fn shell_quote(s: &str) -> String {
    // Replace any embedded single quotes with the sequence: '\''
    // (end single-quote, escaped literal quote, restart single-quote)
    format!("'{}'", s.replace('\'', "'\\''"))
}

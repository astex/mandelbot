use std::collections::HashMap;
use std::path::Path;

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

pub struct ShellConfig<'a> {
    pub command: &'a str,
    pub args: &'a [&'a str],
    pub env: HashMap<&'a str, String>,
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

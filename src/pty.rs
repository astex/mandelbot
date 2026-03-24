use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

pub fn spawn_shell(
    shell: &str,
    args: &[&str],
    env: &[(&str, &str)],
    cwd: Option<&str>,
    rows: u16,
    cols: u16,
) -> Result<
    (
        Box<dyn MasterPty + Send>,
        Box<dyn Child + Send + Sync>,
    ),
    Box<dyn std::error::Error>,
> {
    let pty_system = native_pty_system();
    let pair = pty_system.openpty(PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    })?;

    let mut cmd = CommandBuilder::new(shell);
    for arg in args {
        cmd.arg(arg);
    }
    for (key, val) in env {
        cmd.env(key, val);
    }
    if let Some(dir) = cwd {
        cmd.cwd(dir);
    }
    cmd.env("TERM", "xterm-256color");
    cmd.env("PROMPT_EOL_MARK", "");

    let child = pair.slave.spawn_command(cmd)?;
    drop(pair.slave);

    Ok((pair.master, child))
}

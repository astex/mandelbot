use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::io::{Read, Write};

pub struct PtyHandle {
    _child: Box<dyn portable_pty::Child + Send + Sync>,
    reader: Box<dyn Read + Send>,
    writer: Box<dyn Write + Send>,
}

impl PtyHandle {
    pub fn spawn(shell: &str, rows: u16, cols: u16) -> Result<Self, Box<dyn std::error::Error>> {
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let mut cmd = CommandBuilder::new(shell);
        cmd.env("TERM", "dumb");
        cmd.env("PROMPT_EOL_MARK", "");

        let child = pair.slave.spawn_command(cmd)?;
        drop(pair.slave);

        let reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;

        Ok(Self {
            _child: child,
            reader,
            writer,
        })
    }

    pub fn take_reader(&mut self) -> Box<dyn Read + Send> {
        std::mem::replace(&mut self.reader, Box::new(std::io::empty()))
    }

    pub fn take_writer(&mut self) -> Box<dyn Write + Send> {
        std::mem::replace(&mut self.writer, Box::new(std::io::sink()))
    }
}

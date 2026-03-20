use std::sync::{Arc, Mutex};

pub struct TerminalBuffer {
    lines: Vec<String>,
    cursor_row: usize,
    cursor_col: usize,
    rows: usize,
    cols: usize,
}

impl TerminalBuffer {
    pub fn new(rows: usize, cols: usize) -> Self {
        let mut lines = Vec::with_capacity(rows);
        lines.push(String::new());
        Self {
            lines,
            cursor_row: 0,
            cursor_col: 0,
            rows,
            cols,
        }
    }

    pub fn feed(&mut self, data: &[u8]) {
        let mut i = 0;
        while i < data.len() {
            let b = data[i];
            match b {
                // ESC — skip escape sequences
                0x1b => {
                    i += 1;
                    if i < data.len() && data[i] == b'[' {
                        i += 1;
                        while i < data.len() && !(data[i] as char).is_ascii_alphabetic() {
                            i += 1;
                        }
                        i += 1; // skip final char
                    }
                }
                b'\n' => {
                    self.cursor_row += 1;
                    if self.cursor_row >= self.rows {
                        if self.lines.len() > 1 {
                            self.lines.remove(0);
                        }
                        self.cursor_row = self.rows - 1;
                    }
                    while self.lines.len() <= self.cursor_row {
                        self.lines.push(String::new());
                    }
                    i += 1;
                }
                b'\r' => {
                    self.cursor_col = 0;
                    i += 1;
                }
                0x08 => {
                    // backspace
                    if self.cursor_col > 0 {
                        self.cursor_col -= 1;
                    }
                    i += 1;
                }
                b if b >= 0x20 => {
                    // printable
                    while self.lines.len() <= self.cursor_row {
                        self.lines.push(String::new());
                    }
                    let line = &mut self.lines[self.cursor_row];
                    let ch = b as char;
                    if self.cursor_col < line.len() {
                        line.replace_range(self.cursor_col..self.cursor_col + 1, &ch.to_string());
                    } else {
                        while line.len() < self.cursor_col {
                            line.push(' ');
                        }
                        line.push(ch);
                    }
                    self.cursor_col += 1;
                    if self.cursor_col >= self.cols {
                        self.cursor_col = 0;
                        self.cursor_row += 1;
                        if self.cursor_row >= self.rows {
                            if self.lines.len() > 1 {
                                self.lines.remove(0);
                            }
                            self.cursor_row = self.rows - 1;
                        }
                        while self.lines.len() <= self.cursor_row {
                            self.lines.push(String::new());
                        }
                    }
                    i += 1;
                }
                _ => {
                    // ignore other control chars
                    i += 1;
                }
            }
        }
    }

    pub fn screen_text(&self) -> String {
        let start = self.lines.iter().position(|l| !l.trim().is_empty()).unwrap_or(0);
        self.lines[start..].join("\n")
    }
}

pub type SharedBuffer = Arc<Mutex<TerminalBuffer>>;

pub fn new_shared(rows: usize, cols: usize) -> SharedBuffer {
    Arc::new(Mutex::new(TerminalBuffer::new(rows, cols)))
}

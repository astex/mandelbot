pub struct TerminalBuffer {
    lines: Vec<String>,
    cursor_row: usize,
    cursor_col: usize,
    rows: usize,
    cols: usize,
    wrap_pending: bool,
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
            wrap_pending: false,
        }
    }

    pub fn feed(&mut self, data: &[u8]) {
        let mut i = 0;
        while i < data.len() {
            let b = data[i];
            match b {
                0x1b => {
                    i += 1;
                    if i < data.len() && data[i] == b'[' {
                        i += 1;
                        // Collect parameter bytes
                        let param_start = i;
                        while i < data.len() && (data[i] as char).is_ascii_digit()
                            || (i < data.len() && data[i] == b';')
                            || (i < data.len() && data[i] == b'?')
                        {
                            i += 1;
                        }
                        let params = &data[param_start..i];
                        // Final byte — the command
                        if i < data.len() {
                            let cmd = data[i];
                            match cmd {
                                b'J' => {
                                    // Erase in display
                                    self.erase_display(params);
                                }
                                b'K' => {
                                    // Erase in line
                                    self.erase_line(params);
                                }
                                _ => {} // ignore other CSI sequences
                            }
                            i += 1;
                        }
                    } else if i < data.len() {
                        // Non-CSI escape — skip one char
                        i += 1;
                    }
                }
                b'\n' => {
                    self.wrap_pending = false;
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
                    self.wrap_pending = false;
                    i += 1;
                }
                0x08 => {
                    if self.cursor_col > 0 {
                        self.cursor_col -= 1;
                    }
                    i += 1;
                }
                b if b >= 0x20 => {
                    // Deferred wrap: only advance row when the next char arrives
                    if self.wrap_pending {
                        self.wrap_pending = false;
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
                        self.wrap_pending = true;
                    }
                    i += 1;
                }
                _ => {
                    i += 1;
                }
            }
        }
    }

    fn erase_display(&mut self, params: &[u8]) {
        let mode = parse_param(params, 0);
        match mode {
            0 => {
                // Clear from cursor to end of screen
                if self.cursor_row < self.lines.len() {
                    self.lines[self.cursor_row].truncate(self.cursor_col);
                }
                self.lines.truncate(self.cursor_row + 1);
            }
            1 => {
                // Clear from start to cursor
                for r in 0..self.cursor_row {
                    if r < self.lines.len() {
                        self.lines[r].clear();
                    }
                }
                if self.cursor_row < self.lines.len() {
                    let line = &mut self.lines[self.cursor_row];
                    let fill: String = " ".repeat(self.cursor_col.min(line.len()));
                    line.replace_range(..self.cursor_col.min(line.len()), &fill);
                }
            }
            2 => {
                // Clear entire screen
                for line in &mut self.lines {
                    line.clear();
                }
                self.cursor_row = 0;
                self.cursor_col = 0;
            }
            _ => {}
        }
    }

    fn erase_line(&mut self, params: &[u8]) {
        let mode = parse_param(params, 0);
        if self.cursor_row >= self.lines.len() {
            return;
        }
        let line = &mut self.lines[self.cursor_row];
        match mode {
            0 => {
                // Clear from cursor to end of line
                line.truncate(self.cursor_col);
            }
            1 => {
                // Clear from start to cursor
                let end = self.cursor_col.min(line.len());
                line.replace_range(..end, &" ".repeat(end));
            }
            2 => {
                // Clear entire line
                line.clear();
                self.cursor_col = 0;
            }
            _ => {}
        }
    }

    pub fn screen_text(&self) -> String {
        self.lines.join("\n")
    }
}

fn parse_param(params: &[u8], default: u8) -> u8 {
    let s: String = params
        .iter()
        .filter(|b| b.is_ascii_digit())
        .map(|&b| b as char)
        .collect();
    s.parse().unwrap_or(default)
}

use vte::{Params, Perform};

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

    pub fn screen_text(&self) -> String {
        self.lines.join("\n")
    }

    fn scroll_up(&mut self) {
        if self.lines.len() > 1 {
            self.lines.remove(0);
        }
        self.cursor_row = self.rows - 1;
    }

    fn ensure_row_exists(&mut self) {
        while self.lines.len() <= self.cursor_row {
            self.lines.push(String::new());
        }
    }

    fn advance_row(&mut self) {
        self.cursor_row += 1;
        if self.cursor_row >= self.rows {
            self.scroll_up();
        }
        self.ensure_row_exists();
    }
}

impl Perform for TerminalBuffer {
    fn print(&mut self, c: char) {
        if self.wrap_pending {
            self.wrap_pending = false;
            self.cursor_col = 0;
            self.advance_row();
        }

        self.ensure_row_exists();
        let line = &mut self.lines[self.cursor_row];
        let s = c.to_string();
        if self.cursor_col < line.len() {
            line.replace_range(self.cursor_col..self.cursor_col + 1, &s);
        } else {
            while line.len() < self.cursor_col {
                line.push(' ');
            }
            line.push(c);
        }
        self.cursor_col += 1;
        if self.cursor_col >= self.cols {
            self.wrap_pending = true;
        }
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' => {
                self.wrap_pending = false;
                self.advance_row();
            }
            b'\r' => {
                self.cursor_col = 0;
                self.wrap_pending = false;
            }
            0x08 => {
                // Backspace
                if self.cursor_col > 0 {
                    self.cursor_col -= 1;
                }
            }
            _ => {}
        }
    }

    fn csi_dispatch(&mut self, params: &Params, _intermediates: &[u8], _ignore: bool, action: char) {
        let first_param = params.iter().next().and_then(|p| p.first().copied()).unwrap_or(0);

        match action {
            'J' => {
                // Erase in display
                match first_param {
                    0 => {
                        if self.cursor_row < self.lines.len() {
                            self.lines[self.cursor_row].truncate(self.cursor_col);
                        }
                        self.lines.truncate(self.cursor_row + 1);
                    }
                    1 => {
                        for r in 0..self.cursor_row {
                            if r < self.lines.len() {
                                self.lines[r].clear();
                            }
                        }
                        if self.cursor_row < self.lines.len() {
                            let line = &mut self.lines[self.cursor_row];
                            let end = self.cursor_col.min(line.len());
                            line.replace_range(..end, &" ".repeat(end));
                        }
                    }
                    2 => {
                        for line in &mut self.lines {
                            line.clear();
                        }
                        self.cursor_row = 0;
                        self.cursor_col = 0;
                    }
                    _ => {}
                }
            }
            'K' => {
                // Erase in line
                if self.cursor_row >= self.lines.len() {
                    return;
                }
                let line = &mut self.lines[self.cursor_row];
                match first_param {
                    0 => line.truncate(self.cursor_col),
                    1 => {
                        let end = self.cursor_col.min(line.len());
                        line.replace_range(..end, &" ".repeat(end));
                    }
                    2 => {
                        line.clear();
                        self.cursor_col = 0;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, _byte: u8) {}
    fn hook(&mut self, _params: &Params, _intermediates: &[u8], _ignore: bool, _action: char) {}
    fn put(&mut self, _byte: u8) {}
    fn unhook(&mut self) {}
    fn osc_dispatch(&mut self, _params: &[&[u8]], _bell_terminated: bool) {}
}

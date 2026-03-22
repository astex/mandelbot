use vte::{Params, Perform};

use crate::escape;

pub struct TerminalBuffer {
    lines: Vec<String>,
    cursor_row: usize,
    cursor_col: usize,
    pub rows: usize,
}

impl TerminalBuffer {
    pub fn new(rows: usize) -> Self {
        Self {
            lines: Vec::new(),
            cursor_row: 0,
            cursor_col: 0,
            rows,
        }
    }

    pub fn screen_text(&self) -> String {
        let start = self.lines.len().saturating_sub(self.rows);
        self.lines[start..].join("\n")
    }

    fn ensure_row_exists(&mut self) {
        while self.lines.len() <= self.cursor_row {
            self.lines.push(String::new());
        }
    }

    fn cursor_byte_offset(&self) -> usize {
        let line = &self.lines[self.cursor_row];
        line.char_indices()
            .nth(self.cursor_col)
            .map_or(line.len(), |x| x.0)
    }

    fn dispatch_erase(&mut self, action: char, param: u16) {
        match (action, param) {
            escape::ERASE_DISPLAY_CURSOR_TO_END => {
                self.ensure_row_exists();
                let byte_offset = self.cursor_byte_offset();
                self.lines[self.cursor_row].truncate(byte_offset);
                self.lines.truncate(self.cursor_row + 1);
            }
            escape::ERASE_DISPLAY_START_TO_CURSOR => {
                self.ensure_row_exists();
                for line in &mut self.lines[..self.cursor_row] {
                    line.clear();
                }
                let byte_offset = self.cursor_byte_offset();
                let line = &mut self.lines[self.cursor_row];
                let end = byte_offset.min(line.len());
                line.replace_range(..end, &" ".repeat(self.cursor_col.min(line.chars().count())));
            }
            escape::ERASE_DISPLAY_ENTIRE => {
                self.lines.clear();
                self.cursor_row = 0;
                self.cursor_col = 0;
            }
            escape::ERASE_LINE_CURSOR_TO_END => {
                self.ensure_row_exists();
                let byte_offset = self.cursor_byte_offset();
                self.lines[self.cursor_row].truncate(byte_offset);
            }
            escape::ERASE_LINE_START_TO_CURSOR => {
                self.ensure_row_exists();
                let byte_offset = self.cursor_byte_offset();
                let line = &mut self.lines[self.cursor_row];
                let end = byte_offset.min(line.len());
                line.replace_range(..end, &" ".repeat(self.cursor_col.min(line.chars().count())));
            }
            escape::ERASE_LINE_ENTIRE => {
                self.ensure_row_exists();
                self.lines[self.cursor_row].clear();
                self.cursor_col = 0;
            }
            _ => {}
        }
    }

    fn advance_row(&mut self) {
        self.cursor_row += 1;
        self.ensure_row_exists();
    }
}

impl Perform for TerminalBuffer {
    fn print(&mut self, c: char) {
        self.ensure_row_exists();
        let line = &mut self.lines[self.cursor_row];
        let char_count = line.chars().count();
        if self.cursor_col < char_count {
            let byte_start = line.char_indices().nth(self.cursor_col).unwrap().0;
            let byte_end = line.char_indices().nth(self.cursor_col + 1).map_or(line.len(), |x| x.0);
            line.replace_range(byte_start..byte_end, &c.to_string());
        } else {
            while line.chars().count() < self.cursor_col {
                line.push(' ');
            }
            line.push(c);
        }
        self.cursor_col += 1;
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\r' => {
                self.cursor_col = 0;
            }
            b'\n' => {
                self.advance_row();
            }
            escape::BACKSPACE => {
                if self.cursor_col > 0 {
                    self.cursor_col -= 1;
                }
            }
            _ => {}
        }
    }

    fn csi_dispatch(&mut self, params: &Params, _intermediates: &[u8], _ignore: bool, action: char) {
        let first_param = params.iter().next().and_then(|p| p.first().copied()).unwrap_or(0);

        let n = (first_param as usize).max(1);

        match action {
            escape::CURSOR_UP => {
                self.cursor_row = self.cursor_row.saturating_sub(n);
            }
            escape::CURSOR_DOWN => {
                self.cursor_row += n;
                self.ensure_row_exists();
            }
            escape::CURSOR_FORWARD => {
                self.cursor_col += n;
            }
            escape::CURSOR_BACK => {
                self.cursor_col = self.cursor_col.saturating_sub(n);
            }
            escape::CURSOR_POSITION => {
                let mut iter = params.iter();
                let row = iter.next().and_then(|p| p.first().copied()).unwrap_or(1).max(1) as usize - 1;
                let col = iter.next().and_then(|p| p.first().copied()).unwrap_or(1).max(1) as usize - 1;
                self.cursor_row = row;
                self.cursor_col = col;
                self.ensure_row_exists();
            }
            _ => {
                self.dispatch_erase(action, first_param);
            }
        }
    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, _byte: u8) {}
    fn hook(&mut self, _params: &Params, _intermediates: &[u8], _ignore: bool, _action: char) {}
    fn put(&mut self, _byte: u8) {}
    fn unhook(&mut self) {}
    fn osc_dispatch(&mut self, _params: &[&[u8]], _bell_terminated: bool) {}
}

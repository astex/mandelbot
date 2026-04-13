use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags;

use super::events::TermInstance;

pub struct LogicalLine {
    pub text: String,
    pub start_line: Line,
    pub cols: usize,
}

impl LogicalLine {
    /// Convert a grid point to a character offset in the logical
    /// line text.
    pub fn char_offset(
        &self,
        line: Line,
        col: usize,
    ) -> usize {
        let row_offset = (self.start_line.0 - line.0) as usize;
        row_offset * self.cols + col
    }

    /// Convert a character offset back to a grid (line, col) pair.
    pub fn grid_position(
        &self,
        char_offset: usize,
    ) -> (Line, usize) {
        let row_offset = char_offset / self.cols;
        let col = char_offset % self.cols;
        (
            Line(self.start_line.0 - row_offset as i32),
            col,
        )
    }
}

/// Extract the logical line containing the given grid line.
pub fn logical_line_at(
    term: &TermInstance,
    line: Line,
) -> LogicalLine {
    let grid = term.grid();
    let cols = grid.columns();
    let topmost = Line(-(grid.history_size() as i32));
    let bottommost = Line(grid.screen_lines() as i32 - 1);

    // Walk backwards to find the first row of this logical line.
    let mut start = line;
    loop {
        let prev = Line(start.0 + 1);
        if prev > bottommost {
            break;
        }
        if grid[prev][Column(cols - 1)]
            .flags
            .contains(Flags::WRAPLINE)
        {
            start = prev;
        } else {
            break;
        }
    }

    // Walk forward collecting text until we find a row without
    // WRAPLINE.
    let mut text = String::new();
    let mut current = start;
    loop {
        for col in 0..cols {
            text.push(grid[current][Column(col)].c);
        }
        if current <= topmost {
            break;
        }
        if grid[current][Column(cols - 1)]
            .flags
            .contains(Flags::WRAPLINE)
        {
            current = Line(current.0 - 1);
        } else {
            break;
        }
    }

    LogicalLine { text, start_line: start, cols }
}

/// Extract the text content of a single grid row, right-trimmed.
fn row_text(term: &TermInstance, line: Line) -> String {
    let grid = term.grid();
    let cols = grid.columns();
    let text: String =
        (0..cols).map(|col| grid[line][Column(col)].c).collect();
    text.trim_end().to_string()
}

#[derive(Default)]
pub(crate) struct PromptInfo {
    pub shell_count: usize,
    pub pr_number: Option<u32>,
}

/// Detect Claude Code's prompt frame and read status indicators
/// (background shell count, tracked PR number) from the lines below
/// it.
pub(crate) fn detect_prompt_info(
    term: &TermInstance,
) -> Option<PromptInfo> {
    let grid = term.grid();
    let screen_lines = grid.screen_lines();
    let cursor_line = grid.cursor.point.line.0;
    let top = (cursor_line - 2).max(0) as usize;
    let bot =
        ((cursor_line + 6) as usize).min(screen_lines - 1);
    let rows: Vec<String> = (top..=bot)
        .map(|i| row_text(term, Line(i as i32)))
        .collect();

    let mut first_border = None;
    let mut second_border = None;
    for (i, text) in rows.iter().enumerate() {
        if is_border_row(text) {
            if first_border.is_none() {
                first_border = Some(i);
            } else {
                second_border = Some(i);
                break;
            }
        }
    }

    let (Some(_top_border), Some(bot_border)) =
        (first_border, second_border)
    else {
        return None;
    };

    let mut info = PromptInfo::default();
    for row in rows.iter().skip(bot_border + 1) {
        if info.shell_count == 0 {
            if let Some(n) = parse_shell_count(row) {
                info.shell_count = n;
            }
        }
        if info.pr_number.is_none() {
            if let Some(n) = parse_pr_number(row) {
                info.pr_number = Some(n);
            }
        }
    }

    Some(info)
}

/// Check if a row looks like a Claude Code prompt border (10+ '─'
/// characters).
fn is_border_row(text: &str) -> bool {
    text.len() >= 10 && text.chars().take(10).all(|c| c == '─')
}

/// Parse a shell count from a line.  Handles both the older
/// "· N shell(s)" format and the current "N shell(s) · …" format.
fn parse_shell_count(text: &str) -> Option<usize> {
    let trimmed = text.trim();

    // Current format: "N shell(s) · ↓ to manage"
    let num_str: String =
        trimmed.chars().take_while(|c| c.is_ascii_digit()).collect();
    if !num_str.is_empty() {
        if let Ok(n) = num_str.parse::<usize>() {
            if trimmed[num_str.len()..]
                .trim_start()
                .starts_with("shell")
            {
                return Some(n);
            }
        }
    }

    // Legacy format: "· N shell(s)"
    let idx = trimmed.find("· ")?;
    let after = &trimmed[idx + "· ".len()..];
    let num_str: String =
        after.chars().take_while(|c| c.is_ascii_digit()).collect();
    let n: usize = num_str.parse().ok()?;
    if after[num_str.len()..].trim_start().starts_with("shell") {
        Some(n)
    } else {
        None
    }
}

/// Parse a tracked PR number from a status line.  Matches the
/// Claude Code `PR #<number>` indicator that appears below the
/// prompt frame.
fn parse_pr_number(text: &str) -> Option<u32> {
    let idx = text.find("PR #")?;
    let after = &text[idx + "PR #".len()..];
    let digits: String =
        after.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pr_number_from_statusline() {
        let line =
            "‣‣ accept edits on (shift+tab to cycle) · PR #28045";
        assert_eq!(parse_pr_number(line), Some(28045));
    }

    #[test]
    fn no_pr_number_when_absent() {
        let line = "‣‣ accept edits on (shift+tab to cycle)";
        assert_eq!(parse_pr_number(line), None);
    }

    #[test]
    fn ignores_pr_without_hash() {
        let line = "PR 123 is cool";
        assert_eq!(parse_pr_number(line), None);
    }
}

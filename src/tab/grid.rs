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

/// Return the row texts that sit below Claude Code's prompt frame,
/// or `None` if the frame isn't on screen.  Used by the per-field
/// scrapers below.
fn prompt_status_rows(term: &TermInstance) -> Option<Vec<String>> {
    let grid = term.grid();
    let screen_lines = grid.screen_lines();
    let cursor_line = grid.cursor.point.line.0;
    let top = (cursor_line - 20).max(0) as usize;
    let bot =
        ((cursor_line + 6) as usize).min(screen_lines - 1);
    let rows: Vec<String> = (top..=bot)
        .map(|i| row_text(term, Line(i as i32)))
        .collect();

    // Walk upward from the cursor so we pick the prompt frame
    // closest to the cursor, not an older one up in scrollback.
    let mut bot_border = None;
    let mut top_border = None;
    for (i, text) in rows.iter().enumerate().rev() {
        if is_border_row(text) {
            if bot_border.is_none() {
                bot_border = Some(i);
            } else {
                top_border = Some(i);
                break;
            }
        }
    }

    let (Some(_top), Some(bot)) = (top_border, bot_border) else {
        return None;
    };

    Some(rows.into_iter().skip(bot + 1).collect())
}

/// Detect Claude Code's prompt frame and read the background task
/// count (shells + monitors).  Returns 0 when the frame is on screen
/// but no count line is visible; returns `None` when the frame isn't
/// on screen.
pub(crate) fn detect_prompt_shell_count(
    term: &TermInstance,
) -> Option<usize> {
    let rows = prompt_status_rows(term)?;
    for row in &rows {
        if let Some(n) = parse_bg_task_count(row) {
            return Some(n);
        }
    }
    Some(0)
}

/// Detect the tracked PR number from Claude Code's status line.
/// Returns `None` if no `PR #N` indicator is visible (either
/// because Claude isn't tracking a PR or the frame isn't on screen).
pub(crate) fn detect_prompt_pr_number(
    term: &TermInstance,
) -> Option<u32> {
    let rows = prompt_status_rows(term)?;
    rows.iter().find_map(|r| parse_pr_number(r))
}

/// Check if a row looks like a Claude Code prompt border (10+ '─'
/// characters).
fn is_border_row(text: &str) -> bool {
    text.len() >= 10 && text.chars().take(10).all(|c| c == '─')
}

/// Parse a background-task count from a status line.  Sums every
/// `<digits> shell` and `<digits> monitor` occurrence on the line, so
/// formats like "N shells · ↓ to manage", "N monitors · …", and
/// "N shells, M monitors · …" all work (as does the legacy
/// "· N shell(s)").  Returns `None` if no such token is found.
fn parse_bg_task_count(text: &str) -> Option<usize> {
    let mut total: usize = 0;
    let mut found = false;
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            let num: usize =
                text[start..i].parse().unwrap_or(0);
            let rest = text[i..].trim_start();
            if rest.starts_with("shell") || rest.starts_with("monitor")
            {
                total += num;
                found = true;
            }
        } else {
            i += 1;
        }
    }
    found.then_some(total)
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

    #[test]
    fn parses_shell_count() {
        assert_eq!(
            parse_bg_task_count("2 shells · ↓ to manage"),
            Some(2),
        );
    }

    #[test]
    fn parses_monitor_count() {
        assert_eq!(
            parse_bg_task_count("  PR #342 · 1 monitor · ↓ to manage"),
            Some(1),
        );
    }

    #[test]
    fn sums_shells_and_monitors() {
        assert_eq!(
            parse_bg_task_count(
                "3 shells, 2 monitors · ↓ to manage"
            ),
            Some(5),
        );
    }

    #[test]
    fn parses_legacy_format() {
        assert_eq!(
            parse_bg_task_count("foo · 4 shells"),
            Some(4),
        );
    }

    #[test]
    fn none_when_no_token() {
        assert_eq!(parse_bg_task_count("PR #42 · ↓ to manage"), None);
    }
}

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags;

use super::events::TermInstance;

pub struct LogicalLine {
    pub text: String,
    pub start_line: Line,
    pub cols: usize,
    /// Per row: leading columns omitted from `text` (the indent of a
    /// row that an app hard-wrapped rather than the terminal).
    skips: Vec<usize>,
}

impl LogicalLine {
    /// Leading columns of the given row that are not part of `text`.
    pub fn row_skip(&self, line: Line) -> usize {
        self.skips[(line.0 - self.start_line.0) as usize]
    }

    /// Char offset in `text` where the given row (0 = first) begins.
    fn row_base(&self, row: usize) -> usize {
        (0..row).map(|r| self.cols - self.skips[r]).sum()
    }

    /// Convert a grid point to a character offset in the logical
    /// line text.  Points inside a skipped indent map to the start
    /// of that row's text.
    pub fn char_offset(
        &self,
        line: Line,
        col: usize,
    ) -> usize {
        let row = (line.0 - self.start_line.0) as usize;
        self.row_base(row) + col.saturating_sub(self.skips[row])
    }

    /// Convert a character offset back to a grid (line, col) pair.
    pub fn grid_position(
        &self,
        char_offset: usize,
    ) -> (Line, usize) {
        let mut base = 0;
        for (row, skip) in self.skips.iter().enumerate() {
            let len = self.cols - skip;
            if char_offset < base + len || row + 1 == self.skips.len() {
                return (
                    Line(self.start_line.0 + row as i32),
                    skip + (char_offset - base),
                );
            }
            base += len;
        }
        (self.start_line, char_offset)
    }
}

/// Whether row `lower` (directly below `upper`) continues the logical
/// line of `upper`.  Returns the number of indent columns to skip on
/// `lower`.  Besides the terminal's own soft wrap (WRAPLINE), this
/// accepts apps that hard-wrap long text with an indent: `upper` is
/// full to its last column and `lower` starts with spaces then text.
fn continuation_skip(
    cols: usize,
    cell: &impl Fn(Line, usize) -> (char, bool),
    upper: Line,
    lower: Line,
) -> Option<usize> {
    let (last, wrapped) = cell(upper, cols - 1);
    if wrapped {
        return Some(0);
    }
    if last.is_whitespace() || !cell(lower, 0).0.is_whitespace() {
        return None;
    }
    let indent = (0..cols)
        .take_while(|&c| cell(lower, c).0.is_whitespace())
        .count();
    (indent < cols).then_some(indent)
}

/// Extract the logical line containing the given grid line.
pub fn logical_line_at(
    term: &TermInstance,
    line: Line,
) -> LogicalLine {
    let grid = term.grid();
    let topmost = Line(-(grid.history_size() as i32));
    let bottommost = Line(grid.screen_lines() as i32 - 1);
    let cell = |l: Line, c: usize| {
        let cell = &grid[l][Column(c)];
        (cell.c, cell.flags.contains(Flags::WRAPLINE))
    };
    build_logical_line(grid.columns(), topmost, bottommost, line, cell)
}

/// Join the rows around `line` into a logical line.  `cell` returns a
/// cell's char and whether it carries WRAPLINE.
fn build_logical_line(
    cols: usize,
    topmost: Line,
    bottommost: Line,
    line: Line,
    cell: impl Fn(Line, usize) -> (char, bool),
) -> LogicalLine {
    // Walk backwards to find the first row of this logical line.
    let mut start = line;
    loop {
        if start <= topmost {
            break;
        }
        let prev = Line(start.0 - 1);
        if continuation_skip(cols, &cell, prev, start).is_none()
        {
            break;
        }
        start = prev;
    }

    // Walk forward collecting text while rows continue.
    let mut text = String::new();
    let mut skips = Vec::new();
    let mut current = start;
    let mut skip = 0;
    loop {
        skips.push(skip);
        for col in skip..cols {
            text.push(cell(current, col).0);
        }
        if current >= bottommost {
            break;
        }
        let next = Line(current.0 + 1);
        match continuation_skip(cols, &cell, current, next) {
            Some(s) => {
                skip = s;
                current = next;
            }
            None => break,
        }
    }

    LogicalLine { text, start_line: start, cols, skips }
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

    /// Build a logical line from screen rows (row 0 = `Line(0)`);
    /// a trailing `\\` on a row marks its last cell WRAPLINE.
    fn logical_from_rows(rows: &[&str], line: i32) -> LogicalLine {
        let cols = rows[0].trim_end_matches('\\').chars().count();
        let grid: Vec<(Vec<char>, bool)> = rows
            .iter()
            .map(|r| {
                let wrapped = r.ends_with('\\');
                let chars: Vec<char> =
                    r.trim_end_matches('\\').chars().collect();
                assert_eq!(chars.len(), cols);
                (chars, wrapped)
            })
            .collect();
        let cell = |l: Line, c: usize| {
            let (chars, wrapped) = &grid[l.0 as usize];
            (chars[c], *wrapped && c == cols - 1)
        };
        build_logical_line(
            cols,
            Line(0),
            Line(rows.len() as i32 - 1),
            Line(line),
            cell,
        )
    }

    const HARD_WRAPPED: &[&str] = &[
        "header    ",
        "see https:",
        "  //x.io/a",
        "  bc      ",
        "footer    ",
    ];

    #[test]
    fn joins_hard_wrapped_indented_rows() {
        for line in 1..=3 {
            let logical = logical_from_rows(HARD_WRAPPED, line);
            assert_eq!(logical.start_line, Line(1));
            assert_eq!(logical.text, "see https://x.io/abc      ");
        }
    }

    #[test]
    fn hard_wrap_offsets_skip_indent() {
        let logical = logical_from_rows(HARD_WRAPPED, 2);
        assert_eq!(logical.row_skip(Line(1)), 0);
        assert_eq!(logical.row_skip(Line(2)), 2);
        assert_eq!(logical.row_skip(Line(3)), 2);

        // "//x.io/a" starts at col 2 of row 2, offset 10.
        assert_eq!(logical.char_offset(Line(2), 2), 10);
        assert_eq!(logical.grid_position(10), (Line(2), 2));
        // Indent cells map to the start of the row's text.
        assert_eq!(logical.char_offset(Line(2), 0), 10);
        // "bc" on row 3.
        assert_eq!(logical.char_offset(Line(3), 2), 18);
        assert_eq!(logical.grid_position(18), (Line(3), 2));

        for off in 0..logical.text.len() {
            let (l, c) = logical.grid_position(off);
            assert_eq!(logical.char_offset(l, c), off);
        }
    }

    #[test]
    fn joins_soft_wrapped_rows() {
        let rows = &["abc\\", "def\\", "gh ", "ijk"];
        let logical = logical_from_rows(rows, 2);
        assert_eq!(logical.start_line, Line(0));
        assert_eq!(logical.text, "abcdefgh ");
        assert_eq!(logical.grid_position(4), (Line(1), 1));
    }

    #[test]
    fn does_not_join_unindented_or_short_rows() {
        let rows = &["abcde", "fghij", "  xy ", "  zz "];
        assert_eq!(logical_from_rows(rows, 1).text, "fghijxy ");
        assert_eq!(logical_from_rows(rows, 0).text, "abcde");
        assert_eq!(logical_from_rows(rows, 3).text, "  zz ");
    }

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

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use iced::widget::{column, container, row, text, Space};
use iced::{Alignment, Border, Color, Element, Font, Length, Theme};

use crate::config::Config;
use crate::tab::{AgentRank, TerminalTab};
use crate::theme::TerminalTheme;
use crate::ui::{Message, TimelineMode};

const MARKER_HEIGHT: f32 = 26.0;
const MARKER_SPACING: f32 = 6.0;
const MARKER_PADDING_X: f32 = 8.0;
const STRIP_ROW_HEIGHT: f32 = MARKER_HEIGHT + MARKER_SPACING;
const STRIP_HEIGHT_MIN: f32 = MARKER_HEIGHT + 12.0;
/// Approximate width of one column slot when we need to left-pad a
/// branch row to align with its fork parent. Slot width = a typical
/// marker (`HH:MM #N` ≈ 9 chars at MONOSPACE) plus the inter-marker
/// spacing. Used only to build leading `Space::with_width`, so exact
/// pixel accuracy is not required — just consistent across rows.
const COLUMN_SLOT_WIDTH: f32 = 72.0;

/// A single row of the timeline DAG: a horizontal run of checkpoints
/// sharing a parent chain. `start_col` is the column-slot index of the
/// first marker (0 = leftmost); each subsequent id sits at `start_col + k`.
#[derive(Debug, Clone, PartialEq)]
pub struct TimelineRow {
    pub start_col: usize,
    pub ids: Vec<usize>,
}

/// Compute row layout for a branching checkpoint history.
///
/// Input is `(id, parent_id)` pairs in any order. `parent_id = None`
/// means the checkpoint is a root; for on-disk records older than the
/// `parent_id` field we fall back to "previous id" as the parent so the
/// pre-branching common case keeps rendering as one flat row.
///
/// Layout rules (match coord spec):
///  - Row 0: start at the earliest root, then always follow the newest
///    (highest-id) child.
///  - Each non-main sibling opens a new row, recursively following the
///    same newest-child rule within that sub-branch.
///  - A marker's column = its parent's column + 1; row 0 starts at 0.
pub fn compute_rows(entries: &[(usize, Option<usize>)]) -> Vec<TimelineRow> {
    if entries.is_empty() {
        return Vec::new();
    }

    let mut sorted_ids: Vec<usize> = entries.iter().map(|(id, _)| *id).collect();
    sorted_ids.sort();

    // Resolve effective parents: `None` on a non-first checkpoint is
    // treated as "previous id" (legacy records with no branching info).
    let mut effective_parent: HashMap<usize, Option<usize>> = HashMap::new();
    for (id, parent) in entries {
        let eff = match parent {
            Some(p) => Some(*p),
            None => {
                let idx = sorted_ids.iter().position(|x| x == id).unwrap();
                if idx == 0 { None } else { Some(sorted_ids[idx - 1]) }
            }
        };
        effective_parent.insert(*id, eff);
    }

    // parent -> sorted children (ascending by id).
    let mut children: HashMap<Option<usize>, Vec<usize>> = HashMap::new();
    for (id, parent) in &effective_parent {
        children.entry(*parent).or_default().push(*id);
    }
    for v in children.values_mut() {
        v.sort();
    }

    let mut col_of: HashMap<usize, usize> = HashMap::new();
    let mut rows: Vec<TimelineRow> = Vec::new();

    // Roots are children of `None`. Usually exactly one; walk in id order
    // so the earliest root lands on row 0.
    let mut roots = children.get(&None).cloned().unwrap_or_default();
    roots.sort();
    for root in roots {
        col_of.insert(root, 0);
        walk(root, &children, &mut col_of, &mut rows, 0);
    }

    rows
}

fn walk(
    start: usize,
    children: &HashMap<Option<usize>, Vec<usize>>,
    col_of: &mut HashMap<usize, usize>,
    rows: &mut Vec<TimelineRow>,
    start_col: usize,
) {
    let mut row_ids = vec![start];
    let mut sub_branches: Vec<(usize, usize)> = Vec::new();
    let mut cur = start;
    loop {
        let cs = children.get(&Some(cur)).cloned().unwrap_or_default();
        if cs.is_empty() {
            break;
        }
        // Most recent child wins the main line.
        let main_child = *cs.iter().max().unwrap();
        let parent_col = *col_of.get(&cur).unwrap();
        for c in &cs {
            if *c != main_child {
                sub_branches.push((*c, parent_col + 1));
            }
        }
        col_of.insert(main_child, parent_col + 1);
        row_ids.push(main_child);
        cur = main_child;
    }
    rows.push(TimelineRow { start_col, ids: row_ids });
    // Order sub-branches by id so the visual stack is stable and roughly
    // chronological within a fork parent.
    sub_branches.sort_by_key(|(id, _)| *id);
    for (sib, col) in sub_branches {
        col_of.insert(sib, col);
        walk(sib, children, col_of, rows, col);
    }
}

/// Locate which row a checkpoint id belongs to. Returns
/// `(row_index, index_within_row)`.
pub fn locate(rows: &[TimelineRow], id: usize) -> Option<(usize, usize)> {
    for (r, row) in rows.iter().enumerate() {
        if let Some(c) = row.ids.iter().position(|x| *x == id) {
            return Some((r, c));
        }
    }
    None
}

pub fn view<'a>(
    tab: &'a TerminalTab,
    config: &'a Config,
    theme: &'a TerminalTheme,
) -> Element<'a, Message> {
    let fg = theme.fg;
    let muted = Color { a: 0.55, ..fg };

    if tab.checkpoints.is_empty() {
        let message = match tab.rank {
            AgentRank::Task => {
                "no checkpoints yet — the Stop hook creates one after each turn"
            }
            AgentRank::Home | AgentRank::Project => {
                "checkpoints are only created in task tabs (claude running in a worktree)"
            }
        };
        let label = text(message)
            .size(config.font_size * 0.85)
            .font(Font::MONOSPACE)
            .color(muted);
        let strip = container(label)
            .padding([6, 10])
            .width(Length::Fill)
            .height(STRIP_HEIGHT_MIN)
            .align_y(Alignment::Center)
            .style(move |_: &Theme| container::Style {
                background: Some(theme.black.into()),
                border: Border { width: 0.0, ..Border::default() },
                ..Default::default()
            });
        return strip.into();
    }

    let entries: Vec<(usize, Option<usize>)> = tab
        .checkpoints
        .iter()
        .map(|c| (c.id, c.parent_id))
        .collect();
    let rows = compute_rows(&entries);

    // Resolve the focused marker. `timeline_cursor` is a checkpoint id;
    // if the id has been pruned away since the cursor was set, fall back
    // to the newest checkpoint on row 0.
    let focused_id = if rows.iter().any(|r| r.ids.contains(&tab.timeline_cursor)) {
        tab.timeline_cursor
    } else {
        *rows[0].ids.last().unwrap()
    };

    let by_id: HashMap<usize, &crate::checkpoint::Checkpoint> =
        tab.checkpoints.iter().map(|c| (c.id, c)).collect();

    let selected_bg = theme.blue;
    let unselected_bg = Color { a: 0.25, ..fg };

    let mut rendered_rows = column![].spacing(MARKER_SPACING);
    for r in &rows {
        let mut line = row![].spacing(MARKER_SPACING).align_y(Alignment::Center);
        if r.start_col > 0 {
            line = line.push(
                Space::new().width(Length::Fixed(r.start_col as f32 * COLUMN_SLOT_WIDTH)),
            );
        }
        for id in &r.ids {
            let Some(ckpt) = by_id.get(id) else { continue };
            let is_focused = *id == focused_id;
            let bg = if is_focused { selected_bg } else { unselected_bg };
            let label_color = if is_focused { theme.bg } else { fg };

            let label_text = format!("{} #{}", format_hhmm(ckpt.created_at), ckpt.id);
            let label = text(label_text)
                .size(config.font_size * 0.85)
                .font(Font::MONOSPACE)
                .color(label_color);

            let marker = container(label)
                .padding([4, MARKER_PADDING_X as u16])
                .height(MARKER_HEIGHT)
                .align_y(Alignment::Center)
                .style(move |_: &Theme| container::Style {
                    background: Some(bg.into()),
                    border: Border {
                        radius: 4.0.into(),
                        width: if is_focused { 1.5 } else { 0.0 },
                        color: fg,
                    },
                    ..Default::default()
                });
            line = line.push(marker);
        }
        rendered_rows = rendered_rows.push(line);
    }

    let (cur_row, cur_idx) = locate(&rows, focused_id).unwrap_or((0, 0));
    let row_size = rows[cur_row].ids.len();
    let hint_text = format!(
        "←/→ scrub   enter: {}   shift+enter: {}   ({} / {} on row {}/{})",
        describe(TimelineMode::Replace),
        describe(TimelineMode::Fork),
        cur_idx + 1,
        row_size,
        cur_row + 1,
        rows.len(),
    );
    let hint = text(hint_text)
        .size(config.font_size * 0.75)
        .font(Font::MONOSPACE)
        .color(muted);

    let strip_height =
        (rows.len() as f32 * STRIP_ROW_HEIGHT + 12.0).max(STRIP_HEIGHT_MIN);
    let content = row![rendered_rows, Space::new().width(Length::Fill), hint]
        .align_y(Alignment::Center)
        .spacing(12);

    container(content)
        .padding([6, 10])
        .width(Length::Fill)
        .height(strip_height)
        .style(move |_: &Theme| container::Style {
            background: Some(theme.black.into()),
            ..Default::default()
        })
        .into()
}

fn describe(mode: TimelineMode) -> &'static str {
    match mode {
        TimelineMode::Replace => "replace",
        TimelineMode::Fork => "fork",
    }
}

fn format_hhmm(t: SystemTime) -> String {
    let secs = t.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let total_minutes = secs / 60;
    let hh = (total_minutes / 60) % 24;
    let mm = total_minutes % 60;
    format!("{hh:02}:{mm:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_empty_input() {
        assert!(compute_rows(&[]).is_empty());
    }

    #[test]
    fn rows_linear_no_branching() {
        // Pre-branching layout — the common case. Single row, start_col 0,
        // ids in chronological order.
        let entries = vec![
            (0, None),
            (1, Some(0)),
            (2, Some(1)),
            (3, Some(2)),
        ];
        let rows = compute_rows(&entries);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].start_col, 0);
        assert_eq!(rows[0].ids, vec![0, 1, 2, 3]);
    }

    #[test]
    fn rows_legacy_all_none_parents_is_linear() {
        // Older on-disk records have `parent_id = None` everywhere. They
        // should still render as one flat row.
        let entries = vec![
            (0, None),
            (1, None),
            (2, None),
        ];
        let rows = compute_rows(&entries);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].ids, vec![0, 1, 2]);
    }

    #[test]
    fn rows_single_branch_from_middle() {
        // 0 - 1 - 2 - 3
        //      \- 4 - 5     (4 branches from 1; main follows highest id)
        let entries = vec![
            (0, None),
            (1, Some(0)),
            (2, Some(1)),
            (3, Some(2)),
            (4, Some(1)),
            (5, Some(4)),
        ];
        let rows = compute_rows(&entries);
        assert_eq!(rows.len(), 2);
        // Row 0 takes the newest child at every fork → #4 wins at #1.
        assert_eq!(rows[0].start_col, 0);
        assert_eq!(rows[0].ids, vec![0, 1, 4, 5]);
        // Row 1 is the older (non-newest) sibling branch from #1; #1 sits
        // at col 1, so the sub-branch starts at col 2.
        assert_eq!(rows[1].start_col, 2);
        assert_eq!(rows[1].ids, vec![2, 3]);
    }

    #[test]
    fn rows_nested_branches() {
        // 0 - 1 - 2 - 3
        //      \- 4 - 5
        //           \- 6
        let entries = vec![
            (0, None),
            (1, Some(0)),
            (2, Some(1)),
            (3, Some(2)),
            (4, Some(1)),
            (5, Some(4)),
            (6, Some(4)),
        ];
        let rows = compute_rows(&entries);
        assert_eq!(rows.len(), 3);
        // Main row walks 0 → 1 → (newest at #1 = 6? no — #6's parent is 4,
        // #4's children are {5, 6}. At #1, children are {2, 4}; newest is
        // 4. Then at 4, children {5, 6}; newest is 6. So main = 0,1,4,6.)
        assert_eq!(rows[0].ids, vec![0, 1, 4, 6]);
    }

    #[test]
    fn locate_finds_row_and_column() {
        let entries = vec![
            (0, None),
            (1, Some(0)),
            (2, Some(1)),
            (3, Some(1)),
        ];
        let rows = compute_rows(&entries);
        // Main: 0,1,3. Branch: 2.
        assert_eq!(locate(&rows, 3), Some((0, 2)));
        assert_eq!(locate(&rows, 2), Some((1, 0)));
        assert_eq!(locate(&rows, 99), None);
    }
}

//! Horizontal checkpoint timeline strip.
//!
//! Renders the focused tab's checkpoint component as a small tree laid
//! out left-to-right: the root on the left, forks branching downward.
//!
//! # The model
//!
//! Each checkpoint represents "everything up to this point" — the full
//! cumulative state of the conversation at the moment it was saved.
//! HEAD is just one more such point: "everything up to now", not yet
//! materialized as a commit. It's semantically continuous with the
//! tree, not a different kind of thing.
//!
//! That's why HEAD gets its own node in the layout instead of being
//! implicit in the tip. When you open the strip during a live
//! conversation, you really are on HEAD — not on the last saved
//! checkpoint, which is one cumulative step behind.
//!
//! # Terminology
//!
//! - *tip* — the tab's last-saved checkpoint (`ckpt_store.head_of(...)`).
//!   This is the tree node that the next checkpoint will parent on.
//! - *HEAD* — a virtual node past the tip, representing current live
//!   state. Drawn but not a real checkpoint: replace/fork are no-ops
//!   because there's no commit to point to.
//!
//! The cursor (what Enter / Shift+Enter activates) starts on HEAD when
//! the strip opens, since that's where the user actually is. Arrow
//! keys walk the tree; HEAD participates in navigation like any other
//! node (it's the newest sibling under the tip).

use std::collections::HashMap;

use iced::widget::{column, container, row, text, Space};
use iced::{keyboard, Alignment, Border, Color, Element, Font, Theme};

use crate::checkpoint_store::{CheckpointId, CheckpointNode, CheckpointStore};
use crate::config::Config;
use crate::tab::TerminalTab;
use crate::theme::TerminalTheme;
use crate::ui::{Message, TimelineDir, TimelineMode};

/// Short id used in labels like `[#abcd]`.
const ID_LEN: usize = 4;

/// "[#abcd]" and "[#HEAD]" are both 7 chars.
const NODE_CHARS: usize = ID_LEN + 3;

/// Sentinel id used internally for the virtual HEAD node. Real
/// checkpoint ids are UUIDs, so this never collides.
const HEAD_KEY: &str = "__HEAD__";

pub fn view<'a>(
    store: &'a CheckpointStore,
    tab: &'a TerminalTab,
    theme: &'a TerminalTheme,
    config: &'a Config,
) -> Element<'a, Message> {
    let tip_id = store.head_of(&tab.uuid).cloned();

    let Some(tip) = tip_id.clone() else {
        return placeholder("no checkpoints yet", theme, config);
    };
    let Some(root_id) = store.root_of(&tip) else {
        return placeholder("no checkpoints yet", theme, config);
    };

    let component = collect_component(store, &root_id, tip_id.as_deref());
    if component.is_empty() {
        return placeholder("no checkpoints yet", theme, config);
    }

    let (positions, rows, cols) = layout(&component, &root_id);
    let grid = build_grid(&component, &positions, rows, cols);
    let cursor = cursor_target(tab);

    let mut grid_rows = column![];
    for grid_row in &grid {
        let mut r = row![].align_y(Alignment::Center);
        for (c, cell) in grid_row.iter().enumerate() {
            r = r.push(render_cell(cell, c, &cursor, tip_id.as_deref(), theme, config));
        }
        grid_rows = grid_rows.push(r);
    }

    let hint_color = dim_fg(theme);
    let hint = match &cursor {
        CursorTarget::Head => {
            "[HEAD: live state]   ←→↑↓ scrub · <esc> close".to_string()
        }
        CursorTarget::Checkpoint(_) => {
            "←→↑↓ scrub · <enter> replace · <shift-enter> fork · <esc> close"
                .to_string()
        }
    };
    let hint_text = text(hint)
        .size(config.font_size * 0.85)
        .font(Font::MONOSPACE)
        .color(hint_color);

    let body = column![grid_rows, Space::new().height(4.0), hint_text].padding(8);
    container(body)
        .style(move |_: &Theme| container::Style {
            background: Some(theme.black.into()),
            border: Border {
                color: dim_fg(theme),
                width: 1.0,
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}

fn placeholder<'a>(msg: &str, theme: &TerminalTheme, config: &Config) -> Element<'a, Message> {
    let t = text(msg.to_string())
        .size(config.font_size * 0.9)
        .font(Font::MONOSPACE)
        .color(dim_fg(theme));
    let bg = theme.black;
    let border = dim_fg(theme);
    container(t)
        .padding(8)
        .style(move |_: &Theme| container::Style {
            background: Some(bg.into()),
            border: Border {
                color: border,
                width: 1.0,
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}

#[derive(Clone)]
enum Cell {
    Empty,
    Node(CheckpointId),
    Head,
    HLine,
    VLine,
    Branch,
}

/// Where the cursor is pointing. `Head` is the virtual live-state node;
/// `Checkpoint` is a real saved checkpoint.
pub enum CursorTarget {
    Head,
    Checkpoint(CheckpointId),
}

/// Every node in the same component as `root_id`, plus a synthetic HEAD
/// attached under the tab's tip (if the tip is inside this component).
struct Component {
    /// Real checkpoint nodes. HEAD is intentionally *not* in here.
    nodes: HashMap<CheckpointId, CheckpointNode>,
    /// parent → ordered children. May contain `HEAD_KEY` as the last
    /// child of `tip_id`.
    children: HashMap<CheckpointId, Vec<CheckpointId>>,
    /// If HEAD was injected, its parent id. Used when drawing its edge.
    head_parent: Option<CheckpointId>,
}

impl Component {
    fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

fn collect_component(
    store: &CheckpointStore,
    root_id: &str,
    tip_id: Option<&str>,
) -> Component {
    let mut nodes: HashMap<CheckpointId, CheckpointNode> = HashMap::new();
    for node in store.nodes.values() {
        if store.root_of(&node.id).as_deref() == Some(root_id) {
            nodes.insert(node.id.clone(), node.clone());
        }
    }
    let mut children: HashMap<CheckpointId, Vec<CheckpointId>> = HashMap::new();
    for node in nodes.values() {
        if let Some(p) = &node.parent {
            children.entry(p.clone()).or_default().push(node.id.clone());
        }
    }
    for kids in children.values_mut() {
        kids.sort_by_key(|id| nodes.get(id).map(|n| n.created_at));
    }

    // Attach HEAD as the newest (last) child of the tab's tip, if the
    // tip lives in this component. HEAD always sorts last so it never
    // hijacks the "first child stays on the parent's row" slot from a
    // real checkpoint.
    let mut head_parent = None;
    if let Some(tip) = tip_id {
        if nodes.contains_key(tip) {
            children.entry(tip.to_string()).or_default().push(HEAD_KEY.into());
            head_parent = Some(tip.to_string());
        }
    }

    Component { nodes, children, head_parent }
}

/// DFS layout: first child keeps parent's row, subsequent children get
/// fresh rows at the bottom. Column == depth from root.
fn layout(
    comp: &Component,
    root_id: &str,
) -> (HashMap<CheckpointId, (usize, usize)>, usize, usize) {
    let mut positions: HashMap<CheckpointId, (usize, usize)> = HashMap::new();
    let mut next_row: usize = 0;
    visit(comp, root_id, 0, 0, &mut positions, &mut next_row);
    let rows = next_row + 1;
    let cols = positions.values().map(|(_, c)| *c).max().unwrap_or(0) + 1;
    (positions, rows, cols)
}

fn visit(
    comp: &Component,
    id: &str,
    depth: usize,
    row: usize,
    positions: &mut HashMap<CheckpointId, (usize, usize)>,
    next_row: &mut usize,
) {
    positions.insert(id.to_string(), (row, depth));
    let Some(kids) = comp.children.get(id) else {
        return;
    };
    for (i, kid) in kids.iter().enumerate() {
        if i == 0 {
            visit(comp, kid, depth + 1, row, positions, next_row);
        } else {
            *next_row += 1;
            let new_row = *next_row;
            visit(comp, kid, depth + 1, new_row, positions, next_row);
        }
    }
}

/// Grid cols are 2*cols - 1: even = node slots (wide), odd = connector slots (1 char).
fn build_grid(
    comp: &Component,
    positions: &HashMap<CheckpointId, (usize, usize)>,
    rows: usize,
    cols: usize,
) -> Vec<Vec<Cell>> {
    let grid_cols = if cols == 0 { 0 } else { 2 * cols - 1 };
    let mut grid = vec![vec![Cell::Empty; grid_cols]; rows];

    for (id, (r, c)) in positions {
        grid[*r][2 * c] = if id == HEAD_KEY {
            Cell::Head
        } else {
            Cell::Node(id.clone())
        };
    }

    // Draw edges by iterating parent→child pairs. This handles HEAD
    // uniformly since it's already in `comp.children[tip]`.
    for (parent_id, kids) in &comp.children {
        let Some(&(pr, pc)) = positions.get(parent_id) else {
            continue;
        };
        for kid_id in kids {
            let Some(&(cr, cc)) = positions.get(kid_id) else {
                continue;
            };
            draw_edge(&mut grid, (pr, pc), (cr, cc));
        }
    }

    // HEAD isn't in `comp.children` under any entry (only as a value of
    // `children[tip_id]`). That case is handled above. But if HEAD's
    // parent didn't end up in the positions map for some reason, skip
    // silently — HEAD still shows, just without a connecting edge.
    let _ = &comp.head_parent;

    grid
}

fn draw_edge(grid: &mut [Vec<Cell>], parent: (usize, usize), child: (usize, usize)) {
    let (pr, pc) = parent;
    let (cr, cc) = child;
    if pr == cr {
        for col in (2 * pc + 1)..(2 * cc) {
            if col % 2 == 1 && matches!(grid[cr][col], Cell::Empty) {
                grid[cr][col] = Cell::HLine;
            }
        }
    } else {
        let branch_col = 2 * cc - 1;
        for row_i in (pr + 1)..cr {
            if matches!(grid[row_i][branch_col], Cell::Empty) {
                grid[row_i][branch_col] = Cell::VLine;
            }
        }
        if matches!(grid[cr][branch_col], Cell::Empty) {
            grid[cr][branch_col] = Cell::Branch;
        }
    }
}

fn render_cell<'a>(
    cell: &Cell,
    col_idx: usize,
    cursor: &CursorTarget,
    tip: Option<&str>,
    theme: &TerminalTheme,
    config: &Config,
) -> Element<'a, Message> {
    let is_node_slot = col_idx % 2 == 0;
    let fg = theme.fg;
    let dim = dim_fg(theme);
    let size = config.font_size;
    match cell {
        Cell::Empty if is_node_slot => {
            text(" ".repeat(NODE_CHARS)).size(size).font(Font::MONOSPACE).into()
        }
        Cell::Empty => text(" ").size(size).font(Font::MONOSPACE).into(),
        Cell::HLine => text("─").size(size).font(Font::MONOSPACE).color(dim).into(),
        Cell::VLine => text("│").size(size).font(Font::MONOSPACE).color(dim).into(),
        Cell::Branch => text("└").size(size).font(Font::MONOSPACE).color(dim).into(),
        Cell::Node(id) => {
            let short = short_id(id);
            let label = format!("[#{}]", short);
            let is_cursor = matches!(cursor, CursorTarget::Checkpoint(c) if c == id);
            let is_tip = tip == Some(id.as_str());

            let (text_color, bg_color, border_color) = if is_cursor {
                (theme.bg, theme.fg, theme.fg)
            } else if is_tip {
                (theme.blue, theme.black, theme.blue)
            } else {
                (fg, theme.black, dim)
            };
            styled_node(label, text_color, bg_color, border_color, is_cursor || is_tip, size)
        }
        Cell::Head => {
            let is_cursor = matches!(cursor, CursorTarget::Head);
            let (text_color, bg_color, border_color) = if is_cursor {
                (theme.bg, theme.green, theme.green)
            } else {
                (theme.green, theme.black, theme.green)
            };
            styled_node("[#HEAD]".to_string(), text_color, bg_color, border_color, true, size)
        }
    }
}

fn styled_node<'a>(
    label: String,
    text_color: Color,
    bg_color: Color,
    border_color: Color,
    has_border: bool,
    size: f32,
) -> Element<'a, Message> {
    let t = text(label).size(size).font(Font::MONOSPACE).color(text_color);
    container(t)
        .padding([0, 0])
        .style(move |_: &Theme| container::Style {
            background: Some(bg_color.into()),
            border: Border {
                color: border_color,
                width: if has_border { 1.0 } else { 0.0 },
                ..Default::default()
            },
            text_color: Some(text_color),
            ..Default::default()
        })
        .into()
}

fn short_id(id: &str) -> String {
    id.chars().take(ID_LEN).collect()
}

fn dim_fg(theme: &TerminalTheme) -> Color {
    let f = theme.fg;
    let b = theme.bg;
    Color::from_rgba(
        (f.r + b.r) * 0.5,
        (f.g + b.g) * 0.5,
        (f.b + b.b) * 0.5,
        1.0,
    )
}

/// Read the tab's cursor field. `None` in storage means "cursor is on
/// the virtual HEAD node" — that's the default when the strip opens.
pub fn cursor_target(tab: &TerminalTab) -> CursorTarget {
    match &tab.timeline_cursor {
        Some(id) => CursorTarget::Checkpoint(id.clone()),
        None => CursorTarget::Head,
    }
}

/// Return the next cursor state after moving `dir`. `None` in the
/// result encodes "cursor is on HEAD"; `Some(id)` encodes a real
/// checkpoint. `None` returned outside `Option` means "no change".
pub fn move_cursor(
    store: &CheckpointStore,
    tab: &TerminalTab,
    dir: TimelineDir,
) -> MoveResult {
    let tip_id = store.head_of(&tab.uuid).cloned();
    match cursor_target(tab) {
        CursorTarget::Head => {
            let Some(tip) = tip_id else {
                return MoveResult::NoChange;
            };
            match dir {
                TimelineDir::Left => MoveResult::Go(Some(tip)),
                // HEAD's siblings are the tip's other children — i.e.
                // fork branches. Sorted oldest-first; HEAD is newest,
                // so Up goes to the last real child.
                TimelineDir::Up => {
                    let mut kids: Vec<&CheckpointNode> = store
                        .nodes
                        .values()
                        .filter(|n| n.parent.as_deref() == Some(tip.as_str()))
                        .collect();
                    kids.sort_by_key(|n| n.created_at);
                    match kids.last() {
                        Some(k) => MoveResult::Go(Some(k.id.clone())),
                        None => MoveResult::NoChange,
                    }
                }
                TimelineDir::Right | TimelineDir::Down => MoveResult::NoChange,
            }
        }
        CursorTarget::Checkpoint(cursor) => {
            let Some(node) = store.node(&cursor) else {
                return MoveResult::NoChange;
            };
            match dir {
                TimelineDir::Left => match node.parent.clone() {
                    Some(p) => MoveResult::Go(Some(p)),
                    None => MoveResult::NoChange,
                },
                TimelineDir::Right => {
                    // If this is the tip and it has no real children,
                    // Right moves onto the virtual HEAD.
                    let is_tip = tip_id.as_deref() == Some(cursor.as_str());
                    let mut kids: Vec<&CheckpointNode> = store
                        .nodes
                        .values()
                        .filter(|n| n.parent.as_deref() == Some(cursor.as_str()))
                        .collect();
                    kids.sort_by_key(|n| n.created_at);
                    if let Some(k) = kids.first() {
                        MoveResult::Go(Some(k.id.clone()))
                    } else if is_tip {
                        MoveResult::Go(None)
                    } else {
                        MoveResult::NoChange
                    }
                }
                TimelineDir::Up | TimelineDir::Down => {
                    let Some(parent_id) = node.parent.as_deref() else {
                        return MoveResult::NoChange;
                    };
                    let mut siblings: Vec<&CheckpointNode> = store
                        .nodes
                        .values()
                        .filter(|n| n.parent.as_deref() == Some(parent_id))
                        .collect();
                    siblings.sort_by_key(|n| n.created_at);
                    let Some(idx) = siblings.iter().position(|n| n.id == cursor) else {
                        return MoveResult::NoChange;
                    };
                    // HEAD is a synthetic "newest sibling" whenever its
                    // parent is the tab's tip. Model it as one more
                    // slot beyond the real siblings so Down past the
                    // last real sibling lands on HEAD.
                    let parent_is_tip = tip_id.as_deref() == Some(parent_id);
                    match dir {
                        TimelineDir::Up if idx > 0 => {
                            MoveResult::Go(Some(siblings[idx - 1].id.clone()))
                        }
                        TimelineDir::Down if idx + 1 < siblings.len() => {
                            MoveResult::Go(Some(siblings[idx + 1].id.clone()))
                        }
                        TimelineDir::Down if idx + 1 == siblings.len() && parent_is_tip => {
                            MoveResult::Go(None)
                        }
                        _ => MoveResult::NoChange,
                    }
                }
            }
        }
    }
}

/// Result of attempting to move the cursor. `Go(None)` = move to HEAD;
/// `Go(Some(id))` = move to that checkpoint; `NoChange` = stay put.
pub enum MoveResult {
    NoChange,
    Go(Option<CheckpointId>),
}

/// Map a key press to a timeline action while the strip is visible.
pub fn handle_key(
    key: &keyboard::Key,
    modifiers: keyboard::Modifiers,
    tab_id: usize,
) -> Option<Message> {
    use keyboard::key::Named;
    match key {
        keyboard::Key::Named(Named::ArrowLeft) if modifiers.is_empty() => {
            Some(Message::TimelineScrub(tab_id, TimelineDir::Left))
        }
        keyboard::Key::Named(Named::ArrowRight) if modifiers.is_empty() => {
            Some(Message::TimelineScrub(tab_id, TimelineDir::Right))
        }
        keyboard::Key::Named(Named::ArrowUp) if modifiers.is_empty() => {
            Some(Message::TimelineScrub(tab_id, TimelineDir::Up))
        }
        keyboard::Key::Named(Named::ArrowDown) if modifiers.is_empty() => {
            Some(Message::TimelineScrub(tab_id, TimelineDir::Down))
        }
        keyboard::Key::Named(Named::Enter) if modifiers.is_empty() => Some(
            Message::TimelineActivate(tab_id, TimelineMode::Replace),
        ),
        keyboard::Key::Named(Named::Enter) if modifiers.shift() => {
            Some(Message::TimelineActivate(tab_id, TimelineMode::Fork))
        }
        keyboard::Key::Named(Named::Escape) if modifiers.is_empty() => {
            Some(Message::ToggleTimeline(tab_id))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{Duration, UNIX_EPOCH};

    fn mk_node(id: &str, parent: Option<&str>, t_offset: u64) -> CheckpointNode {
        CheckpointNode {
            id: id.into(),
            parent: parent.map(|s| s.into()),
            session_id: "s".into(),
            jsonl_line_count: 0,
            shadow_commit: "deadbeef".into(),
            created_at: UNIX_EPOCH + Duration::from_secs(t_offset),
            title: None,
            worktree_dir: PathBuf::from("/tmp/x"),
        }
    }

    fn mk_store(nodes: &[CheckpointNode]) -> CheckpointStore {
        let mut store = CheckpointStore::default();
        for n in nodes {
            store.insert_node(n.clone());
        }
        store
    }

    #[test]
    fn linear_layout_is_one_row_without_head() {
        let store = mk_store(&[
            mk_node("a", None, 1),
            mk_node("b", Some("a"), 2),
            mk_node("c", Some("b"), 3),
        ]);
        // tip=None → no HEAD injected.
        let comp = collect_component(&store, "a", None);
        let (pos, rows, cols) = layout(&comp, "a");
        assert_eq!(rows, 1);
        assert_eq!(cols, 3);
        assert_eq!(pos["a"], (0, 0));
        assert_eq!(pos["b"], (0, 1));
        assert_eq!(pos["c"], (0, 2));
    }

    #[test]
    fn head_appended_as_next_column_when_tip_has_no_children() {
        let store = mk_store(&[
            mk_node("a", None, 1),
            mk_node("b", Some("a"), 2),
            mk_node("c", Some("b"), 3),
        ]);
        let comp = collect_component(&store, "a", Some("c"));
        let (pos, rows, cols) = layout(&comp, "a");
        assert_eq!(rows, 1);
        assert_eq!(cols, 4);
        assert_eq!(pos[HEAD_KEY], (0, 3));
    }

    #[test]
    fn head_goes_below_real_children_of_tip() {
        // tip=b, b already has a real child c (fork).
        // HEAD should land on a new row below c.
        let store = mk_store(&[
            mk_node("a", None, 1),
            mk_node("b", Some("a"), 2),
            mk_node("c", Some("b"), 3),
        ]);
        let comp = collect_component(&store, "a", Some("b"));
        let (pos, rows, cols) = layout(&comp, "a");
        assert_eq!(rows, 2);
        assert_eq!(cols, 3);
        assert_eq!(pos["c"], (0, 2));
        assert_eq!(pos[HEAD_KEY], (1, 2));
    }

    #[test]
    fn branch_sends_second_child_to_new_row() {
        // a -> b -> c
        //      b -> d -> e
        // tip=e so HEAD attaches past e.
        let store = mk_store(&[
            mk_node("a", None, 1),
            mk_node("b", Some("a"), 2),
            mk_node("c", Some("b"), 3),
            mk_node("d", Some("b"), 4),
            mk_node("e", Some("d"), 5),
        ]);
        let comp = collect_component(&store, "a", Some("e"));
        let (pos, rows, _cols) = layout(&comp, "a");
        assert_eq!(rows, 2);
        assert_eq!(pos["a"], (0, 0));
        assert_eq!(pos["b"], (0, 1));
        assert_eq!(pos["c"], (0, 2));
        assert_eq!(pos["d"], (1, 2));
        assert_eq!(pos["e"], (1, 3));
        assert_eq!(pos[HEAD_KEY], (1, 4));
    }

    #[test]
    fn branch_cell_sits_at_child_col_minus_one() {
        let store = mk_store(&[
            mk_node("a", None, 1),
            mk_node("b", Some("a"), 2),
            mk_node("c", Some("b"), 3),
            mk_node("d", Some("b"), 4),
        ]);
        let comp = collect_component(&store, "a", None);
        let (pos, _rows, cols) = layout(&comp, "a");
        let grid = build_grid(&comp, &pos, 2, cols);
        assert!(matches!(grid[1][3], Cell::Branch));
        assert!(matches!(grid[1][4], Cell::Node(ref id) if id == "d"));
    }
}

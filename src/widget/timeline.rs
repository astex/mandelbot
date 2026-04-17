//! Horizontal checkpoint timeline strip.
//!
//! Renders the focused tab's checkpoint component as a tree laid out
//! left-to-right: the root on the left, forks branching downward. Each
//! node is drawn as a three-line rounded box (curved box-drawing
//! corners) so the strip reads as a row of discrete cards rather than
//! a run of text. The tab's *tip* — its current, active checkpoint —
//! gets a distinct "current" highlight. A movable cursor sits on the
//! tip when the strip opens and walks the tree via the arrow keys;
//! Enter replaces to whatever the cursor points at, Shift+Enter forks.
//!
//! The strip fills the full bottom width of the window and is
//! scrollable in both axes when the tree outgrows the available
//! space.

use std::collections::HashMap;

use iced::widget::text::{Rich, Span};
use iced::widget::{column, container, rule, scrollable, text, Space};
use iced::{
    keyboard, widget, Background, Border, Color, Element, Fill, Font, Length, Shadow, Task,
    Theme, Vector,
};

use crate::checkpoint_store::{CheckpointId, CheckpointNode, CheckpointStore};
use crate::config::Config;
use crate::tab::TerminalTab;
use crate::theme::TerminalTheme;
use crate::ui::{Message, TimelineDir, TimelineMode};

/// Inner label width of a node box, in monospace chars. The label
/// renders as `  MM-DD HH:MM  ` (2-space padding on each side) for a
/// roomier, less cramped feel.
const NODE_INNER: usize = 15;
/// Width of a connector cell in monospace chars.
const CONNECTOR_CHARS: usize = 3;
/// Fixed pixel height the strip reserves at the bottom of the window
/// when visible. Content scrolls inside this region if it doesn't fit.
const STRIP_HEIGHT: f32 = 260.0;

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

    let component = collect_component(store, &root_id);
    if component.is_empty() {
        return placeholder("no checkpoints yet", theme, config);
    }

    let (positions, rows, cols) = layout(&component, &root_id);
    let content_grid = build_grid(&component, &positions, rows, cols);
    let cursor_id = effective_cursor(tab, tip_id.as_deref());

    let mut grid_col = column![].spacing(0);
    for (r, grid_row) in content_grid.iter().enumerate() {
        grid_col = grid_col.push(render_content_row(
            grid_row,
            cursor_id.as_deref(),
            tip_id.as_deref(),
            &component,
            theme,
            config,
        ));
        if r + 1 < content_grid.len() {
            grid_col = grid_col.push(render_filler_row(&content_grid[r + 1], theme, config));
        }
    }

    let track_color = Color { a: 0.12, ..theme.fg };
    let thumb_color = Color { a: 0.4, ..theme.fg };
    let thumb_hover = Color { a: 0.7, ..theme.fg };
    let scrolled = scrollable(container(grid_col).padding([0, 16]))
        .id(scroll_id(tab.id))
        .direction(scrollable::Direction::Both {
            vertical: scrollable::Scrollbar::new().width(8).scroller_width(8),
            horizontal: scrollable::Scrollbar::new().width(8).scroller_width(8),
        })
        .style(move |_: &Theme, status| scroll_style(status, track_color, thumb_color, thumb_hover))
        .width(Fill)
        .height(Fill);

    let hint = "←→↑↓ scrub · <enter> replace · <shift-enter> fork · <esc> close";
    let hint_text = text(hint.to_string())
        .size(config.font_size * 0.85)
        .font(Font::MONOSPACE)
        .color(dim_fg(theme));

    strip(
        column![scrolled, Space::new().height(6.0), hint_text]
            .padding([12, 16])
            .spacing(0)
            .into(),
        theme,
    )
}

/// The outer bar: a top divider separating the strip from the
/// terminal above, then the body. No side or bottom borders — the
/// strip spans flush to the window edges.
fn strip<'a>(body: Element<'a, Message>, theme: &TerminalTheme) -> Element<'a, Message> {
    let bg = theme.bg;
    let divider_color = dim_fg(theme);
    container(
        column![
            rule::horizontal(1).style(move |_: &Theme| rule::Style {
                color: divider_color,
                radius: 0.0.into(),
                fill_mode: rule::FillMode::Full,
                snap: true,
            }),
            container(body).width(Fill).height(Fill),
        ]
        .spacing(0),
    )
    .width(Fill)
    .height(Length::Fixed(STRIP_HEIGHT))
    .style(move |_: &Theme| container::Style {
        background: Some(bg.into()),
        text_color: Some(divider_color),
        ..Default::default()
    })
    .into()
}

fn placeholder<'a>(msg: &str, theme: &TerminalTheme, config: &Config) -> Element<'a, Message> {
    let t = text(msg.to_string())
        .size(config.font_size * 0.9)
        .font(Font::MONOSPACE)
        .color(dim_fg(theme));
    strip(container(t).padding([12, 16]).into(), theme)
}

#[derive(Clone)]
enum Cell {
    Empty,
    Node(CheckpointId),
    HLine,
    VLine,
    Tee,
    Fork,
    Branch,
}

/// Nodes and parent→children adjacency for one connected component.
struct Component {
    nodes: HashMap<CheckpointId, CheckpointNode>,
    children: HashMap<CheckpointId, Vec<CheckpointId>>,
}

impl Component {
    fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

fn collect_component(store: &CheckpointStore, root_id: &str) -> Component {
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
    Component { nodes, children }
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

/// Grid cols are 2*cols - 1: even = node slots, odd = connector slots.
fn build_grid(
    comp: &Component,
    positions: &HashMap<CheckpointId, (usize, usize)>,
    rows: usize,
    cols: usize,
) -> Vec<Vec<Cell>> {
    let grid_cols = if cols == 0 { 0 } else { 2 * cols - 1 };
    let mut grid = vec![vec![Cell::Empty; grid_cols]; rows];

    for (id, (r, c)) in positions {
        grid[*r][2 * c] = Cell::Node(id.clone());
    }

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
        if matches!(grid[pr][branch_col], Cell::HLine) {
            grid[pr][branch_col] = Cell::Tee;
        }
        for row_i in (pr + 1)..cr {
            match grid[row_i][branch_col] {
                Cell::Empty => grid[row_i][branch_col] = Cell::VLine,
                Cell::Branch => grid[row_i][branch_col] = Cell::Fork,
                _ => {}
            }
        }
        match grid[cr][branch_col] {
            Cell::Empty => grid[cr][branch_col] = Cell::Branch,
            Cell::VLine => grid[cr][branch_col] = Cell::Fork,
            _ => {}
        }
    }
}

/// Render one full content row as three `rich_text` widgets (top,
/// middle, bottom). Each line is a single text widget with colored
/// spans per cell — rendering as one text avoids sub-pixel gaps
/// between adjacent cells that plagued the per-widget approach.
fn render_content_row<'a>(
    grid_row: &[Cell],
    cursor: Option<&str>,
    tip: Option<&str>,
    component: &Component,
    theme: &TerminalTheme,
    config: &Config,
) -> Element<'a, Message> {
    let size = config.font_size;
    let dim = dim_fg(theme);
    let blue = theme.blue;
    let fg = theme.fg;
    let bg = theme.bg;

    let mut top: Vec<Span<'a, (), Font>> = Vec::new();
    let mut mid: Vec<Span<'a, (), Font>> = Vec::new();
    let mut bot: Vec<Span<'a, (), Font>> = Vec::new();

    for (c, cell) in grid_row.iter().enumerate() {
        let is_node_slot = c % 2 == 0;
        let width = if is_node_slot { NODE_INNER + 2 } else { CONNECTOR_CHARS };
        let blank = " ".repeat(width);
        match cell {
            Cell::Empty => {
                top.push(plain_span(blank.clone(), dim));
                mid.push(plain_span(blank.clone(), dim));
                bot.push(plain_span(blank, dim));
            }
            Cell::HLine => {
                top.push(plain_span("   ".into(), dim));
                mid.push(plain_span("───".into(), dim));
                bot.push(plain_span("   ".into(), dim));
            }
            Cell::VLine => {
                top.push(plain_span(" │ ".into(), dim));
                mid.push(plain_span(" │ ".into(), dim));
                bot.push(plain_span(" │ ".into(), dim));
            }
            Cell::Tee => {
                top.push(plain_span("   ".into(), dim));
                mid.push(plain_span("─┬─".into(), dim));
                bot.push(plain_span(" │ ".into(), dim));
            }
            Cell::Fork => {
                top.push(plain_span(" │ ".into(), dim));
                mid.push(plain_span(" ├─".into(), dim));
                bot.push(plain_span(" │ ".into(), dim));
            }
            Cell::Branch => {
                top.push(plain_span(" │ ".into(), dim));
                mid.push(plain_span(" ╰─".into(), dim));
                bot.push(plain_span("   ".into(), dim));
            }
            Cell::Node(id) => {
                let label = component
                    .nodes
                    .get(id)
                    .map(format_label)
                    .unwrap_or_else(|| " ".repeat(NODE_INNER));
                let top_s = format!("╭{}╮", "─".repeat(NODE_INNER));
                let mid_s = format!("│{}│", label);
                let bot_s = format!("╰{}╯", "─".repeat(NODE_INNER));

                let is_cursor = cursor == Some(id.as_str());
                let is_tip = tip == Some(id.as_str());
                let (fg_c, bg_c) = if is_cursor {
                    (bg, Some(fg))
                } else if is_tip {
                    (blue, None)
                } else {
                    (dim, None)
                };
                top.push(node_span(top_s, fg_c, bg_c));
                mid.push(node_span(mid_s, fg_c, bg_c));
                bot.push(node_span(bot_s, fg_c, bg_c));
            }
        }
    }

    column![
        Rich::with_spans(top).size(size).font(Font::MONOSPACE),
        Rich::with_spans(mid).size(size).font(Font::MONOSPACE),
        Rich::with_spans(bot).size(size).font(Font::MONOSPACE),
    ]
    .spacing(0)
    .into()
}

/// Thin single-line spacer row. Carries `│` in any connector column
/// where the row below continues a vertical edge.
fn render_filler_row<'a>(
    below: &[Cell],
    theme: &TerminalTheme,
    config: &Config,
) -> Element<'a, Message> {
    let size = config.font_size;
    let dim = dim_fg(theme);
    let mut spans: Vec<Span<'a, (), Font>> = Vec::new();
    for (c, cell_below) in below.iter().enumerate() {
        let is_node_slot = c % 2 == 0;
        let width = if is_node_slot { NODE_INNER + 2 } else { CONNECTOR_CHARS };
        let has_v = matches!(cell_below, Cell::VLine | Cell::Branch | Cell::Fork);
        let s = if has_v && !is_node_slot {
            " │ ".to_string()
        } else {
            " ".repeat(width)
        };
        spans.push(plain_span(s, dim));
    }
    Rich::with_spans(spans)
        .size(size)
        .font(Font::MONOSPACE)
        .into()
}

fn plain_span<'a>(s: String, color: Color) -> Span<'a, (), Font> {
    Span::new(s).font(Font::MONOSPACE).color(color)
}

fn node_span<'a>(s: String, color: Color, bg: Option<Color>) -> Span<'a, (), Font> {
    let mut span = Span::new(s).font(Font::MONOSPACE).color(color);
    if let Some(b) = bg {
        span = span.background(Background::Color(b));
    }
    span
}

/// Format the node label padded to `NODE_INNER` chars.
fn format_label(node: &CheckpointNode) -> String {
    use chrono::{DateTime, Local};
    let dt: DateTime<Local> = node.created_at.into();
    let stamp = format!("{}", dt.format("%m-%d %H:%M"));
    let pad = NODE_INNER.saturating_sub(stamp.chars().count());
    let left = pad / 2;
    let right = pad - left;
    format!("{}{}{}", " ".repeat(left), stamp, " ".repeat(right))
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

/// The cursor id to render and to activate on Enter. `None` in
/// `tab.timeline_cursor` falls back to the tab's tip (the default
/// when the strip first opens).
pub fn effective_cursor(tab: &TerminalTab, tip: Option<&str>) -> Option<String> {
    tab.timeline_cursor
        .clone()
        .or_else(|| tip.map(|s| s.to_string()))
}

/// How many pixels tall the strip will be. Fixed regardless of tree
/// size — the strip always reserves the same bottom region and
/// scrolls internally when the tree overflows.
pub fn pixel_height(
    _store: &CheckpointStore,
    tab: &TerminalTab,
    _config: &Config,
) -> f32 {
    if !tab.timeline_visible {
        return 0.0;
    }
    STRIP_HEIGHT
}

/// Compute where the cursor lands after a directional press. Returns
/// the new checkpoint id, or `None` to indicate "no change".
pub fn move_cursor(
    store: &CheckpointStore,
    tab: &TerminalTab,
    dir: TimelineDir,
) -> Option<CheckpointId> {
    let tip = store.head_of(&tab.uuid).cloned();
    let cursor = effective_cursor(tab, tip.as_deref())?;
    let node = store.node(&cursor)?;
    match dir {
        TimelineDir::Left => node.parent.clone(),
        TimelineDir::Right => {
            let mut kids: Vec<&CheckpointNode> = store
                .nodes
                .values()
                .filter(|n| n.parent.as_deref() == Some(cursor.as_str()))
                .collect();
            kids.sort_by_key(|n| n.created_at);
            kids.first().map(|k| k.id.clone())
        }
        TimelineDir::Up | TimelineDir::Down => {
            let parent_id = node.parent.as_deref()?;
            let mut siblings: Vec<&CheckpointNode> = store
                .nodes
                .values()
                .filter(|n| n.parent.as_deref() == Some(parent_id))
                .collect();
            siblings.sort_by_key(|n| n.created_at);
            let idx = siblings.iter().position(|n| n.id == cursor)?;
            match dir {
                TimelineDir::Up if idx > 0 => Some(siblings[idx - 1].id.clone()),
                TimelineDir::Down if idx + 1 < siblings.len() => {
                    Some(siblings[idx + 1].id.clone())
                }
                _ => None,
            }
        }
    }
}

/// Map a key press to a timeline action while the strip is visible.
pub fn handle_key(
    key: &keyboard::Key,
    modifiers: keyboard::Modifiers,
    tab_id: usize,
) -> Option<Message> {
    use keyboard::key::Named;
    match key {
        keyboard::Key::Named(Named::ArrowLeft) => {
            Some(Message::TimelineScrub(tab_id, TimelineDir::Left))
        }
        keyboard::Key::Named(Named::ArrowRight) => {
            Some(Message::TimelineScrub(tab_id, TimelineDir::Right))
        }
        keyboard::Key::Named(Named::ArrowUp) => {
            Some(Message::TimelineScrub(tab_id, TimelineDir::Up))
        }
        keyboard::Key::Named(Named::ArrowDown) => {
            Some(Message::TimelineScrub(tab_id, TimelineDir::Down))
        }
        keyboard::Key::Named(Named::Enter) if modifiers.shift() => {
            Some(Message::TimelineActivate(tab_id, TimelineMode::Fork))
        }
        keyboard::Key::Named(Named::Enter) => {
            Some(Message::TimelineActivate(tab_id, TimelineMode::Replace))
        }
        keyboard::Key::Named(Named::Escape) => {
            Some(Message::ToggleTimeline(tab_id))
        }
        _ => None,
    }
}

/// Stable scrollable id for a tab's timeline, so scroll-to
/// operations from the update loop can find the right widget.
pub fn scroll_id(tab_id: usize) -> widget::Id {
    widget::Id::from(format!("timeline-{tab_id}"))
}

fn scroll_style(
    status: scrollable::Status,
    track: Color,
    thumb: Color,
    thumb_hover: Color,
) -> scrollable::Style {
    let radius = 4.0;
    let rail = |scroller_color: Color| scrollable::Rail {
        background: Some(Background::Color(track)),
        border: Border {
            radius: radius.into(),
            ..Border::default()
        },
        scroller: scrollable::Scroller {
            background: Background::Color(scroller_color),
            border: Border {
                radius: radius.into(),
                ..Border::default()
            },
        },
    };
    let (v_color, h_color) = match status {
        scrollable::Status::Hovered {
            is_vertical_scrollbar_hovered,
            is_horizontal_scrollbar_hovered,
            ..
        } => (
            if is_vertical_scrollbar_hovered { thumb_hover } else { thumb },
            if is_horizontal_scrollbar_hovered { thumb_hover } else { thumb },
        ),
        scrollable::Status::Dragged {
            is_vertical_scrollbar_dragged,
            is_horizontal_scrollbar_dragged,
            ..
        } => (
            if is_vertical_scrollbar_dragged { thumb_hover } else { thumb },
            if is_horizontal_scrollbar_dragged { thumb_hover } else { thumb },
        ),
        _ => (thumb, thumb),
    };
    scrollable::Style {
        container: container::Style::default(),
        vertical_rail: rail(v_color),
        horizontal_rail: rail(h_color),
        gap: None,
        auto_scroll: scrollable::AutoScroll {
            background: Background::Color(Color::TRANSPARENT),
            border: Border::default(),
            shadow: Shadow {
                color: Color::TRANSPARENT,
                offset: Vector::ZERO,
                blur_radius: 0.0,
            },
            icon: Color::TRANSPARENT,
        },
    }
}

/// Task that scrolls the timeline strip so the cursor (or tip, if no
/// explicit cursor is set) is visible. Called when the strip opens
/// and after every arrow-key scrub so the cursor stays on-screen as
/// the user walks the tree. The cursor is placed a little left of
/// centre so there's room ahead of the cursor to preview what's
/// coming.
pub fn scroll_to_cursor(
    store: &CheckpointStore,
    tab: &TerminalTab,
    config: &Config,
) -> Task<Message> {
    let tip = store.head_of(&tab.uuid).cloned();
    let Some(target) = effective_cursor(tab, tip.as_deref()) else {
        return Task::none();
    };
    let Some(root_id) = store.root_of(&target) else {
        return Task::none();
    };
    let comp = collect_component(store, &root_id);
    let (positions, _, _) = layout(&comp, &root_id);
    let Some(&(row, depth)) = positions.get(&target) else {
        return Task::none();
    };

    let cw = config.char_width();
    let ch = config.char_height();
    let node_total = (NODE_INNER + 2) as f32;
    let conn = CONNECTOR_CHARS as f32;

    // Content starts after the grid container's 16px left padding.
    let node_left = 16.0 + (depth as f32) * (node_total + conn) * cw;
    // Leave ~100px of lead on the left so the node sits a bit ahead
    // of the viewport's edge — the nodes *after* the cursor stay
    // visible as you scrub rightward.
    let x = (node_left - 100.0).max(0.0);

    // Content rows are 3 lines tall, separated by 1-line fillers, so
    // row `r` starts at (3+1) * r = 4r lines down.
    let row_top = (row as f32) * 4.0 * ch;
    let y = (row_top - 20.0).max(0.0);

    iced::widget::operation::scroll_to(
        scroll_id(tab.id),
        scrollable::AbsoluteOffset { x, y },
    )
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
    fn linear_layout_is_one_row() {
        let store = mk_store(&[
            mk_node("a", None, 1),
            mk_node("b", Some("a"), 2),
            mk_node("c", Some("b"), 3),
        ]);
        let comp = collect_component(&store, "a");
        let (pos, rows, cols) = layout(&comp, "a");
        assert_eq!(rows, 1);
        assert_eq!(cols, 3);
        assert_eq!(pos["a"], (0, 0));
        assert_eq!(pos["b"], (0, 1));
        assert_eq!(pos["c"], (0, 2));
    }

    #[test]
    fn branch_sends_second_child_to_new_row() {
        let store = mk_store(&[
            mk_node("a", None, 1),
            mk_node("b", Some("a"), 2),
            mk_node("c", Some("b"), 3),
            mk_node("d", Some("b"), 4),
            mk_node("e", Some("d"), 5),
        ]);
        let comp = collect_component(&store, "a");
        let (pos, rows, _cols) = layout(&comp, "a");
        assert_eq!(rows, 2);
        assert_eq!(pos["a"], (0, 0));
        assert_eq!(pos["b"], (0, 1));
        assert_eq!(pos["c"], (0, 2));
        assert_eq!(pos["d"], (1, 2));
        assert_eq!(pos["e"], (1, 3));
    }

    #[test]
    fn branch_cell_sits_at_child_col_minus_one() {
        let store = mk_store(&[
            mk_node("a", None, 1),
            mk_node("b", Some("a"), 2),
            mk_node("c", Some("b"), 3),
            mk_node("d", Some("b"), 4),
        ]);
        let comp = collect_component(&store, "a");
        let (pos, _rows, cols) = layout(&comp, "a");
        let grid = build_grid(&comp, &pos, 2, cols);
        assert!(matches!(grid[1][3], Cell::Branch));
        assert!(matches!(grid[1][4], Cell::Node(ref id) if id == "d"));
    }

    #[test]
    fn move_cursor_left_goes_to_parent() {
        let store = mk_store(&[
            mk_node("a", None, 1),
            mk_node("b", Some("a"), 2),
        ]);
        let mut tab = crate::tab::TerminalTab::new(
            1, 24, 80, false, crate::tab::AgentRank::Home, None, None, 0, None,
        );
        tab.timeline_cursor = Some("b".into());
        let uuid = tab.uuid.clone();
        let mut store = store;
        store.set_head(&uuid, "b".into());
        assert_eq!(
            move_cursor(&store, &tab, TimelineDir::Left),
            Some("a".into())
        );
    }
}

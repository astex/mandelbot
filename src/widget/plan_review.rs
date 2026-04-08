use iced::advanced::layout;
use iced::advanced::renderer::{self, Quad};
use iced::advanced::text::{self, Renderer as _};
use iced::advanced::widget::{self, Tree};
use iced::advanced::Renderer as _;
use iced::advanced::{Clipboard, Layout, Shell, Text, Widget};
use iced::border::Border;
use iced::keyboard;
use iced::mouse;
use iced::{Color, Element, Event, Font, Length, Point, Rectangle, Size};

use crate::config::Config;
use crate::markdown::{self, BlockKind, SpanStyle};
use crate::ui::Message;

/// Minimal plain-text plan renderer. Reads cached plan contents off the tab
/// and draws them line-by-line in the monospace metrics used by the terminal.
pub struct PlanReviewWidget<'a> {
    tab_id: usize,
    contents: &'a str,
    review_pending: bool,
    config: &'a Config,
}

impl<'a> PlanReviewWidget<'a> {
    pub fn new(tab_id: usize, contents: &'a str, review_pending: bool, config: &'a Config) -> Self {
        Self { tab_id, contents, review_pending, config }
    }
}

/// Widget tree state — tracks the active mouse selection.
#[derive(Debug, Default, Clone)]
pub struct State {
    pub selection: Option<Selection>,
    pub dragging: bool,
}

/// Mouse selection in markdown-line coordinates. `anchor` is where the drag
/// started; `head` follows the cursor. Both points are `(line_index, char_index)`
/// over the rendered text of the line (markers excluded).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor: (usize, usize),
    pub head: (usize, usize),
}

impl Selection {
    /// Returns `(start, end)` with `start <= end`.
    pub fn ordered(&self) -> ((usize, usize), (usize, usize)) {
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }
}

/// Per-line geometry needed for hit testing and selection highlighting.
/// Computed from the parsed markdown and current widget bounds.
#[derive(Debug, Clone)]
struct LineLayout {
    y: f32,
    height: f32,
    text_x: f32,
    char_width: f32,
    #[allow(dead_code)]
    text: String,
    char_count: usize,
}

impl<'a> Widget<Message, iced::Theme, iced::Renderer> for PlanReviewWidget<'a> {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<State>()
    }

    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(State::default())
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, Length::Fill, Length::Fill)
    }

    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut iced::Renderer,
        _theme: &iced::Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let theme = self.config.terminal_theme();
        let base_size = self.config.font_size;
        let base_line = self.config.char_height();
        let char_w = self.config.char_width();
        let pad_x = char_w * 2.0;
        let indent_step = char_w * 2.0;

        let lines = markdown::parse(self.contents);
        let layouts = compute_layouts(&lines, bounds, self.config);

        let max_y = bounds.y + bounds.height;
        for (line, line_layout) in lines.iter().zip(layouts.iter()) {
            let y = line_layout.y;
            let metrics = line_metrics(&line.block, base_size, base_line);
            if y + metrics.line_height > max_y {
                break;
            }

            let line_x = bounds.x + pad_x + line.indent as f32 * indent_step;
            let avail_w = (bounds.x + bounds.width - pad_x) - line_x;

            // Block-level decoration drawn first so text sits on top.
            match &line.block {
                BlockKind::CodeBlock { lang, .. } => {
                    let bg = code_block_bg(&theme, lang.as_deref());
                    renderer.fill_quad(
                        Quad {
                            bounds: Rectangle::new(
                                Point::new(line_x - char_w / 2.0, y),
                                Size::new(avail_w + char_w, metrics.line_height),
                            ),
                            border: Border::default(),
                            ..Quad::default()
                        },
                        bg,
                    );
                }
                BlockKind::BlockQuote => {
                    renderer.fill_quad(
                        Quad {
                            bounds: Rectangle::new(
                                Point::new(line_x - char_w, y),
                                Size::new(char_w / 2.0, metrics.line_height),
                            ),
                            border: Border::default(),
                            ..Quad::default()
                        },
                        theme.bright_black,
                    );
                }
                BlockKind::HorizontalRule => {
                    let thickness = (base_size / 8.0).max(1.0);
                    renderer.fill_quad(
                        Quad {
                            bounds: Rectangle::new(
                                Point::new(line_x, y + metrics.line_height / 2.0 - thickness / 2.0),
                                Size::new(avail_w, thickness),
                            ),
                            border: Border::default(),
                            ..Quad::default()
                        },
                        theme.bright_black,
                    );
                    continue;
                }
                _ => {}
            }

            // Marker (list bullet / number) drawn just left of the text.
            let mut x = line_x;
            if !line.marker.is_empty() {
                let marker_w = line.marker.chars().count() as f32 * char_w;
                renderer.fill_text(
                    Text {
                        content: line.marker.clone(),
                        bounds: Size::new(marker_w, metrics.line_height),
                        size: metrics.size.into(),
                        line_height: text::LineHeight::Relative(self.config.line_height),
                        font: metrics.font,
                        align_x: iced::alignment::Horizontal::Left.into(),
                        align_y: iced::alignment::Vertical::Top.into(),
                        shaping: text::Shaping::Advanced,
                        wrapping: text::Wrapping::None,
                    },
                    Point::new(x, y),
                    theme.bright_black,
                    bounds,
                );
                x += marker_w;
            }

            // Spans flow left-to-right on this line.
            for span in &line.spans {
                let style = span_font(&line.block, span.style, &metrics);
                let color = span_color(&theme, &line.block, span.style);
                let span_w = span.text.chars().count() as f32 * metrics.char_width;
                renderer.fill_text(
                    Text {
                        content: span.text.clone(),
                        bounds: Size::new(span_w.max(1.0), metrics.line_height),
                        size: metrics.size.into(),
                        line_height: text::LineHeight::Relative(self.config.line_height),
                        font: style,
                        align_x: iced::alignment::Horizontal::Left.into(),
                        align_y: iced::alignment::Vertical::Top.into(),
                        shaping: text::Shaping::Advanced,
                        wrapping: text::Wrapping::None,
                    },
                    Point::new(x, y),
                    color,
                    bounds,
                );
                x += span_w;
            }
        }

        // Draw selection highlight on top of text. A translucent overlay in the
        // theme's foreground color reads clearly against any theme without
        // requiring per-span re-rendering.
        let state = tree.state.downcast_ref::<State>();
        if let Some(sel) = state.selection {
            if !sel.is_empty() {
                let ((s_line, s_char), (e_line, e_char)) = sel.ordered();
                let highlight = Color { a: 0.35, ..theme.fg };
                for (i, ll) in layouts.iter().enumerate() {
                    if i < s_line || i > e_line {
                        continue;
                    }
                    if ll.y + ll.height > max_y {
                        break;
                    }
                    let from = if i == s_line { s_char } else { 0 };
                    let to = if i == e_line { e_char } else { ll.char_count };
                    if to <= from {
                        continue;
                    }
                    let x = ll.text_x + from as f32 * ll.char_width;
                    let w = (to - from) as f32 * ll.char_width;
                    renderer.fill_quad(
                        Quad {
                            bounds: Rectangle::new(
                                Point::new(x, ll.y),
                                Size::new(w, ll.height),
                            ),
                            border: Border::default(),
                            ..Quad::default()
                        },
                        highlight,
                    );
                }
            }
        }
    }

    fn mouse_interaction(
        &self,
        _tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        let bounds = layout.bounds();
        let pos = match cursor {
            mouse::Cursor::Available(p) | mouse::Cursor::Levitating(p) => Some(p),
            mouse::Cursor::Unavailable => None,
        };
        if pos.is_some_and(|p| bounds.contains(p)) {
            mouse::Interaction::Text
        } else {
            mouse::Interaction::default()
        }
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let cursor_pos = match cursor {
            mouse::Cursor::Available(p) | mouse::Cursor::Levitating(p) => Some(p),
            mouse::Cursor::Unavailable => None,
        };

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(pos) = cursor_pos {
                    if bounds.contains(pos) {
                        let lines = markdown::parse(self.contents);
                        let layouts = compute_layouts(&lines, bounds, self.config);
                        let point = hit_test(&layouts, pos);
                        let state = tree.state.downcast_mut::<State>();
                        state.selection = Some(Selection { anchor: point, head: point });
                        state.dragging = true;
                        shell.request_redraw();
                        shell.capture_event();
                        return;
                    }
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                let state = tree.state.downcast_mut::<State>();
                if state.dragging {
                    let lines = markdown::parse(self.contents);
                    let layouts = compute_layouts(&lines, bounds, self.config);
                    let point = hit_test(&layouts, *position);
                    if let Some(sel) = state.selection.as_mut() {
                        if sel.head != point {
                            sel.head = point;
                            shell.request_redraw();
                        }
                    }
                    shell.capture_event();
                    return;
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                let state = tree.state.downcast_mut::<State>();
                if state.dragging {
                    state.dragging = false;
                    // Clear empty selections so a plain click doesn't leave a
                    // zero-width artifact in state.
                    if state.selection.is_some_and(|s| s.is_empty()) {
                        state.selection = None;
                    }
                    shell.request_redraw();
                    shell.capture_event();
                    return;
                }
            }
            _ => {}
        }

        if let Event::Keyboard(keyboard::Event::KeyPressed {
            key,
            modifiers,
            ..
        }) = event
        {
            use keyboard::key::Named;

            let ctx = super::keybindings::KeyContext { active_tab_id: self.tab_id };
            if let Some(msg) = super::keybindings::keybinding_message(self.config, key, *modifiers, &ctx) {
                shell.publish(msg);
                shell.capture_event();
                return;
            }

            // Escape clears an active selection before falling through to
            // review/toggle behavior.
            if matches!(key, keyboard::Key::Named(Named::Escape)) {
                let state = tree.state.downcast_mut::<State>();
                if state.selection.is_some() {
                    state.selection = None;
                    state.dragging = false;
                    shell.request_redraw();
                    shell.capture_event();
                    return;
                }
            }

            if self.review_pending {
                if matches!(key, keyboard::Key::Named(Named::Enter)) {
                    shell.publish(Message::PlanReviewAccept(self.tab_id));
                    shell.capture_event();
                    return;
                }
                if matches!(key, keyboard::Key::Named(Named::Escape)) {
                    shell.publish(Message::PlanReviewReject(self.tab_id));
                    shell.capture_event();
                    return;
                }
            } else if matches!(key, keyboard::Key::Named(Named::Escape)) {
                shell.publish(Message::TogglePlanView(self.tab_id));
                shell.capture_event();
            }
        }
    }
}

impl<'a> From<PlanReviewWidget<'a>> for Element<'a, Message, iced::Theme, iced::Renderer> {
    fn from(widget: PlanReviewWidget<'a>) -> Self {
        Self::new(widget)
    }
}

/// Per-line typographic metrics derived from the block kind.
struct LineMetrics {
    size: f32,
    line_height: f32,
    char_width: f32,
    font: Font,
}

fn line_metrics(block: &BlockKind, base_size: f32, base_line: f32) -> LineMetrics {
    match block {
        BlockKind::Heading(level) => {
            let scale = match level {
                1 => 1.6,
                2 => 1.4,
                3 => 1.25,
                4 => 1.15,
                5 => 1.05,
                _ => 1.0,
            };
            LineMetrics {
                size: base_size * scale,
                line_height: base_line * scale,
                char_width: base_size * 0.6 * scale,
                font: Font { weight: iced::font::Weight::Bold, ..Font::MONOSPACE },
            }
        }
        _ => LineMetrics {
            size: base_size,
            line_height: base_line,
            char_width: base_size * 0.6,
            font: Font::MONOSPACE,
        },
    }
}

fn span_font(block: &BlockKind, style: SpanStyle, metrics: &LineMetrics) -> Font {
    let mut font = metrics.font;
    if matches!(block, BlockKind::Heading(_)) {
        font.weight = iced::font::Weight::Bold;
    }
    if style.bold {
        font.weight = iced::font::Weight::Bold;
    }
    if style.italic {
        font.style = iced::font::Style::Italic;
    }
    font
}

fn span_color(
    theme: &crate::theme::TerminalTheme,
    block: &BlockKind,
    style: SpanStyle,
) -> Color {
    if style.link {
        return theme.blue;
    }
    if style.code {
        return theme.cyan;
    }
    match block {
        BlockKind::Heading(_) => theme.bright_white,
        BlockKind::CodeBlock { .. } => theme.fg,
        BlockKind::BlockQuote => Color { a: 0.85, ..theme.fg },
        _ => theme.fg,
    }
}

/// Background color for a fenced code block. Leaves a clear extension point
/// for custom languages — `mermaid` (and any future special block) can be
/// matched here so PR-N can render them as full-fidelity blocks rather than
/// raw text.
fn code_block_bg(theme: &crate::theme::TerminalTheme, lang: Option<&str>) -> Color {
    match lang {
        Some("mermaid") => {
            // Placeholder until a dedicated mermaid renderer lands. The
            // background tints differently so users can see we recognized it.
            tint(theme.bg, theme.bright_blue, 0.18)
        }
        _ => tint(theme.bg, theme.fg, 0.10),
    }
}

fn tint(base: Color, with: Color, amount: f32) -> Color {
    Color {
        r: base.r * (1.0 - amount) + with.r * amount,
        g: base.g * (1.0 - amount) + with.g * amount,
        b: base.b * (1.0 - amount) + with.b * amount,
        a: 1.0,
    }
}

/// Compute per-line layout for hit testing and selection highlighting. Mirrors
/// the y-advancement done in `draw` so the two stay in sync.
fn compute_layouts(
    lines: &[markdown::RenderedLine],
    bounds: Rectangle,
    config: &Config,
) -> Vec<LineLayout> {
    let base_size = config.font_size;
    let base_line = config.char_height();
    let char_w = config.char_width();
    let pad_x = char_w * 2.0;
    let pad_y = base_line / 2.0;
    let indent_step = char_w * 2.0;

    let mut out = Vec::with_capacity(lines.len());
    let mut y = bounds.y + pad_y;
    for line in lines {
        let metrics = line_metrics(&line.block, base_size, base_line);
        let line_x = bounds.x + pad_x + line.indent as f32 * indent_step;
        let marker_w = line.marker.chars().count() as f32 * char_w;
        let text_x = line_x + marker_w;
        let text: String = line.spans.iter().map(|s| s.text.as_str()).collect();
        let char_count = text.chars().count();
        out.push(LineLayout {
            y,
            height: metrics.line_height,
            text_x,
            char_width: metrics.char_width,
            text,
            char_count,
        });
        y += metrics.line_height;
    }
    out
}

/// Map a pixel position to `(line_index, char_index)`. Clamps in both
/// dimensions so points outside the laid-out content snap to the nearest edge.
fn hit_test(layouts: &[LineLayout], pos: Point) -> (usize, usize) {
    if layouts.is_empty() {
        return (0, 0);
    }
    if pos.y < layouts[0].y {
        return (0, 0);
    }
    let line_idx = layouts
        .iter()
        .position(|l| pos.y < l.y + l.height)
        .unwrap_or(layouts.len() - 1);
    let ll = &layouts[line_idx];
    let dx = pos.x - ll.text_x;
    let char_idx = if dx <= 0.0 || ll.char_width <= 0.0 {
        0
    } else {
        ((dx / ll.char_width).round() as usize).min(ll.char_count)
    };
    (line_idx, char_idx)
}

/// Returns the concatenated text covered by the selection, joining lines with
/// `\n`. Used by PR-6 to populate the comment composer.
#[allow(dead_code)]
pub fn selected_text(state: &State, contents: &str, bounds: Rectangle, config: &Config) -> Option<String> {
    let sel = state.selection?;
    if sel.is_empty() {
        return None;
    }
    let lines = markdown::parse(contents);
    let layouts = compute_layouts(&lines, bounds, config);
    let ((s_line, s_char), (e_line, e_char)) = sel.ordered();
    let mut parts: Vec<String> = Vec::new();
    for (i, ll) in layouts.iter().enumerate() {
        if i < s_line || i > e_line {
            continue;
        }
        let from = if i == s_line { s_char } else { 0 };
        let to = if i == e_line { e_char } else { ll.char_count };
        let segment: String = ll.text.chars().skip(from).take(to.saturating_sub(from)).collect();
        // Skip blank spacer lines so plain-text extraction reads naturally.
        if !segment.is_empty() {
            parts.push(segment);
        }
    }
    Some(parts.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn test_bounds() -> Rectangle {
        Rectangle::new(Point::new(0.0, 0.0), Size::new(800.0, 600.0))
    }

    #[test]
    fn selection_ordered_swaps_when_head_before_anchor() {
        let s = Selection { anchor: (3, 5), head: (1, 2) };
        assert_eq!(s.ordered(), ((1, 2), (3, 5)));
        let s = Selection { anchor: (1, 2), head: (3, 5) };
        assert_eq!(s.ordered(), ((1, 2), (3, 5)));
    }

    #[test]
    fn selection_is_empty_for_zero_width() {
        let s = Selection { anchor: (2, 4), head: (2, 4) };
        assert!(s.is_empty());
    }

    #[test]
    fn hit_test_clamps_above_first_line() {
        let config = Config::default();
        let lines = markdown::parse("hello\nworld\n");
        let layouts = compute_layouts(&lines, test_bounds(), &config);
        let (l, c) = hit_test(&layouts, Point::new(-100.0, -100.0));
        assert_eq!((l, c), (0, 0));
    }

    #[test]
    fn hit_test_picks_line_by_y() {
        let config = Config::default();
        // Each paragraph becomes a separate rendered line; blank-line spacers
        // sit between them.
        let lines = markdown::parse("one\n\ntwo\n\nthree\n");
        let layouts = compute_layouts(&lines, test_bounds(), &config);
        assert!(layouts.len() >= 3);
        // Probe directly inside the middle non-empty line.
        let target = layouts.iter().enumerate().filter(|(_, l)| l.char_count > 0).nth(1).unwrap().0;
        let mid_y = layouts[target].y + layouts[target].height / 2.0;
        let (l, _) = hit_test(&layouts, Point::new(layouts[target].text_x + 1.0, mid_y));
        assert_eq!(l, target);
    }

    #[test]
    fn selected_text_single_line() {
        let config = Config::default();
        let contents = "hello world\n";
        let bounds = test_bounds();
        let mut state = State::default();
        // Select chars 0..5 of line 0.
        state.selection = Some(Selection { anchor: (0, 0), head: (0, 5) });
        let text = selected_text(&state, contents, bounds, &config).unwrap();
        assert_eq!(text, "hello");
    }

    #[test]
    fn selected_text_multi_line() {
        let config = Config::default();
        // Use a bullet list — each item becomes its own rendered line.
        let contents = "- alpha\n- beta\n- gamma\n";
        let bounds = test_bounds();
        let lines = markdown::parse(contents);
        let layouts = compute_layouts(&lines, bounds, &config);
        // Find the indices of the three non-empty rendered lines.
        let item_idxs: Vec<usize> = layouts
            .iter()
            .enumerate()
            .filter(|(_, l)| l.char_count > 0)
            .map(|(i, _)| i)
            .collect();
        assert_eq!(item_idxs.len(), 3);
        let mut state = State::default();
        state.selection = Some(Selection {
            anchor: (item_idxs[0], 2),
            head: (item_idxs[2], 3),
        });
        let text = selected_text(&state, contents, bounds, &config).unwrap();
        assert_eq!(text, "pha\nbeta\ngam");
    }

    #[test]
    fn selected_text_none_when_empty() {
        let config = Config::default();
        let mut state = State::default();
        state.selection = Some(Selection { anchor: (1, 1), head: (1, 1) });
        let result = selected_text(&state, "abc\n", test_bounds(), &config);
        assert!(result.is_none());
    }
}

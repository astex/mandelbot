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
    comments: &'a [Comment],
    config: &'a Config,
}

impl<'a> PlanReviewWidget<'a> {
    pub fn new(
        tab_id: usize,
        contents: &'a str,
        review_pending: bool,
        comments: &'a [Comment],
        config: &'a Config,
    ) -> Self {
        Self { tab_id, contents, review_pending, comments, config }
    }
}

/// A user-authored comment attached to a selection range in the rendered plan.
/// `line_*`/`char_*` are indices over the widget's rendered-line coordinate
/// system (the same one used by [`Selection`]).
#[derive(Debug, Clone)]
pub struct Comment {
    pub line_start: usize,
    pub char_start: usize,
    pub line_end: usize,
    pub char_end: usize,
    /// Body the user typed in the composer.
    pub text: String,
    /// Plain-text excerpt of what was selected when the comment was made.
    pub selected_text: String,
}

/// Widget tree state — tracks the active mouse selection and inline composer.
#[derive(Debug, Default, Clone)]
pub struct State {
    pub selection: Option<Selection>,
    pub dragging: bool,
    /// True while the inline comment composer is accepting keyboard input.
    pub composing: bool,
    /// Text the user is currently typing in the composer.
    pub composer_buffer: String,
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

        // Draw gutter markers for commented line ranges. A small vertical bar
        // sits in the left padding area for any line touched by a saved
        // comment, plus a translucent underline tint on the affected text.
        if !self.comments.is_empty() {
            let marker_color = theme.yellow;
            let underline = Color { a: 0.18, ..theme.yellow };
            for c in self.comments {
                for (i, ll) in layouts.iter().enumerate() {
                    if i < c.line_start || i > c.line_end {
                        continue;
                    }
                    if ll.y + ll.height > max_y {
                        break;
                    }
                    let bar_x = bounds.x + pad_x / 2.0;
                    let bar_w = (char_w / 3.0).max(2.0);
                    renderer.fill_quad(
                        Quad {
                            bounds: Rectangle::new(
                                Point::new(bar_x, ll.y),
                                Size::new(bar_w, ll.height),
                            ),
                            border: Border::default(),
                            ..Quad::default()
                        },
                        marker_color,
                    );
                    let from = if i == c.line_start { c.char_start } else { 0 };
                    let to = if i == c.line_end { c.char_end } else { ll.char_count };
                    if to > from && ll.char_width > 0.0 {
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
                            underline,
                        );
                    }
                }
            }
        }

        // Draw selection highlight on top of text. A translucent overlay in the
        // theme's foreground color reads clearly against any theme without
        // requiring per-span re-rendering.
        let state = tree.state.downcast_ref::<State>();

        // Footer at the bottom of the bounds explaining the current keybinds.
        let footer_h = base_line + char_w; // a little vertical breathing room
        let footer_y = bounds.y + bounds.height - footer_h;
        let footer_bg = tint(theme.bg, theme.fg, 0.08);
        renderer.fill_quad(
            Quad {
                bounds: Rectangle::new(
                    Point::new(bounds.x, footer_y),
                    Size::new(bounds.width, footer_h),
                ),
                border: Border::default(),
                ..Quad::default()
            },
            footer_bg,
        );
        let footer_text = footer_label(state, self.comments.len(), self.review_pending);
        renderer.fill_text(
            Text {
                content: footer_text,
                bounds: Size::new(bounds.width - pad_x * 2.0, base_line),
                size: base_size.into(),
                line_height: text::LineHeight::Relative(self.config.line_height),
                font: Font::MONOSPACE,
                align_x: iced::alignment::Horizontal::Left.into(),
                align_y: iced::alignment::Vertical::Top.into(),
                shaping: text::Shaping::Advanced,
                wrapping: text::Wrapping::None,
            },
            Point::new(bounds.x + pad_x, footer_y + char_w / 2.0),
            theme.fg,
            bounds,
        );

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

        // Inline composer popup, anchored at the bottom-right of the selection.
        if state.composing
            && let Some(sel) = state.selection
        {
            let (_, (e_line, e_char)) = sel.ordered();
            if let Some(ll) = layouts.get(e_line) {
                let popup_w = (char_w * 40.0).min(bounds.width - pad_x * 2.0);
                let popup_h = base_line + char_w * 2.0;
                let mut px = ll.text_x + e_char as f32 * ll.char_width;
                let mut py = ll.y + ll.height;
                if px + popup_w > bounds.x + bounds.width - pad_x {
                    px = bounds.x + bounds.width - pad_x - popup_w;
                }
                if py + popup_h > footer_y {
                    py = (footer_y - popup_h).max(bounds.y);
                }
                let popup_bg = tint(theme.bg, theme.fg, 0.18);
                renderer.fill_quad(
                    Quad {
                        bounds: Rectangle::new(
                            Point::new(px, py),
                            Size::new(popup_w, popup_h),
                        ),
                        border: Border {
                            color: theme.bright_black,
                            width: 1.0,
                            radius: 2.0.into(),
                        },
                        ..Quad::default()
                    },
                    popup_bg,
                );
                let buf = format!("{}\u{2581}", state.composer_buffer);
                renderer.fill_text(
                    Text {
                        content: buf,
                        bounds: Size::new(popup_w - char_w, base_line),
                        size: base_size.into(),
                        line_height: text::LineHeight::Relative(self.config.line_height),
                        font: Font::MONOSPACE,
                        align_x: iced::alignment::Horizontal::Left.into(),
                        align_y: iced::alignment::Vertical::Top.into(),
                        shaping: text::Shaping::Advanced,
                        wrapping: text::Wrapping::None,
                    },
                    Point::new(px + char_w / 2.0, py + char_w / 2.0),
                    theme.fg,
                    bounds,
                );
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
                        // Cancel any open composer when starting a new drag.
                        if state.composing {
                            state.composing = false;
                            state.composer_buffer.clear();
                        }
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
                    // zero-width artifact in state. A non-empty drag opens
                    // the inline composer for the user to attach a comment.
                    if state.selection.is_some_and(|s| s.is_empty()) {
                        state.selection = None;
                    } else if state.selection.is_some() {
                        state.composing = true;
                        state.composer_buffer.clear();
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
            text,
            ..
        }) = event
        {
            use keyboard::key::Named;

            // While composing a comment, capture text and editing keys before
            // routing through global keybindings so plain ASCII characters
            // don't fire shortcuts. Modifier-bearing keys (e.g. cmd+w) still
            // fall through to keybindings below.
            {
                let state_ref = tree.state.downcast_ref::<State>();
                if state_ref.composing && !modifiers.command() && !modifiers.control() {
                    if matches!(key, keyboard::Key::Named(Named::Enter)) {
                        // Save the comment from current state.
                        let lines = markdown::parse(self.contents);
                        let layouts = compute_layouts(&lines, bounds, self.config);
                        let state = tree.state.downcast_mut::<State>();
                        if let Some(sel) = state.selection {
                            let ((s_line, s_char), (e_line, e_char)) = sel.ordered();
                            let selected = collect_selected_text(&layouts, sel);
                            let body = std::mem::take(&mut state.composer_buffer);
                            state.composing = false;
                            state.selection = None;
                            if !body.is_empty() {
                                shell.publish(Message::PlanReviewAddComment(
                                    self.tab_id,
                                    Comment {
                                        line_start: s_line,
                                        char_start: s_char,
                                        line_end: e_line,
                                        char_end: e_char,
                                        text: body,
                                        selected_text: selected,
                                    },
                                ));
                            }
                        }
                        shell.request_redraw();
                        shell.capture_event();
                        return;
                    }
                    if matches!(key, keyboard::Key::Named(Named::Escape)) {
                        let state = tree.state.downcast_mut::<State>();
                        state.composing = false;
                        state.composer_buffer.clear();
                        state.selection = None;
                        shell.request_redraw();
                        shell.capture_event();
                        return;
                    }
                    if matches!(key, keyboard::Key::Named(Named::Backspace)) {
                        let state = tree.state.downcast_mut::<State>();
                        state.composer_buffer.pop();
                        shell.request_redraw();
                        shell.capture_event();
                        return;
                    }
                    if let Some(t) = text {
                        let state = tree.state.downcast_mut::<State>();
                        for ch in t.chars() {
                            if !ch.is_control() {
                                state.composer_buffer.push(ch);
                            }
                        }
                        shell.request_redraw();
                        shell.capture_event();
                        return;
                    }
                }
            }

            let ctx = super::keybindings::KeyContext { active_tab_id: self.tab_id };
            if let Some(msg) = super::keybindings::keybinding_message(self.config, key, *modifiers, &ctx) {
                shell.publish(msg);
                shell.capture_event();
                return;
            }

            // With saved comments, Enter/Esc become send-feedback / cancel
            // (clear comments and return to plain accept/reject mode).
            if !self.comments.is_empty() {
                if matches!(key, keyboard::Key::Named(Named::Enter)) {
                    shell.publish(Message::PlanReviewFeedback(self.tab_id));
                    shell.capture_event();
                    return;
                }
                if matches!(key, keyboard::Key::Named(Named::Escape)) {
                    let state = tree.state.downcast_mut::<State>();
                    state.selection = None;
                    shell.publish(Message::PlanReviewClearComments(self.tab_id));
                    shell.request_redraw();
                    shell.capture_event();
                    return;
                }
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

/// Footer text shown at the bottom of the overlay. The exact label depends on
/// whether the composer is active, whether the user has any saved comments,
/// and whether the agent is awaiting a review decision.
fn footer_label(state: &State, comment_count: usize, review_pending: bool) -> String {
    if state.composing {
        "[Enter] Save comment · [Esc] Cancel".to_string()
    } else if comment_count > 0 {
        format!("[Enter] Send feedback ({}) · [Esc] Cancel", comment_count)
    } else if review_pending {
        "[Enter] Accept · [Esc] Reject".to_string()
    } else {
        "[Esc] Close".to_string()
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

/// Joins the text covered by `sel` across already-computed line layouts.
/// Used by the comment composer when saving a comment so the message
/// payload contains the literal selected excerpt.
fn collect_selected_text(layouts: &[LineLayout], sel: Selection) -> String {
    let ((s_line, s_char), (e_line, e_char)) = sel.ordered();
    let mut parts: Vec<String> = Vec::new();
    for (i, ll) in layouts.iter().enumerate() {
        if i < s_line || i > e_line {
            continue;
        }
        let from = if i == s_line { s_char } else { 0 };
        let to = if i == e_line { e_char } else { ll.char_count };
        let segment: String = ll.text.chars().skip(from).take(to.saturating_sub(from)).collect();
        if !segment.is_empty() {
            parts.push(segment);
        }
    }
    parts.join("\n")
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
    fn footer_label_reflects_state() {
        let empty = State::default();
        assert_eq!(footer_label(&empty, 0, true), "[Enter] Accept · [Esc] Reject");
        assert_eq!(footer_label(&empty, 0, false), "[Esc] Close");
        assert_eq!(
            footer_label(&empty, 3, true),
            "[Enter] Send feedback (3) · [Esc] Cancel"
        );
        let mut composing = State::default();
        composing.composing = true;
        assert_eq!(
            footer_label(&composing, 0, true),
            "[Enter] Save comment · [Esc] Cancel"
        );
    }

    #[test]
    fn collect_selected_text_joins_lines() {
        let config = Config::default();
        let contents = "- alpha\n- beta\n- gamma\n";
        let lines = markdown::parse(contents);
        let layouts = compute_layouts(&lines, test_bounds(), &config);
        let item_idxs: Vec<usize> = layouts
            .iter()
            .enumerate()
            .filter(|(_, l)| l.char_count > 0)
            .map(|(i, _)| i)
            .collect();
        let sel = Selection {
            anchor: (item_idxs[0], 2),
            head: (item_idxs[2], 3),
        };
        assert_eq!(collect_selected_text(&layouts, sel), "pha\nbeta\ngam");
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

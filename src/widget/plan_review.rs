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

impl<'a> Widget<Message, iced::Theme, iced::Renderer> for PlanReviewWidget<'a> {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
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
        _tree: &widget::Tree,
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
        let pad_y = base_line / 2.0;
        let indent_step = char_w * 2.0;

        let lines = markdown::parse(self.contents);

        let mut y = bounds.y + pad_y;
        let max_y = bounds.y + bounds.height;
        for line in &lines {
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
                    y += metrics.line_height;
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

            y += metrics.line_height;
        }
    }

    fn update(
        &mut self,
        _tree: &mut Tree,
        event: &Event,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
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

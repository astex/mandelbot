use iced::advanced::layout;
use iced::advanced::renderer::{self, Quad};
use iced::advanced::text::{self, Renderer as _};
use iced::advanced::widget::{self, Tree};
use iced::advanced::{Clipboard, Layout, Renderer as _, Shell, Text, Widget};
use iced::keyboard;
use iced::mouse;
use iced::{Border, Color, Element, Event, Font, Length, Point, Rectangle, Size};

use crate::config::Config;
use crate::ui::Message;

const SCROLLBAR_WIDTH: f32 = 8.0;
const SCROLLBAR_MIN_THUMB: f32 = 12.0;
const FOOTER_LINES: f32 = 1.5;

/// Minimal plain-text plan renderer with scrolling and a footer affordance.
///
/// PR-7 scope: keyboard / scroll handling in `update` and scrollbar layout.
/// PR-4 owns rich markdown rendering in `draw` and `src/markdown.rs`; the
/// plain-text rendering here is the temporary placeholder shipped in PR-2.
pub struct PlanReviewWidget<'a> {
    tab_id: usize,
    contents: &'a str,
    review_pending: bool,
    config: &'a Config,
}

#[derive(Default)]
struct PlanReviewState {
    /// First visible line index.
    scroll_offset: usize,
    scrollbar_hovered: bool,
    drag: Option<Drag>,
}

struct Drag {
    drag_start_y: f32,
    drag_start_offset: usize,
}

struct TrackGeometry {
    thumb_height: f32,
    usable: f32,
    max_offset: usize,
}

impl TrackGeometry {
    fn new(track_height: f32, visible_lines: usize, total_lines: usize) -> Option<Self> {
        if total_lines <= visible_lines || track_height <= 0.0 {
            return None;
        }
        let max_offset = total_lines - visible_lines;
        let thumb_ratio = visible_lines as f32 / total_lines as f32;
        let thumb_height = (thumb_ratio * track_height).max(SCROLLBAR_MIN_THUMB);
        let usable = (track_height - thumb_height).max(0.0);
        Some(Self { thumb_height, usable, max_offset })
    }

    fn thumb_y(&self, bounds_y: f32, offset: usize) -> f32 {
        let frac = if self.max_offset == 0 {
            0.0
        } else {
            offset as f32 / self.max_offset as f32
        };
        bounds_y + frac * self.usable
    }

    fn offset_from_y(&self, y: f32, bounds_y: f32) -> usize {
        if self.usable <= 0.0 {
            return 0;
        }
        let frac = ((y - bounds_y - self.thumb_height / 2.0) / self.usable).clamp(0.0, 1.0);
        (frac * self.max_offset as f32).round() as usize
    }

    fn offset_from_drag(&self, drag_start_y: f32, drag_start_offset: usize, current_y: f32) -> usize {
        if self.usable <= 0.0 {
            return 0;
        }
        let dy = current_y - drag_start_y;
        let delta = (dy / self.usable * self.max_offset as f32).round() as i32;
        (drag_start_offset as i32 + delta)
            .max(0)
            .min(self.max_offset as i32) as usize
    }
}

impl<'a> PlanReviewWidget<'a> {
    pub fn new(tab_id: usize, contents: &'a str, review_pending: bool, config: &'a Config) -> Self {
        Self { tab_id, contents, review_pending, config }
    }

    fn line_height(&self) -> f32 {
        self.config.char_height()
    }

    fn footer_height(&self) -> f32 {
        self.line_height() * FOOTER_LINES
    }

    fn content_area(&self, bounds: Rectangle) -> Rectangle {
        let footer = self.footer_height();
        Rectangle {
            x: bounds.x,
            y: bounds.y,
            width: bounds.width,
            height: (bounds.height - footer).max(0.0),
        }
    }

    fn visible_lines(&self, bounds: Rectangle) -> usize {
        let h = self.content_area(bounds).height - self.line_height();
        (h / self.line_height()).max(0.0) as usize
    }

    fn total_lines(&self) -> usize {
        self.contents.lines().count().max(1)
    }

    fn max_offset(&self, bounds: Rectangle) -> usize {
        self.total_lines().saturating_sub(self.visible_lines(bounds))
    }

    fn scrollbar_rect(&self, bounds: Rectangle) -> Rectangle {
        let content = self.content_area(bounds);
        Rectangle::new(
            Point::new(content.x + content.width - SCROLLBAR_WIDTH, content.y),
            Size::new(SCROLLBAR_WIDTH, content.height),
        )
    }

    fn track_geometry(&self, bounds: Rectangle) -> Option<TrackGeometry> {
        TrackGeometry::new(self.content_area(bounds).height, self.visible_lines(bounds), self.total_lines())
    }

    fn thumb_rect(&self, bounds: Rectangle, offset: usize) -> Option<Rectangle> {
        let track = self.track_geometry(bounds)?;
        let content = self.content_area(bounds);
        let y = track.thumb_y(content.y, offset);
        Some(Rectangle::new(
            Point::new(content.x + content.width - SCROLLBAR_WIDTH, y),
            Size::new(SCROLLBAR_WIDTH, track.thumb_height),
        ))
    }
}

impl<'a> Widget<Message, iced::Theme, iced::Renderer> for PlanReviewWidget<'a> {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<PlanReviewState>()
    }

    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(PlanReviewState::default())
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
        let state = tree.state.downcast_ref::<PlanReviewState>();
        let content = self.content_area(bounds);
        let line_height = self.line_height();
        let pad_x = self.config.char_width();
        let pad_y = line_height / 2.0;
        let theme = self.config.terminal_theme();
        let fg = theme.fg;

        let max_off = self.max_offset(bounds);
        let offset = state.scroll_offset.min(max_off);

        for (i, line) in self.contents.lines().skip(offset).enumerate() {
            let y = content.y + pad_y + i as f32 * line_height;
            if y + line_height > content.y + content.height {
                break;
            }
            renderer.fill_text(
                Text {
                    content: line.to_string(),
                    bounds: Size::new(content.width - pad_x * 2.0 - SCROLLBAR_WIDTH, line_height),
                    size: self.config.font_size.into(),
                    line_height: text::LineHeight::Relative(self.config.line_height),
                    font: Font::MONOSPACE,
                    align_x: iced::alignment::Horizontal::Left.into(),
                    align_y: iced::alignment::Vertical::Top.into(),
                    shaping: text::Shaping::Advanced,
                    wrapping: text::Wrapping::None,
                },
                Point::new(content.x + pad_x, y),
                fg,
                content,
            );
        }

        // Scrollbar.
        if let Some(thumb) = self.thumb_rect(bounds, offset) {
            let active = state.drag.is_some();
            let alpha = if active || state.scrollbar_hovered { 0.7 } else { 0.4 };
            let track_color = Color { a: alpha * 0.3, ..fg };
            renderer.fill_quad(
                Quad {
                    bounds: self.scrollbar_rect(bounds),
                    border: Border {
                        radius: (SCROLLBAR_WIDTH / 2.0).into(),
                        ..Border::default()
                    },
                    ..Quad::default()
                },
                track_color,
            );
            let thumb_color = Color { a: alpha, ..fg };
            renderer.fill_quad(
                Quad {
                    bounds: thumb,
                    border: Border {
                        radius: (SCROLLBAR_WIDTH / 2.0).into(),
                        ..Border::default()
                    },
                    ..Quad::default()
                },
                thumb_color,
            );
        }

        // Footer affordance.
        let footer_label = if self.review_pending {
            "[Enter] Accept  ·  [Esc] Reject"
        } else {
            "[Esc] Close"
        };
        let footer_y = bounds.y + bounds.height - self.footer_height() + line_height * 0.25;
        renderer.fill_text(
            Text {
                content: footer_label.to_string(),
                bounds: Size::new(bounds.width - pad_x * 2.0, line_height),
                size: self.config.font_size.into(),
                line_height: text::LineHeight::Relative(self.config.line_height),
                font: Font::MONOSPACE,
                align_x: iced::alignment::Horizontal::Left.into(),
                align_y: iced::alignment::Vertical::Top.into(),
                shaping: text::Shaping::Advanced,
                wrapping: text::Wrapping::None,
            },
            Point::new(bounds.x + pad_x, footer_y),
            Color { a: 0.7, ..fg },
            bounds,
        );

        // Focus ring around the content area.
        renderer.fill_quad(
            Quad {
                bounds: Rectangle {
                    x: bounds.x + 0.5,
                    y: bounds.y + 0.5,
                    width: bounds.width - 1.0,
                    height: bounds.height - 1.0,
                },
                border: Border {
                    color: Color { a: 0.25, ..fg },
                    width: 1.0,
                    radius: 2.0.into(),
                },
                ..Quad::default()
            },
            Color::TRANSPARENT,
        );
    }

    fn mouse_interaction(
        &self,
        _tree: &widget::Tree,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        mouse::Interaction::default()
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
        let state = tree.state.downcast_mut::<PlanReviewState>();
        let max_off = self.max_offset(bounds);
        if state.scroll_offset > max_off {
            state.scroll_offset = max_off;
        }
        let visible = self.visible_lines(bounds).max(1);

        let cursor_pos = match cursor {
            mouse::Cursor::Available(pos) | mouse::Cursor::Levitating(pos) => Some(pos),
            mouse::Cursor::Unavailable => None,
        };

        match event {
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                if let Some(pos) = cursor_pos {
                    if !bounds.contains(pos) {
                        return;
                    }
                }
                let lines = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y * 3.0,
                    mouse::ScrollDelta::Pixels { y, .. } => y / self.line_height(),
                };
                let new_offset = (state.scroll_offset as i32 - lines.round() as i32)
                    .max(0)
                    .min(max_off as i32) as usize;
                if new_offset != state.scroll_offset {
                    state.scroll_offset = new_offset;
                    shell.request_redraw();
                }
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                let was = state.scrollbar_hovered;
                state.scrollbar_hovered = max_off > 0 && self.scrollbar_rect(bounds).contains(*position);
                if state.scrollbar_hovered != was {
                    shell.request_redraw();
                }
                if let Some(drag) = &state.drag {
                    if let Some(track) = self.track_geometry(bounds) {
                        let new_off = track.offset_from_drag(drag.drag_start_y, drag.drag_start_offset, position.y);
                        if new_off != state.scroll_offset {
                            state.scroll_offset = new_off;
                            shell.request_redraw();
                        }
                    }
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(pos) = cursor_pos {
                    let scrollbar = self.scrollbar_rect(bounds);
                    if scrollbar.contains(pos) && max_off > 0 {
                        let mut drag_start_offset = state.scroll_offset;
                        if let (Some(thumb), Some(track)) = (self.thumb_rect(bounds, state.scroll_offset), self.track_geometry(bounds)) {
                            if !thumb.contains(pos) {
                                drag_start_offset = track.offset_from_y(pos.y, scrollbar.y);
                                state.scroll_offset = drag_start_offset;
                            }
                        }
                        state.drag = Some(Drag {
                            drag_start_y: pos.y,
                            drag_start_offset,
                        });
                        shell.capture_event();
                        shell.request_redraw();
                    }
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if state.drag.take().is_some() {
                    shell.capture_event();
                }
            }
            Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, text, .. }) => {
                use keyboard::key::Named;

                let ctx = super::keybindings::KeyContext { active_tab_id: self.tab_id };
                if let Some(msg) = super::keybindings::keybinding_message(self.config, key, *modifiers, &ctx) {
                    shell.publish(msg);
                    shell.capture_event();
                    return;
                }

                let mut new_offset = state.scroll_offset as i32;
                let mut handled = false;

                match key {
                    keyboard::Key::Named(Named::ArrowDown) => {
                        new_offset += 1;
                        handled = true;
                    }
                    keyboard::Key::Named(Named::ArrowUp) => {
                        new_offset -= 1;
                        handled = true;
                    }
                    keyboard::Key::Named(Named::PageDown) | keyboard::Key::Named(Named::Space) => {
                        new_offset += visible as i32;
                        handled = true;
                    }
                    keyboard::Key::Named(Named::PageUp) => {
                        new_offset -= visible as i32;
                        handled = true;
                    }
                    keyboard::Key::Named(Named::Home) => {
                        new_offset = 0;
                        handled = true;
                    }
                    keyboard::Key::Named(Named::End) => {
                        new_offset = max_off as i32;
                        handled = true;
                    }
                    _ => {}
                }

                if !handled {
                    if let Some(t) = text.as_ref().map(|s| s.as_str()) {
                        match t {
                            "j" => { new_offset += 1; handled = true; }
                            "k" => { new_offset -= 1; handled = true; }
                            "g" => { new_offset = 0; handled = true; }
                            "G" => { new_offset = max_off as i32; handled = true; }
                            _ => {}
                        }
                    }
                }

                if handled {
                    let clamped = new_offset.max(0).min(max_off as i32) as usize;
                    if clamped != state.scroll_offset {
                        state.scroll_offset = clamped;
                        shell.request_redraw();
                    }
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
            _ => {}
        }
    }
}

impl<'a> From<PlanReviewWidget<'a>> for Element<'a, Message, iced::Theme, iced::Renderer> {
    fn from(widget: PlanReviewWidget<'a>) -> Self {
        Self::new(widget)
    }
}

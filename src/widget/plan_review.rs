use iced::advanced::layout;
use iced::advanced::renderer;
use iced::advanced::text::{self, Renderer as _};
use iced::advanced::widget::{self, Tree};
use iced::advanced::{Clipboard, Layout, Shell, Text, Widget};
use iced::keyboard;
use iced::mouse;
use iced::{Element, Event, Font, Length, Point, Rectangle, Size};

use crate::config::Config;
use crate::ui::Message;

/// Minimal plain-text plan renderer. Reads cached plan contents off the tab
/// and draws them line-by-line in the monospace metrics used by the terminal.
pub struct PlanReviewWidget<'a> {
    tab_id: usize,
    contents: &'a str,
    config: &'a Config,
}

impl<'a> PlanReviewWidget<'a> {
    pub fn new(tab_id: usize, contents: &'a str, config: &'a Config) -> Self {
        Self { tab_id, contents, config }
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
        let line_height = self.config.char_height();
        let pad_x = self.config.char_width();
        let pad_y = line_height / 2.0;
        let fg = self.config.terminal_theme().fg;

        for (i, line) in self.contents.lines().enumerate() {
            let y = bounds.y + pad_y + i as f32 * line_height;
            if y + line_height > bounds.y + bounds.height {
                break;
            }
            renderer.fill_text(
                Text {
                    content: line.to_string(),
                    bounds: Size::new(bounds.width - pad_x * 2.0, line_height),
                    size: self.config.font_size.into(),
                    line_height: text::LineHeight::Relative(self.config.line_height),
                    font: Font::MONOSPACE,
                    align_x: iced::alignment::Horizontal::Left.into(),
                    align_y: iced::alignment::Vertical::Top.into(),
                    shaping: text::Shaping::Advanced,
                    wrapping: text::Wrapping::None,
                },
                Point::new(bounds.x + pad_x, y),
                fg,
                bounds,
            );
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

            if matches!(key, keyboard::Key::Named(Named::Escape)) {
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

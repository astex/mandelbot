use iced::advanced::layout;
use iced::advanced::renderer;
use iced::advanced::text::{self, Renderer as _};
use iced::advanced::widget::{self, Tree};
use iced::advanced::{Clipboard, Layout, Shell, Text, Widget};
use iced::keyboard;
use iced::mouse;
use iced::{Color, Element, Event, Font, Length, Point, Rectangle, Size};

use crate::config::Config;
use crate::ui::Message;

pub struct FoldPlaceholderWidget<'a> {
    fold_parent_id: usize,
    fold_count: usize,
    config: &'a Config,
}

impl<'a> FoldPlaceholderWidget<'a> {
    pub fn new(fold_parent_id: usize, fold_count: usize, config: &'a Config) -> Self {
        Self { fold_parent_id, fold_count, config }
    }
}

impl<'a> Widget<Message, iced::Theme, iced::Renderer> for FoldPlaceholderWidget<'a> {
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
        let count = self.fold_count;
        let label = format!("press any key to open {} tabs", count);

        let text_width = label.len() as f32 * self.config.char_width();
        let text_x = bounds.x + (bounds.width - text_width) / 2.0;
        let text_y = bounds.y + (bounds.height - self.config.char_height()) / 2.0;
        renderer.fill_text(
            Text {
                content: label,
                bounds: Size::new(text_width, self.config.char_height()),
                size: self.config.font_size.into(),
                line_height: text::LineHeight::Relative(self.config.line_height),
                font: Font::MONOSPACE,
                align_x: iced::alignment::Horizontal::Left.into(),
                align_y: iced::alignment::Vertical::Top.into(),
                shaping: text::Shaping::Advanced,
                wrapping: text::Wrapping::None,
            },
            Point::new(text_x, text_y),
            Color { a: 0.5, ..self.config.terminal_theme().fg },
            bounds,
        );
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

            let ctx = super::keybindings::KeyContext { active_tab_id: self.fold_parent_id };
            if let Some(msg) = super::keybindings::keybinding_message(self.config, key, *modifiers, &ctx) {
                shell.publish(msg);
                shell.capture_event();
                return;
            }

            // Ignore bare modifier key presses.
            if matches!(key, keyboard::Key::Named(
                Named::Shift | Named::Control | Named::Alt | Named::Super
            )) {
                return;
            }

            // Any other key: unfold.
            shell.publish(Message::UnfoldTab(self.fold_parent_id));
            shell.capture_event();
        }
    }
}

impl<'a> From<FoldPlaceholderWidget<'a>> for Element<'a, Message, iced::Theme, iced::Renderer> {
    fn from(widget: FoldPlaceholderWidget<'a>) -> Self {
        Self::new(widget)
    }
}

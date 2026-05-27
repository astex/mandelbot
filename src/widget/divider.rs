//! Vertical drag handle between the tab bar and the terminal pane.
//!
//! Only handles press-detection — the actual drag (cursor motion +
//! release) is driven by a global `iced::event::listen_with`
//! subscription gated on `App::tab_bar_dragging`.  See `App::subscription`.

use iced::widget::{container, mouse_area, Space};
use iced::{Border, Color, Element, Fill, Theme};

use crate::theme::TerminalTheme;
use crate::ui::{Message, DIVIDER_WIDTH};

/// Build the draggable vertical divider element.  Renders as a thin
/// muted strip; cursor changes to a horizontal-resize glyph on hover.
pub fn view<'a>(terminal_theme: &TerminalTheme) -> Element<'a, Message> {
    let muted = Color { a: 0.25, ..terminal_theme.fg };
    let strip = container(Space::new().width(DIVIDER_WIDTH).height(Fill))
        .width(DIVIDER_WIDTH)
        .height(Fill)
        .style(move |_theme: &Theme| container::Style {
            background: Some(muted.into()),
            border: Border::default(),
            ..Default::default()
        });

    mouse_area(strip)
        .on_press(Message::TabBarDragStart)
        .interaction(iced::mouse::Interaction::ResizingHorizontally)
        .into()
}

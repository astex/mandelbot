use alacritty_terminal::term::cell::Flags;

use iced::advanced::text::{self, Renderer as _};
use iced::advanced::Text;
use iced::{Color, Font, Point, Rectangle, Size};

use super::box_char;

/// Draw a single terminal cell's character, trying geometric box/block
/// drawing first and falling back to text rendering.
pub fn draw(
    renderer: &mut iced::Renderer,
    c: char,
    flags: Flags,
    fg: Color,
    cell_bounds: Rectangle,
    base_font: Font,
    font_size: f32,
    line_height: f32,
) {
    if box_char::draw(renderer, c, fg, cell_bounds) {
        return;
    }

    let font = if flags.contains(Flags::BOLD) {
        Font {
            weight: iced::font::Weight::Bold,
            ..base_font
        }
    } else {
        base_font
    };

    let cell_width = cell_bounds.width;
    let row_height = cell_bounds.height;

    // Widen clip bounds for non-ASCII so emoji-style fallback
    // glyphs aren't clipped to half a cell.
    let text_clip = if !c.is_ascii() && !c.is_whitespace() {
        Rectangle::new(
            Point::new(cell_bounds.x, cell_bounds.y),
            Size::new(cell_width * 2.0, row_height),
        )
    } else {
        cell_bounds
    };

    renderer.fill_text(
        Text {
            content: c.to_string(),
            bounds: Size::new(cell_width * 2.0, row_height),
            size: font_size.into(),
            line_height: text::LineHeight::Relative(line_height),
            font,
            align_x: iced::alignment::Horizontal::Left.into(),
            align_y: iced::alignment::Vertical::Top.into(),
            shaping: text::Shaping::Advanced,
            wrapping: text::Wrapping::None,
        },
        Point::new(cell_bounds.x, cell_bounds.y),
        fg,
        text_clip,
    );
}

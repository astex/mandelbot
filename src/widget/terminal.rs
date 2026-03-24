use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::TermMode;
use alacritty_terminal::vte::ansi::{Color as AnsiColor, NamedColor};

use iced::advanced::layout;
use iced::advanced::renderer::{self, Quad};
use iced::advanced::text::{self, Renderer as _};
use iced::advanced::widget::{self, Tree};
use iced::advanced::{Clipboard, Layout, Renderer as _, Shell, Text, Widget};
use iced::keyboard;
use iced::mouse;
use iced::{Border, Color, Element, Event, Font, Length, Point, Rectangle, Size};

use crate::config::Config;
use crate::keys;
use crate::terminal::TerminalTab;
use crate::theme::TerminalTheme;
use crate::ui::Message;

pub struct TerminalWidget<'a> {
    tab: &'a TerminalTab,
    config: &'a Config,
    theme: TerminalTheme,
}

impl<'a> TerminalWidget<'a> {
    pub fn new(
        tab: &'a TerminalTab,
        config: &'a Config,
    ) -> Self {
        Self {
            tab,
            config,
            theme: config.terminal_theme(),
        }
    }

    fn char_width(&self) -> f32 {
        self.config.char_width()
    }

    fn char_height(&self) -> f32 {
        self.config.char_height()
    }
}

impl<'a> Widget<Message, iced::Theme, iced::Renderer> for TerminalWidget<'a> {
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
        let grid = self.tab.grid();
        let display_offset = grid.display_offset();
        let cursor_point = grid.cursor.point;
        let show_cursor = self.tab.mode().contains(TermMode::SHOW_CURSOR);

        for row in 0..grid.screen_lines() {
            let row_idx = alacritty_terminal::index::Line(row as i32) - display_offset;
            let y = bounds.y + row as f32 * self.char_height();
            let next_y = bounds.y + (row + 1) as f32 * self.char_height();
            let row_height = next_y - y;

            let mut col = 0;
            while col < grid.columns() {
                let cell = &grid[row_idx][alacritty_terminal::index::Column(col)];
                let x = bounds.x + col as f32 * self.char_width();

                let is_wide = cell.flags.contains(Flags::WIDE_CHAR);
                let cell_cols = if is_wide { 2 } else { 1 };
                let next_x = bounds.x + (col + cell_cols) as f32 * self.char_width();
                let cell_width = next_x - x;

                let is_cursor = show_cursor
                    && display_offset == 0
                    && cursor_point.line == row_idx
                    && cursor_point.column.0 == col;

                let mut fg = ansi_to_color(cell.fg, &self.theme);
                let mut bg = ansi_to_color_bg(cell.bg, &self.theme);

                if is_cursor || cell.flags.contains(Flags::INVERSE) {
                    std::mem::swap(&mut fg, &mut bg);
                }

                let cell_bounds = Rectangle::new(
                    Point::new(x, y),
                    Size::new(cell_width, row_height),
                );

                if bg != self.theme.bg || is_cursor {
                    renderer.fill_quad(
                        Quad {
                            bounds: cell_bounds,
                            border: Border::default(),
                            ..Quad::default()
                        },
                        bg,
                    );
                }

                if cell.c != ' ' && cell.c != '\0' {
                    let font = if cell.flags.contains(Flags::BOLD) {
                        Font {
                            weight: iced::font::Weight::Bold,
                            ..Font::MONOSPACE
                        }
                    } else {
                        Font::MONOSPACE
                    };

                    renderer.fill_text(
                        Text {
                            content: cell.c.to_string(),
                            bounds: Size::new(cell_width, row_height),
                            size: self.config.font_size.into(),
                            line_height: text::LineHeight::Absolute(row_height.into()),
                            font,
                            align_x: iced::alignment::Horizontal::Left.into(),
                            align_y: iced::alignment::Vertical::Top.into(),
                            shaping: text::Shaping::Advanced,
                            wrapping: text::Wrapping::None,
                        },
                        Point::new(x, y),
                        fg,
                        cell_bounds,
                    );
                }

                col += cell_cols;
            }
        }
    }

    fn update(
        &mut self,
        _tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        match event {
            Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                text,
                modifiers,
                ..
            }) => {
                let key = key.clone();
                let text = text.clone();

                if let Some(bytes) = key_to_bytes(&key, text.as_deref(), *modifiers) {
                    shell.publish(Message::PtyInput(bytes));
                    shell.capture_event();
                } else if let Some(scroll) = key_to_scroll(&key, *modifiers, self.tab.rows()) {
                    shell.publish(Message::Scroll(scroll));
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                if cursor.is_over(layout.bounds()) {
                    let lines = match delta {
                        mouse::ScrollDelta::Lines { y, .. } => *y as i32,
                        mouse::ScrollDelta::Pixels { y, .. } => {
                            (*y / self.char_height()) as i32
                        }
                    };
                    if lines != 0 {
                        shell.publish(Message::Scroll(lines));
                        shell.capture_event();
                    }
                }
            }
            _ => {}
        }
    }
}

fn key_to_bytes(
    key: &keyboard::Key,
    text: Option<&str>,
    modifiers: keyboard::Modifiers,
) -> Option<Vec<u8>> {
    use keyboard::key::Named;
    use keyboard::Key;

    match (key, text) {
        (Key::Named(Named::Enter), _) if modifiers.shift() => Some(vec![b'\n']),
        (Key::Named(Named::Enter), _) => Some(vec![b'\r']),
        (Key::Named(Named::Backspace), _) => Some(vec![keys::DEL]),
        (Key::Named(Named::Space), _) => Some(vec![keys::SPACE]),
        (Key::Named(Named::Tab), _) => Some(vec![keys::TAB]),
        (Key::Named(Named::Escape), _) => Some(vec![keys::ESCAPE]),
        (Key::Named(Named::ArrowUp), _) => Some(keys::ARROW_UP.to_vec()),
        (Key::Named(Named::ArrowDown), _) => Some(keys::ARROW_DOWN.to_vec()),
        (Key::Named(Named::ArrowRight), _) => Some(keys::ARROW_RIGHT.to_vec()),
        (Key::Named(Named::ArrowLeft), _) => Some(keys::ARROW_LEFT.to_vec()),
        (Key::Character(c), _) if modifiers.control() && c.as_ref() == "c" => {
            Some(vec![keys::CTRL_C])
        }
        (Key::Named(_), _) => None,
        (_, Some(chars)) if !chars.is_empty() => Some(chars.as_bytes().to_vec()),
        _ => None,
    }
}

fn key_to_scroll(
    key: &keyboard::Key,
    modifiers: keyboard::Modifiers,
    rows: usize,
) -> Option<i32> {
    use keyboard::key::Named;
    use keyboard::Key;

    match key {
        Key::Named(Named::PageUp) if modifiers.shift() => Some(-(rows as i32)),
        Key::Named(Named::PageDown) if modifiers.shift() => Some(rows as i32),
        _ => None,
    }
}

fn ansi_to_color_bg(color: AnsiColor, theme: &TerminalTheme) -> Color {
    match color {
        AnsiColor::Named(NamedColor::Background) => theme.bg,
        other => ansi_to_color(other, theme),
    }
}

fn ansi_to_color(color: AnsiColor, theme: &TerminalTheme) -> Color {
    match color {
        AnsiColor::Named(named) => match named {
            NamedColor::Black => theme.black,
            NamedColor::Red => theme.red,
            NamedColor::Green => theme.green,
            NamedColor::Yellow => theme.yellow,
            NamedColor::Blue => theme.blue,
            NamedColor::Magenta => theme.magenta,
            NamedColor::Cyan => theme.cyan,
            NamedColor::White => theme.white,
            NamedColor::BrightBlack => theme.bright_black,
            NamedColor::BrightRed => theme.bright_red,
            NamedColor::BrightGreen => theme.bright_green,
            NamedColor::BrightYellow => theme.bright_yellow,
            NamedColor::BrightBlue => theme.bright_blue,
            NamedColor::BrightMagenta => theme.bright_magenta,
            NamedColor::BrightCyan => theme.bright_cyan,
            NamedColor::BrightWhite => theme.bright_white,
            NamedColor::Foreground => theme.fg,
            _ => theme.fg,
        },
        AnsiColor::Spec(rgb) => Color::from_rgb8(rgb.r, rgb.g, rgb.b),
        AnsiColor::Indexed(idx) => ansi_256_to_color(idx, theme),
    }
}

fn ansi_256_to_color(idx: u8, theme: &TerminalTheme) -> Color {
    match idx {
        0..=15 => {
            let named = match idx {
                0 => NamedColor::Black,
                1 => NamedColor::Red,
                2 => NamedColor::Green,
                3 => NamedColor::Yellow,
                4 => NamedColor::Blue,
                5 => NamedColor::Magenta,
                6 => NamedColor::Cyan,
                7 => NamedColor::White,
                8 => NamedColor::BrightBlack,
                9 => NamedColor::BrightRed,
                10 => NamedColor::BrightGreen,
                11 => NamedColor::BrightYellow,
                12 => NamedColor::BrightBlue,
                13 => NamedColor::BrightMagenta,
                14 => NamedColor::BrightCyan,
                15 => NamedColor::BrightWhite,
                _ => unreachable!(),
            };
            ansi_to_color(AnsiColor::Named(named), theme)
        }
        16..=231 => {
            let idx = idx - 16;
            let r = (idx / 36) * 51;
            let g = ((idx / 6) % 6) * 51;
            let b = (idx % 6) * 51;
            Color::from_rgb8(r, g, b)
        }
        232..=255 => {
            let gray = 8 + (idx - 232) * 10;
            Color::from_rgb8(gray, gray, gray)
        }
    }
}

impl<'a> From<TerminalWidget<'a>> for Element<'a, Message, iced::Theme, iced::Renderer> {
    fn from(widget: TerminalWidget<'a>) -> Self {
        Self::new(widget)
    }
}

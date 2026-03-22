use alacritty_terminal::event::VoidListener;
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, TermMode};
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::vte::ansi::{self, Color as AnsiColor, NamedColor};

use iced::widget::text;
use iced::{font, Color, Font};

use alacritty_terminal::Term;

use crate::theme::TerminalTheme;

pub struct TerminalBuffer {
    term: Term<VoidListener>,
    parser: ansi::Processor,
}

impl TerminalBuffer {
    pub fn new(rows: usize, cols: usize) -> Self {
        let size = TermSize::new(cols, rows);
        let term = Term::new(Config::default(), &size, VoidListener);
        Self {
            term,
            parser: ansi::Processor::new(),
        }
    }

    pub fn feed(&mut self, data: &[u8]) {
        self.parser.advance(&mut self.term, data);
    }

    pub fn rows(&self) -> usize {
        self.term.screen_lines()
    }

    pub fn scroll(&mut self, delta: i32) {
        self.term.scroll_display(Scroll::Delta(delta));
    }

    pub fn scroll_to_bottom(&mut self) {
        self.term.scroll_display(Scroll::Bottom);
    }

    pub fn resize(&mut self, rows: usize, cols: usize) {
        let size = TermSize::new(cols, rows);
        self.term.resize(size);
    }

    pub fn screen_spans(&self, theme: &TerminalTheme) -> Vec<text::Span<'static, (), Font>> {
        let grid = self.term.grid();
        let display_offset = grid.display_offset();
        let cursor_point = grid.cursor.point;
        let show_cursor = self.term.mode().contains(TermMode::SHOW_CURSOR);
        let mut spans = Vec::new();

        for row in 0..grid.screen_lines() {
            if row > 0 {
                spans.push(text::Span::new("\n"));
            }

            let row_idx = alacritty_terminal::index::Line(row as i32)
                - display_offset;
            let mut current_text = String::new();
            let mut current_fg = AnsiColor::Named(NamedColor::Foreground);
            let mut current_bg = AnsiColor::Named(NamedColor::Background);
            let mut current_flags = Flags::empty();
            let mut current_is_cursor = false;

            for col in 0..grid.columns() {
                let cell = &grid[row_idx][alacritty_terminal::index::Column(col)];
                let is_cursor = show_cursor
                    && display_offset == 0
                    && cursor_point.line == row_idx
                    && cursor_point.column.0 == col;

                if cell.fg != current_fg
                    || cell.bg != current_bg
                    || cell.flags != current_flags
                    || is_cursor != current_is_cursor
                {
                    if !current_text.is_empty() {
                        spans.push(styled_span(
                            &current_text,
                            current_fg,
                            current_bg,
                            current_flags,
                            current_is_cursor,
                            theme,
                        ));
                        current_text.clear();
                    }
                    current_fg = cell.fg;
                    current_bg = cell.bg;
                    current_flags = cell.flags;
                    current_is_cursor = is_cursor;
                }

                current_text.push(cell.c);
            }

            let trimmed = current_text.trim_end();
            if !trimmed.is_empty() {
                spans.push(styled_span(
                    trimmed,
                    current_fg,
                    current_bg,
                    current_flags,
                    current_is_cursor,
                    theme,
                ));
            }
        }

        spans
    }
}

fn styled_span(
    content: &str,
    fg: AnsiColor,
    bg: AnsiColor,
    flags: Flags,
    is_cursor: bool,
    theme: &TerminalTheme,
) -> text::Span<'static, (), Font> {
    let mut fg_color = ansi_to_iced_color(fg, theme);
    let mut bg_color = ansi_to_iced_color_bg(bg, theme);
    let has_bg = !matches!(bg, AnsiColor::Named(NamedColor::Background));

    if is_cursor || flags.contains(Flags::INVERSE) {
        std::mem::swap(&mut fg_color, &mut bg_color);
    }

    let mut span = text::Span::new(content.to_string())
        .color(fg_color);

    if has_bg || is_cursor || flags.contains(Flags::INVERSE) {
        span = span.background(bg_color);
    }

    if flags.contains(Flags::BOLD) {
        span = span.font(Font {
            weight: font::Weight::Bold,
            ..Font::MONOSPACE
        });
    }

    span
}

fn ansi_to_iced_color_bg(color: AnsiColor, theme: &TerminalTheme) -> Color {
    match color {
        AnsiColor::Named(NamedColor::Background) => theme.bg,
        other => ansi_to_iced_color(other, theme),
    }
}

fn ansi_to_iced_color(color: AnsiColor, theme: &TerminalTheme) -> Color {
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
        AnsiColor::Indexed(idx) => ansi_256_to_iced_color(idx, theme),
    }
}

fn ansi_256_to_iced_color(idx: u8, theme: &TerminalTheme) -> Color {
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
            ansi_to_iced_color(AnsiColor::Named(named), theme)
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

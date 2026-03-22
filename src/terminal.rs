use alacritty_terminal::event::VoidListener;
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, TermMode};
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::vte::ansi::{self, Color as AnsiColor, NamedColor};

use iced::widget::text;
use iced::{font, Color, Font};

use alacritty_terminal::Term;

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

    pub fn screen_spans(&self) -> Vec<text::Span<'static, (), Font>> {
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
                ));
            }
        }

        spans
    }
}

const DEFAULT_FG: Color = Color::from_rgb(0.83, 0.83, 0.83);
const DEFAULT_BG: Color = Color::from_rgb(0.12, 0.12, 0.12);

fn styled_span(
    content: &str,
    fg: AnsiColor,
    bg: AnsiColor,
    flags: Flags,
    is_cursor: bool,
) -> text::Span<'static, (), Font> {
    let mut fg_color = ansi_to_iced_color(fg);
    let mut bg_color = ansi_to_iced_color_bg(bg);
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

fn ansi_to_iced_color_bg(color: AnsiColor) -> Color {
    match color {
        AnsiColor::Named(NamedColor::Background) => DEFAULT_BG,
        other => ansi_to_iced_color(other),
    }
}

fn ansi_to_iced_color(color: AnsiColor) -> Color {
    match color {
        AnsiColor::Named(named) => match named {
            NamedColor::Black => Color::from_rgb(0.0, 0.0, 0.0),
            NamedColor::Red => Color::from_rgb(0.8, 0.0, 0.0),
            NamedColor::Green => Color::from_rgb(0.0, 0.8, 0.0),
            NamedColor::Yellow => Color::from_rgb(0.8, 0.8, 0.0),
            NamedColor::Blue => Color::from_rgb(0.3, 0.3, 1.0),
            NamedColor::Magenta => Color::from_rgb(0.8, 0.0, 0.8),
            NamedColor::Cyan => Color::from_rgb(0.0, 0.8, 0.8),
            NamedColor::White => Color::from_rgb(0.75, 0.75, 0.75),
            NamedColor::BrightBlack => Color::from_rgb(0.5, 0.5, 0.5),
            NamedColor::BrightRed => Color::from_rgb(1.0, 0.33, 0.33),
            NamedColor::BrightGreen => Color::from_rgb(0.33, 1.0, 0.33),
            NamedColor::BrightYellow => Color::from_rgb(1.0, 1.0, 0.33),
            NamedColor::BrightBlue => Color::from_rgb(0.5, 0.5, 1.0),
            NamedColor::BrightMagenta => Color::from_rgb(1.0, 0.33, 1.0),
            NamedColor::BrightCyan => Color::from_rgb(0.33, 1.0, 1.0),
            NamedColor::BrightWhite => Color::from_rgb(1.0, 1.0, 1.0),
            NamedColor::Foreground => DEFAULT_FG,
            _ => DEFAULT_FG,
        },
        AnsiColor::Spec(rgb) => {
            Color::from_rgb8(rgb.r, rgb.g, rgb.b)
        }
        AnsiColor::Indexed(idx) => {
            ansi_256_to_iced_color(idx)
        }
    }
}

fn ansi_256_to_iced_color(idx: u8) -> Color {
    match idx {
        0..=15 => {
            // Standard colors — defer to named
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
            ansi_to_iced_color(AnsiColor::Named(named))
        }
        16..=231 => {
            // 6x6x6 color cube
            let idx = idx - 16;
            let r = (idx / 36) * 51;
            let g = ((idx / 6) % 6) * 51;
            let b = (idx % 6) * 51;
            Color::from_rgb8(r, g, b)
        }
        232..=255 => {
            // Grayscale ramp
            let gray = 8 + (idx - 232) * 10;
            Color::from_rgb8(gray, gray, gray)
        }
    }
}

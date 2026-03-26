use std::path::Path;
use std::time::Instant;

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point as GridPoint, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
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
use iced::window;
use iced::{Border, Color, Element, Event, Font, Length, Point, Rectangle, Size};

use crate::config::Config;
use crate::keys;
use crate::links;
use crate::terminal::TerminalTab;
use crate::theme::TerminalTheme;
use crate::ui::Message;

pub const SCROLLBAR_WIDTH: f32 = 8.0;
const SCROLLBAR_MIN_THUMB: f32 = 12.0;

struct TrackGeometry {
    thumb_height: f32,
    usable: f32,
    history_size: usize,
}

impl TrackGeometry {
    fn new(track_height: f32, screen_lines: usize, history_size: usize) -> Option<Self> {
        if history_size == 0 {
            return None;
        }
        let total_lines = history_size + screen_lines;
        let thumb_ratio = screen_lines as f32 / total_lines as f32;
        let thumb_height = (thumb_ratio * track_height).max(SCROLLBAR_MIN_THUMB);
        let usable = track_height - thumb_height;
        Some(Self { thumb_height, usable, history_size })
    }

    fn thumb_y(&self, bounds_y: f32, display_offset: usize) -> f32 {
        let scroll_fraction = display_offset as f32 / self.history_size as f32;
        bounds_y + (1.0 - scroll_fraction) * self.usable
    }

    fn offset_from_y(&self, y: f32, bounds_y: f32) -> usize {
        if self.usable <= 0.0 {
            return 0;
        }
        let fraction = 1.0 - ((y - bounds_y - self.thumb_height / 2.0) / self.usable).clamp(0.0, 1.0);
        (fraction * self.history_size as f32).round() as usize
    }

    fn offset_from_drag(&self, drag_start_y: f32, drag_start_offset: usize, current_y: f32) -> usize {
        if self.usable <= 0.0 {
            return 0;
        }
        let dy = current_y - drag_start_y;
        let offset_delta = (dy / self.usable * self.history_size as f32).round() as i32;
        (drag_start_offset as i32 - offset_delta)
            .max(0)
            .min(self.history_size as i32) as usize
    }
}

#[derive(Default)]
enum Interaction {
    #[default]
    Idle,
    Scrollbar {
        drag_start_y: f32,
        drag_start_offset: usize,
    },
    Selecting,
    HoveringLink {
        url: String,
        /// Grid line and column range for each row the link spans.
        cells: Vec<(Line, usize, usize)>,
    },
}

#[derive(Default)]
struct TerminalState {
    interaction: Interaction,
    scrollbar_hovered: bool,
    drop_hovering: bool,
    click_count: u8,
    last_click_time: Option<Instant>,
    last_click_point: Option<Point>,
    modifiers: keyboard::Modifiers,
}

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

    fn scrollbar_rect(&self, bounds: &Rectangle) -> Rectangle {
        Rectangle::new(
            Point::new(bounds.x + bounds.width - SCROLLBAR_WIDTH, bounds.y),
            Size::new(SCROLLBAR_WIDTH, bounds.height),
        )
    }

    fn track_geometry(&self, bounds: &Rectangle) -> Option<TrackGeometry> {
        let grid = self.tab.grid();
        TrackGeometry::new(bounds.height, grid.screen_lines(), self.tab.history_size())
    }

    fn pixel_to_grid(&self, bounds: &Rectangle, pos: Point) -> (GridPoint, Side) {
        let grid = self.tab.grid();
        let col_f = (pos.x - bounds.x) / self.char_width();
        let row_f = (pos.y - bounds.y) / self.char_height();

        let col = (col_f as usize).min(grid.columns().saturating_sub(1));
        let row = (row_f as i32).clamp(0, grid.screen_lines() as i32 - 1);

        let side = if col_f.fract() < 0.5 { Side::Left } else { Side::Right };

        let display_offset = grid.display_offset();
        let line = Line(row) - display_offset;

        (GridPoint::new(line, Column(col)), side)
    }

    fn detect_link(&self, bounds: &Rectangle, pos: Point) -> Option<Interaction> {
        let (grid_point, _side) = self.pixel_to_grid(bounds, pos);
        let logical = self.tab.logical_line_at(grid_point.line);
        let char_off = logical.char_offset(grid_point.line, grid_point.column.0);
        let url_match = links::find_url_at(&logical.text, char_off)?;

        let mut cells = Vec::new();
        let (start_line, start_col) = logical.grid_position(url_match.start);
        let (end_line, end_col) = logical.grid_position(url_match.end.saturating_sub(1));

        // Build per-row (line, start_col, end_col_exclusive) spans.
        let mut line = start_line;
        loop {
            let row_start = if line == start_line { start_col } else { 0 };
            let row_end = if line == end_line { end_col + 1 } else { logical.cols };
            cells.push((line, row_start, row_end));
            if line == end_line {
                break;
            }
            line = line - 1; // move down one screen row
        }

        Some(Interaction::HoveringLink { url: url_match.url, cells })
    }

    fn thumb_rect(&self, bounds: &Rectangle) -> Option<Rectangle> {
        let track = self.track_geometry(bounds)?;
        let thumb_y = track.thumb_y(bounds.y, self.tab.grid().display_offset());
        let track_x = bounds.x + bounds.width - SCROLLBAR_WIDTH;

        Some(Rectangle::new(
            Point::new(track_x, thumb_y),
            Size::new(SCROLLBAR_WIDTH, track.thumb_height),
        ))
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

    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<TerminalState>()
    }

    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(TerminalState::default())
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

        // Pending tab: render a simple text prompt at top-left.
        if let Some(input) = &self.tab.pending_input {
            let label = format!("Project directory: {}_", input);

            renderer.fill_text(
                Text {
                    content: label,
                    bounds: Size::new(bounds.width, self.char_height()),
                    size: self.config.font_size.into(),
                    line_height: text::LineHeight::Absolute(self.char_height().into()),
                    font: Font::MONOSPACE,
                    align_x: iced::alignment::Horizontal::Left.into(),
                    align_y: iced::alignment::Vertical::Top.into(),
                    shaping: text::Shaping::Advanced,
                    wrapping: text::Wrapping::None,
                },
                Point::new(bounds.x, bounds.y),
                self.theme.fg,
                bounds,
            );
            return;
        }

        let grid = self.tab.grid();
        let display_offset = grid.display_offset();
        let cursor_point = grid.cursor.point;
        let show_cursor = self.tab.mode().contains(TermMode::SHOW_CURSOR);
        let selection_range = self.tab.selection_range();

        use std::sync::OnceLock;
        static FONT_NAME: OnceLock<String> = OnceLock::new();
        let font_name = FONT_NAME.get_or_init(|| self.config.font.clone());
        let base_font = Font {
            family: iced::font::Family::Name(font_name.as_str()),
            ..Font::MONOSPACE
        };

        for row in 0..grid.screen_lines() {
            let row_idx = Line(row as i32) - display_offset;
            let y = bounds.y + row as f32 * self.char_height();
            let next_y = bounds.y + (row + 1) as f32 * self.char_height();
            let row_height = next_y - y;

            let mut col = 0;
            while col < grid.columns() {
                let cell = &grid[row_idx][Column(col)];
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

                let is_selected = selection_range.as_ref().is_some_and(|range| {
                    range.contains(GridPoint::new(row_idx, Column(col)))
                });

                if is_cursor || cell.flags.contains(Flags::INVERSE) || is_selected {
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

                let font = if cell.flags.contains(Flags::BOLD) {
                    Font {
                        weight: iced::font::Weight::Bold,
                        ..base_font
                    }
                } else {
                    base_font
                };

                // Widen clip bounds for non-ASCII so emoji-style fallback
                // glyphs aren't clipped to half a cell.
                let text_clip = if !cell.c.is_ascii() && !cell.c.is_whitespace() {
                    Rectangle::new(
                        Point::new(x, y),
                        Size::new(cell_width * 2.0, row_height),
                    )
                } else {
                    cell_bounds
                };

                renderer.fill_text(
                    Text {
                        content: cell.c.to_string(),
                        bounds: Size::new(cell_width * 2.0, row_height),
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
                    text_clip,
                );

                col += cell_cols;
            }
        }

        // Draw underline for hovered link.
        let state = tree.state.downcast_ref::<TerminalState>();
        if let Interaction::HoveringLink { cells, .. } = &state.interaction {
            let underline_thickness = (self.config.font_size * 0.07).max(1.0);
            for &(line, start_col, end_col) in cells {
                // Convert grid line to screen row.
                let screen_row = line.0 + display_offset as i32;
                if screen_row < 0 || screen_row >= grid.screen_lines() as i32 {
                    continue;
                }
                let y = bounds.y + screen_row as f32 * self.char_height()
                    + self.char_height() - underline_thickness;
                let x = bounds.x + start_col as f32 * self.char_width();
                let width = (end_col - start_col) as f32 * self.char_width();
                renderer.fill_quad(
                    Quad {
                        bounds: Rectangle::new(
                            Point::new(x, y),
                            Size::new(width, underline_thickness),
                        ),
                        border: Border::default(),
                        ..Quad::default()
                    },
                    self.theme.fg,
                );
            }
        }

        if let Some(thumb) = self.thumb_rect(&bounds) {
            let scrollbar_active = matches!(state.interaction, Interaction::Scrollbar { .. });
            let alpha = if scrollbar_active || state.scrollbar_hovered { 0.7 } else { 0.4 };

            let track_color = Color { a: alpha * 0.3, ..self.theme.fg };
            let scrollbar = self.scrollbar_rect(&bounds);
            renderer.fill_quad(
                Quad {
                    bounds: scrollbar,
                    border: Border {
                        radius: (SCROLLBAR_WIDTH / 2.0).into(),
                        ..Border::default()
                    },
                    ..Quad::default()
                },
                track_color,
            );

            let thumb_color = Color { a: alpha, ..self.theme.fg };
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
    }

    fn mouse_interaction(
        &self,
        tree: &widget::Tree,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<TerminalState>();
        if state.drop_hovering {
            mouse::Interaction::Copy
        } else if matches!(state.interaction, Interaction::HoveringLink { .. }) {
            mouse::Interaction::Pointer
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
        let state = tree.state.downcast_mut::<TerminalState>();
        let bounds = layout.bounds();

        // Extract cursor position from any variant (Available or Levitating).
        let cursor_pos = match cursor {
            mouse::Cursor::Available(pos) | mouse::Cursor::Levitating(pos) => Some(pos),
            mouse::Cursor::Unavailable => None,
        };

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(pos) = cursor_pos {
                    // Open link on control+click.
                    if let Interaction::HoveringLink { url, .. } = &state.interaction {
                        let _ = open::that(url);
                        shell.capture_event();
                        return;
                    }

                    // Scrollbar takes priority.
                    let scrollbar = self.scrollbar_rect(&bounds);
                    if scrollbar.contains(pos) && self.tab.history_size() > 0 {
                        let mut drag_start_offset = self.tab.grid().display_offset();

                        if let (Some(thumb), Some(track)) = (self.thumb_rect(&bounds), self.track_geometry(&bounds)) {
                            if !thumb.contains(pos) {
                                drag_start_offset = track.offset_from_y(pos.y, bounds.y);
                                shell.publish(Message::ScrollTo(drag_start_offset));
                            }
                        }

                        state.interaction = Interaction::Scrollbar {
                            drag_start_y: pos.y,
                            drag_start_offset,
                        };
                        shell.capture_event();
                        return;
                    }

                    // Text selection in terminal content area.
                    if bounds.contains(pos) {
                        let (grid_point, side) = self.pixel_to_grid(&bounds, pos);

                        // Detect multi-click.
                        let now = Instant::now();
                        let is_multi = state.last_click_time
                            .is_some_and(|t| now.duration_since(t).as_millis() < 500)
                            && state.last_click_point
                                .is_some_and(|p| (p.x - pos.x).abs() < 5.0 && (p.y - pos.y).abs() < 5.0);

                        state.click_count = if is_multi { (state.click_count % 3) + 1 } else { 1 };
                        state.last_click_time = Some(now);
                        state.last_click_point = Some(pos);

                        let sel_type = match state.click_count {
                            2 => SelectionType::Semantic,
                            3 => SelectionType::Lines,
                            _ => SelectionType::Simple,
                        };

                        let mut selection = Selection::new(sel_type, grid_point, side);
                        if state.click_count >= 2 {
                            selection.include_all();
                        }

                        shell.publish(Message::SetSelection(Some(selection)));
                        state.interaction = Interaction::Selecting;
                        shell.request_redraw();
                        shell.capture_event();
                    }
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                // Scrollbar hover tracking (orthogonal to interaction state).
                let was_hovered = state.scrollbar_hovered;
                state.scrollbar_hovered = self.tab.history_size() > 0
                    && self.scrollbar_rect(&bounds).contains(*position);
                if state.scrollbar_hovered != was_hovered {
                    shell.request_redraw();
                }

                // Link detection when control modifier is held.
                if matches!(state.interaction, Interaction::Idle | Interaction::HoveringLink { .. })
                    && self.config.matches_control(state.modifiers)
                    && bounds.contains(*position)
                {
                    let had_link = matches!(state.interaction, Interaction::HoveringLink { .. });
                    state.interaction = self.detect_link(&bounds, *position)
                        .unwrap_or(Interaction::Idle);
                    let has_link = matches!(state.interaction, Interaction::HoveringLink { .. });
                    if had_link || has_link {
                        shell.request_redraw();
                    }
                }

                match &state.interaction {
                    Interaction::Scrollbar { drag_start_y, drag_start_offset } => {
                        if let Some(track) = self.track_geometry(&bounds) {
                            let target = track.offset_from_drag(
                                *drag_start_y,
                                *drag_start_offset,
                                position.y,
                            );
                            shell.publish(Message::ScrollTo(target));
                        }
                        shell.capture_event();
                    }
                    Interaction::Selecting => {
                        let (grid_point, side) = self.pixel_to_grid(&bounds, *position);
                        shell.publish(Message::UpdateSelection(grid_point, side));

                        // Auto-scroll when dragging above or below bounds.
                        if position.y < bounds.y {
                            shell.publish(Message::Scroll(1));
                        } else if position.y > bounds.y + bounds.height {
                            shell.publish(Message::Scroll(-1));
                        }

                        shell.request_redraw();
                        shell.capture_event();
                    }
                    Interaction::Idle | Interaction::HoveringLink { .. } => {}
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                match &state.interaction {
                    Interaction::Scrollbar { .. } => {
                        state.interaction = Interaction::Idle;
                        if let Some(pos) = cursor_pos {
                            state.scrollbar_hovered = self.scrollbar_rect(&bounds).contains(pos);
                        } else {
                            state.scrollbar_hovered = false;
                        }
                        shell.request_redraw();
                        shell.capture_event();
                    }
                    Interaction::Selecting => {
                        state.interaction = Interaction::Idle;
                        shell.request_redraw();
                    }
                    Interaction::Idle | Interaction::HoveringLink { .. } => {}
                }
            }
            Event::Mouse(mouse::Event::CursorLeft) => {
                let mut needs_redraw = false;
                if state.scrollbar_hovered {
                    state.scrollbar_hovered = false;
                    needs_redraw = true;
                }
                if matches!(state.interaction, Interaction::HoveringLink { .. }) {
                    state.interaction = Interaction::Idle;
                    needs_redraw = true;
                }
                if needs_redraw {
                    shell.request_redraw();
                }
            }
            Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                state.modifiers = *modifiers;
                if !self.config.matches_control(*modifiers)
                    && matches!(state.interaction, Interaction::HoveringLink { .. })
                {
                    state.interaction = Interaction::Idle;
                    shell.request_redraw();
                }
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                text,
                modifiers,
                ..
            }) => {
                let key = key.clone();
                let text = text.clone();
                use keyboard::key::Named;

                // Global bindings (work for both normal and pending tabs).

                if self.config.matches_control(*modifiers)
                    && key == keyboard::Key::Character("t".into())
                {
                    shell.publish(Message::NewTab);
                    shell.capture_event();
                    return;
                }

                if self.config.matches_control(*modifiers)
                    && key == keyboard::Key::Named(Named::Space)
                {
                    shell.publish(Message::SpawnAgent);
                    shell.capture_event();
                    return;
                }

                if self.config.matches_control(*modifiers)
                    && key == keyboard::Key::Character("w".into())
                {
                    shell.publish(Message::CloseTab(self.tab.id));
                    shell.capture_event();
                    return;
                }

                // Tree navigation.
                if self.config.matches_movement(*modifiers) {
                    match &key {
                        keyboard::Key::Named(Named::ArrowDown) => {
                            shell.publish(Message::NavigateSibling(1));
                            shell.capture_event();
                            return;
                        }
                        keyboard::Key::Named(Named::ArrowUp) => {
                            shell.publish(Message::NavigateSibling(-1));
                            shell.capture_event();
                            return;
                        }
                        keyboard::Key::Named(Named::ArrowRight) => {
                            shell.publish(Message::NavigateRank(1));
                            shell.capture_event();
                            return;
                        }
                        keyboard::Key::Named(Named::ArrowLeft) => {
                            shell.publish(Message::NavigateRank(-1));
                            shell.capture_event();
                            return;
                        }
                        keyboard::Key::Character(c) => {
                            if let Some(digit) = c.as_ref().parse::<usize>().ok().filter(|&d| (0..=9).contains(&d)) {
                                shell.publish(Message::SelectTabByIndex(digit));
                                shell.capture_event();
                                return;
                            }
                        }
                        _ => {}
                    }
                }

                // Pending tab: route input to PendingInput messages.
                if self.tab.is_pending() {
                    use crate::ui::PendingKey;
                    match &key {
                        keyboard::Key::Named(Named::Enter) => {
                            shell.publish(Message::PendingInput(PendingKey::Submit));
                        }
                        keyboard::Key::Named(Named::Escape) => {
                            shell.publish(Message::PendingInput(PendingKey::Cancel));
                        }
                        keyboard::Key::Named(Named::Backspace) => {
                            shell.publish(Message::PendingInput(PendingKey::Backspace));
                        }
                        _ => {
                            if let Some(chars) = &text {
                                for c in chars.chars() {
                                    shell.publish(Message::PendingInput(PendingKey::Char(c)));
                                }
                            }
                        }
                    }
                    shell.capture_event();
                    return;
                }

                // Normal tab bindings below.

                // Copy selection.
                if self.config.matches_control(*modifiers)
                    && key == keyboard::Key::Character("c".into())
                {
                    if let Some(content) = self.tab.selection_to_string() {
                        _clipboard.write(iced::advanced::clipboard::Kind::Standard, content);
                    }
                    shell.capture_event();
                    return;
                }

                if self.config.matches_control(*modifiers)
                    && key == keyboard::Key::Character("v".into())
                {
                    if let Some(content) = _clipboard.read(iced::advanced::clipboard::Kind::Standard) {
                        shell.publish(Message::PtyInput(content.into_bytes()));
                        shell.capture_event();
                        return;
                    }
                }

                if let Some(bytes) = key_to_bytes(&key, text.as_deref(), *modifiers) {
                    shell.publish(Message::SetSelection(None));
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
            Event::Window(window::Event::FileDropped(path)) => {
                let escaped = shell_escape_path(path);
                shell.publish(Message::PtyInput(escaped.into_bytes()));
                state.drop_hovering = false;
                shell.request_redraw();
                shell.capture_event();
            }
            Event::Window(window::Event::FileHovered(_)) => {
                state.drop_hovering = true;
                shell.request_redraw();
            }
            Event::Window(window::Event::FilesHoveredLeft) => {
                state.drop_hovering = false;
                shell.request_redraw();
            }
            _ => {}
        }
    }
}

fn shell_escape_path(path: &Path) -> String {
    let s = path.to_string_lossy();
    if s.chars().any(|c| " \t'\"\\()&;|<>$`!#*?[]{}~".contains(c)) {
        format!("'{}'", s.replace('\'', "'\\''"))
    } else {
        s.into_owned()
    }
}

fn key_to_bytes(
    key: &keyboard::Key,
    text: Option<&str>,
    modifiers: keyboard::Modifiers,
) -> Option<Vec<u8>> {
    use keyboard::key::Named;
    use keyboard::Key;

    // Arrow keys use CSI modifier encoding: \x1b[1;{m}X where
    // m = 1 + (shift?1:0) + (alt?2:0) + (ctrl?4:0).
    if let Key::Named(
        named @ (Named::ArrowUp | Named::ArrowDown | Named::ArrowRight | Named::ArrowLeft),
    ) = key
    {
        let dir = match named {
            Named::ArrowUp => 'A',
            Named::ArrowDown => 'B',
            Named::ArrowRight => 'C',
            Named::ArrowLeft => 'D',
            _ => unreachable!(),
        };
        let m =
            1 + modifiers.shift() as u8 + (modifiers.alt() as u8) * 2 + (modifiers.control() as u8) * 4;
        return if m == 1 {
            Some(format!("\x1b[{dir}").into_bytes())
        } else {
            Some(format!("\x1b[1;{m}{dir}").into_bytes())
        };
    }

    let base = match (key, text) {
        (Key::Named(Named::Enter), _) if modifiers.shift() => Some(keys::SHIFT_ENTER.to_vec()),
        (Key::Named(Named::Enter), _) => Some(keys::ENTER.to_vec()),
        (Key::Named(Named::Backspace), _) => Some(keys::DEL.to_vec()),
        (Key::Named(Named::Space), _) => Some(keys::SPACE.to_vec()),
        (Key::Named(Named::Tab), _) if modifiers.shift() => Some(b"\x1b[Z".to_vec()),
        (Key::Named(Named::Tab), _) => Some(keys::TAB.to_vec()),
        (Key::Named(Named::Escape), _) => Some(keys::ESCAPE.to_vec()),
        (Key::Character(c), _) if modifiers.control() && c.as_ref() == "c" => {
            Some(keys::CTRL_C.to_vec())
        }
        (Key::Named(_), _) => None,
        (_, Some(chars)) if !chars.is_empty() => Some(chars.as_bytes().to_vec()),
        _ => None,
    };

    // "Meta sends escape": prepend ESC when Alt is held.
    match base {
        Some(bytes) if modifiers.alt() => {
            let mut out = Vec::with_capacity(1 + bytes.len());
            out.push(0x1b);
            out.extend(bytes);
            Some(out)
        }
        other => other,
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

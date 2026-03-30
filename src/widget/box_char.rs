// Block elements (U+2580–U+259F) and box-drawing characters (U+2500–U+257F)
// are drawn as geometric quads rather than font glyphs.
//
// Why: font glyphs for these characters don't reliably fill the terminal grid
// cell. For example, in Noto Sans Mono at 12px, the full block character █
// (U+2588) has ink bounds spanning 14.56px vertically, but the cell height at
// line_height=1.3 is 15.6px — leaving a ~1px gap. This happens because the
// glyph's y-extent (ascender 973, descender -240 in font units) doesn't match
// the font's full ascender-descender range (1069 to -293). The gap varies by
// font, font size, and line_height setting, so no single metric formula can
// eliminate it.
//
// Background quads drawn with fill_quad tile perfectly because they're exact
// rectangles at our floating-point coordinates. Font glyphs don't tile because
// their outlines are designed for text readability, not geometric precision.
//
// Every major terminal emulator (Alacritty, Kitty, WezTerm, etc.) draws these
// characters as custom geometry for the same reason.

use iced::advanced::renderer::Quad;
use iced::advanced::Renderer as _;
use iced::widget::canvas::{self, LineCap, Stroke, Style};
use iced::{Border, Color, Point, Rectangle, Size};

/// Draw a block element or box-drawing character as geometric quads.
/// Returns `true` if the character was handled, `false` to fall back to text.
pub fn draw(
    renderer: &mut iced::Renderer,
    c: char,
    fg: Color,
    cell: Rectangle,
) -> bool {
    let x = cell.x;
    let y = cell.y;
    let w = cell.width;
    let h = cell.height;
    let hw = w / 2.0;
    let hh = h / 2.0;

    let quad = |renderer: &mut iced::Renderer, bounds: Rectangle, color: Color| {
        renderer.fill_quad(
            Quad {
                bounds,
                border: Border::default(),
                ..Quad::default()
            },
            color,
        );
    };

    match c {
        // === Block elements (U+2580–U+259F) ===

        // Upper half block
        '▀' => quad(renderer, Rectangle::new(Point::new(x, y), Size::new(w, hh)), fg),
        // Lower one eighth block through lower seven eighths block
        '▁' => quad(renderer, Rectangle::new(Point::new(x, y + h * 7.0 / 8.0), Size::new(w, h / 8.0)), fg),
        '▂' => quad(renderer, Rectangle::new(Point::new(x, y + h * 6.0 / 8.0), Size::new(w, h * 2.0 / 8.0)), fg),
        '▃' => quad(renderer, Rectangle::new(Point::new(x, y + h * 5.0 / 8.0), Size::new(w, h * 3.0 / 8.0)), fg),
        '▄' => quad(renderer, Rectangle::new(Point::new(x, y + hh), Size::new(w, hh)), fg),
        '▅' => quad(renderer, Rectangle::new(Point::new(x, y + h * 3.0 / 8.0), Size::new(w, h * 5.0 / 8.0)), fg),
        '▆' => quad(renderer, Rectangle::new(Point::new(x, y + h * 2.0 / 8.0), Size::new(w, h * 6.0 / 8.0)), fg),
        '▇' => quad(renderer, Rectangle::new(Point::new(x, y + h / 8.0), Size::new(w, h * 7.0 / 8.0)), fg),
        // Full block
        '█' => quad(renderer, cell, fg),
        // Left seven eighths block through left one eighth block
        '▉' => quad(renderer, Rectangle::new(Point::new(x, y), Size::new(w * 7.0 / 8.0, h)), fg),
        '▊' => quad(renderer, Rectangle::new(Point::new(x, y), Size::new(w * 6.0 / 8.0, h)), fg),
        '▋' => quad(renderer, Rectangle::new(Point::new(x, y), Size::new(w * 5.0 / 8.0, h)), fg),
        '▌' => quad(renderer, Rectangle::new(Point::new(x, y), Size::new(hw, h)), fg),
        '▍' => quad(renderer, Rectangle::new(Point::new(x, y), Size::new(w * 3.0 / 8.0, h)), fg),
        '▎' => quad(renderer, Rectangle::new(Point::new(x, y), Size::new(w * 2.0 / 8.0, h)), fg),
        '▏' => quad(renderer, Rectangle::new(Point::new(x, y), Size::new(w / 8.0, h)), fg),
        // Right half block
        '▐' => quad(renderer, Rectangle::new(Point::new(x + hw, y), Size::new(hw, h)), fg),
        // Light/medium/dark shade
        // These are dithering patterns — fall back to font glyphs.
        '░' | '▒' | '▓' => return false,
        // Upper one eighth block
        '▔' => quad(renderer, Rectangle::new(Point::new(x, y), Size::new(w, h / 8.0)), fg),
        // Right one eighth block
        '▕' => quad(renderer, Rectangle::new(Point::new(x + w * 7.0 / 8.0, y), Size::new(w / 8.0, h)), fg),

        // Quadrant characters: composed of up to 4 quarter-cell quads.
        '▖' => quad(renderer, Rectangle::new(Point::new(x, y + hh), Size::new(hw, hh)), fg),
        '▗' => quad(renderer, Rectangle::new(Point::new(x + hw, y + hh), Size::new(hw, hh)), fg),
        '▘' => quad(renderer, Rectangle::new(Point::new(x, y), Size::new(hw, hh)), fg),
        '▙' => {
            quad(renderer, Rectangle::new(Point::new(x, y), Size::new(hw, h)), fg);     // left half
            quad(renderer, Rectangle::new(Point::new(x + hw, y + hh), Size::new(hw, hh)), fg); // bottom right
        }
        '▚' => {
            quad(renderer, Rectangle::new(Point::new(x, y), Size::new(hw, hh)), fg);     // top left
            quad(renderer, Rectangle::new(Point::new(x + hw, y + hh), Size::new(hw, hh)), fg); // bottom right
        }
        '▛' => {
            quad(renderer, Rectangle::new(Point::new(x, y), Size::new(w, hh)), fg);      // top half
            quad(renderer, Rectangle::new(Point::new(x, y + hh), Size::new(hw, hh)), fg); // bottom left
        }
        '▜' => {
            quad(renderer, Rectangle::new(Point::new(x, y), Size::new(w, hh)), fg);       // top half
            quad(renderer, Rectangle::new(Point::new(x + hw, y + hh), Size::new(hw, hh)), fg); // bottom right
        }
        '▝' => quad(renderer, Rectangle::new(Point::new(x + hw, y), Size::new(hw, hh)), fg),
        '▞' => {
            quad(renderer, Rectangle::new(Point::new(x + hw, y), Size::new(hw, hh)), fg); // top right
            quad(renderer, Rectangle::new(Point::new(x, y + hh), Size::new(hw, hh)), fg); // bottom left
        }
        '▟' => {
            quad(renderer, Rectangle::new(Point::new(x + hw, y), Size::new(hw, hh)), fg); // top right
            quad(renderer, Rectangle::new(Point::new(x, y + hh), Size::new(w, hh)), fg);  // bottom half
        }

        // === Box-drawing characters (U+2500–U+257F) ===
        // Drawn as centered horizontal/vertical strokes within the cell.
        // Light strokes are ~1/8 cell width/height; heavy strokes are ~1/4.
        _ if ('\u{2500}'..='\u{257F}').contains(&c) => {
            draw_box_drawing(renderer, c, fg, cell);
        }

        _ => return false,
    }
    true
}

/// Draw box-drawing characters (U+2500–U+257F) as geometric strokes.
fn draw_box_drawing(
    renderer: &mut iced::Renderer,
    c: char,
    fg: Color,
    cell: Rectangle,
) {
    let x = cell.x;
    let y = cell.y;
    let w = cell.width;
    let h = cell.height;
    let cx = x + w / 2.0;
    let cy = y + h / 2.0;

    // Stroke thicknesses: light ≈ 1px (min), heavy ≈ 2–3px.
    let light = (w / 8.0).max(1.0);
    let heavy = (w / 4.0).max(2.0);

    let quad = |renderer: &mut iced::Renderer, bounds: Rectangle| {
        renderer.fill_quad(
            Quad {
                bounds,
                border: Border::default(),
                ..Quad::default()
            },
            fg,
        );
    };

    // Helper: draw a horizontal stroke segment.
    // Round the y position so thin strokes land on whole pixels.
    let h_stroke = |renderer: &mut iced::Renderer, x0: f32, x1: f32, thickness: f32| {
        quad(renderer, Rectangle::new(
            Point::new(x0, (cy - thickness / 2.0).round()),
            Size::new(x1 - x0, thickness),
        ));
    };

    // Helper: draw a vertical stroke segment.
    // Round the x position so thin strokes land on whole pixels.
    let v_stroke = |renderer: &mut iced::Renderer, y0: f32, y1: f32, thickness: f32| {
        quad(renderer, Rectangle::new(
            Point::new((cx - thickness / 2.0).round(), y0),
            Size::new(thickness, y1 - y0),
        ));
    };

    // Encode each box-drawing character as a combination of strokes extending
    // from the center of the cell toward its edges: left, right, up, down.
    // Each direction can be: none, light, or heavy.
    #[derive(Clone, Copy, PartialEq)]
    enum S { None, Light, Heavy }

    let (left, right, up, down) = match c {
        // Single horizontal/vertical lines
        '─' => (S::Light, S::Light, S::None, S::None),
        '━' => (S::Heavy, S::Heavy, S::None, S::None),
        '│' => (S::None, S::None, S::Light, S::Light),
        '┃' => (S::None, S::None, S::Heavy, S::Heavy),

        // Dashed lines — draw as solid (dashing would need more complex logic)
        '┄' | '┈' | '╌' => (S::Light, S::Light, S::None, S::None),
        '┅' | '┉' | '╍' => (S::Heavy, S::Heavy, S::None, S::None),
        '┆' | '┊' | '╎' => (S::None, S::None, S::Light, S::Light),
        '┇' | '┋' | '╏' => (S::None, S::None, S::Heavy, S::Heavy),

        // Corners: light
        '┌' => (S::None, S::Light, S::None, S::Light),
        '┐' => (S::Light, S::None, S::None, S::Light),
        '└' => (S::None, S::Light, S::Light, S::None),
        '┘' => (S::Light, S::None, S::Light, S::None),

        // Corners: heavy
        '┏' => (S::None, S::Heavy, S::None, S::Heavy),
        '┓' => (S::Heavy, S::None, S::None, S::Heavy),
        '┗' => (S::None, S::Heavy, S::Heavy, S::None),
        '┛' => (S::Heavy, S::None, S::Heavy, S::None),

        // Corners: mixed light/heavy
        '┍' => (S::None, S::Heavy, S::None, S::Light),
        '┎' => (S::None, S::Light, S::None, S::Heavy),
        '┑' => (S::Heavy, S::None, S::None, S::Light),
        '┒' => (S::Light, S::None, S::None, S::Heavy),
        '┕' => (S::None, S::Heavy, S::Light, S::None),
        '┖' => (S::None, S::Light, S::Heavy, S::None),
        '┙' => (S::Heavy, S::None, S::Light, S::None),
        '┚' => (S::Light, S::None, S::Heavy, S::None),

        // T-pieces: light
        '├' => (S::None, S::Light, S::Light, S::Light),
        '┤' => (S::Light, S::None, S::Light, S::Light),
        '┬' => (S::Light, S::Light, S::None, S::Light),
        '┴' => (S::Light, S::Light, S::Light, S::None),

        // T-pieces: heavy
        '┣' => (S::None, S::Heavy, S::Heavy, S::Heavy),
        '┫' => (S::Heavy, S::None, S::Heavy, S::Heavy),
        '┳' => (S::Heavy, S::Heavy, S::None, S::Heavy),
        '┻' => (S::Heavy, S::Heavy, S::Heavy, S::None),

        // T-pieces: mixed (most common combinations)
        '┝' => (S::None, S::Heavy, S::Light, S::Light),
        '┞' => (S::None, S::Light, S::Heavy, S::Light),
        '┟' => (S::None, S::Light, S::Light, S::Heavy),
        '┠' => (S::None, S::Light, S::Heavy, S::Heavy),
        '┡' => (S::None, S::Heavy, S::Heavy, S::Light),
        '┢' => (S::None, S::Heavy, S::Light, S::Heavy),
        '┥' => (S::Heavy, S::None, S::Light, S::Light),
        '┦' => (S::Light, S::None, S::Heavy, S::Light),
        '┧' => (S::Light, S::None, S::Light, S::Heavy),
        '┨' => (S::Light, S::None, S::Heavy, S::Heavy),
        '┩' => (S::Heavy, S::None, S::Heavy, S::Light),
        '┪' => (S::Heavy, S::None, S::Light, S::Heavy),
        '┭' => (S::Heavy, S::Light, S::None, S::Light),
        '┮' => (S::Light, S::Heavy, S::None, S::Light),
        '┯' => (S::Heavy, S::Heavy, S::None, S::Light),
        '┰' => (S::Light, S::Light, S::None, S::Heavy),
        '┱' => (S::Heavy, S::Light, S::None, S::Heavy),
        '┲' => (S::Light, S::Heavy, S::None, S::Heavy),
        '┵' => (S::Heavy, S::Light, S::Light, S::None),
        '┶' => (S::Light, S::Heavy, S::Light, S::None),
        '┷' => (S::Heavy, S::Heavy, S::Light, S::None),
        '┸' => (S::Light, S::Light, S::Heavy, S::None),
        '┹' => (S::Heavy, S::Light, S::Heavy, S::None),
        '┺' => (S::Light, S::Heavy, S::Heavy, S::None),

        // Cross: light
        '┼' => (S::Light, S::Light, S::Light, S::Light),
        // Cross: heavy
        '╋' => (S::Heavy, S::Heavy, S::Heavy, S::Heavy),
        // Cross: mixed (common combinations)
        '┽' => (S::Heavy, S::Light, S::Light, S::Light),
        '┾' => (S::Light, S::Heavy, S::Light, S::Light),
        '┿' => (S::Heavy, S::Heavy, S::Light, S::Light),
        '╀' => (S::Light, S::Light, S::Heavy, S::Light),
        '╁' => (S::Light, S::Light, S::Light, S::Heavy),
        '╂' => (S::Light, S::Light, S::Heavy, S::Heavy),
        '╃' => (S::Heavy, S::Light, S::Heavy, S::Light),
        '╄' => (S::Light, S::Heavy, S::Heavy, S::Light),
        '╅' => (S::Heavy, S::Light, S::Light, S::Heavy),
        '╆' => (S::Light, S::Heavy, S::Light, S::Heavy),
        '╇' => (S::Heavy, S::Heavy, S::Heavy, S::Light),
        '╈' => (S::Heavy, S::Heavy, S::Light, S::Heavy),
        '╉' => (S::Heavy, S::Light, S::Heavy, S::Heavy),
        '╊' => (S::Light, S::Heavy, S::Heavy, S::Heavy),

        // Half lines: light (single direction only)
        '\u{2574}' => (S::Light, S::None, S::None, S::None),  // ╴ left
        '\u{2575}' => (S::None, S::None, S::Light, S::None),  // ╵ up
        '\u{2576}' => (S::None, S::Light, S::None, S::None),  // ╶ right
        '\u{2577}' => (S::None, S::None, S::None, S::Light),  // ╷ down

        // Half lines: heavy
        '\u{2578}' => (S::Heavy, S::None, S::None, S::None),  // ╸ left
        '\u{2579}' => (S::None, S::None, S::Heavy, S::None),  // ╹ up
        '\u{257A}' => (S::None, S::Heavy, S::None, S::None),  // ╺ right
        '\u{257B}' => (S::None, S::None, S::None, S::Heavy),  // ╻ down

        // Half lines: mixed
        '\u{257C}' => (S::Light, S::Heavy, S::None, S::None),  // ╼ light left, heavy right
        '\u{257D}' => (S::None, S::None, S::Light, S::Heavy),  // ╽ light up, heavy down
        '\u{257E}' => (S::Heavy, S::Light, S::None, S::None),  // ╾ heavy left, light right
        '\u{257F}' => (S::None, S::None, S::Heavy, S::Light),  // ╿ heavy up, light down

        // Arc corners (rounded): drawn with quarter-circle arcs.
        '╭' | '╮' | '╰' | '╯' => {
            draw_arc_corner(renderer, c, fg, cell);
            return;
        }

        // Diagonal lines (U+2571–U+2573): drawn as stroked paths.
        '\u{2571}' | '\u{2572}' | '\u{2573}' => {
            draw_diagonal(renderer, c, fg, cell);
            return;
        }

        // Double lines and double/single combinations (U+2550–U+256C):
        // drawn as parallel strokes.
        '\u{2550}'..='\u{256C}' => {
            draw_double(renderer, c, fg, cell);
            return;
        }

        _ => return,
    };

    let t = |s: S| match s {
        S::None => 0.0,
        S::Light => light,
        S::Heavy => heavy,
    };

    if left != S::None {
        h_stroke(renderer, x, cx + t(left) / 2.0, t(left));
    }
    if right != S::None {
        h_stroke(renderer, cx - t(right) / 2.0, x + w, t(right));
    }
    if up != S::None {
        v_stroke(renderer, y, cy + t(up) / 2.0, t(up));
    }
    if down != S::None {
        v_stroke(renderer, cy - t(down) / 2.0, y + h, t(down));
    }
}

/// Draw arc corner characters (╭ ╮ ╰ ╯) as quarter-circle arcs with
/// straight extensions to the cell edges.
fn draw_arc_corner(
    renderer: &mut iced::Renderer,
    c: char,
    fg: Color,
    cell: Rectangle,
) {
    use iced::advanced::graphics::geometry::Renderer as _;

    let x = cell.x;
    let y = cell.y;
    let w = cell.width;
    let h = cell.height;
    let hw = w / 2.0;
    let hh = h / 2.0;

    let thickness = (w / 8.0).max(1.0);
    let r = hw.min(hh);

    // Match the pixel-rounding used by draw_box_drawing's h_stroke/v_stroke
    // so the arc endpoints align exactly with adjacent straight lines.
    let cx = (x + hw - thickness / 2.0).round() + thickness / 2.0;
    let cy = (y + hh - thickness / 2.0).round() + thickness / 2.0;

    // Absolute coordinates — the frame doesn't remap to local space.
    // Each arc corner: straight extension from edge to arc start, circular
    // arc_to through the center, straight extension from arc end to edge.
    let path = canvas::Path::new(|b| {
        match c {
            '╭' => {
                b.move_to(Point::new(x + w, cy));
                b.line_to(Point::new(cx + r, cy));
                b.arc_to(Point::new(cx, cy), Point::new(cx, cy + r), r);
                b.line_to(Point::new(cx, y + h));
            }
            '╮' => {
                b.move_to(Point::new(x, cy));
                b.line_to(Point::new(cx - r, cy));
                b.arc_to(Point::new(cx, cy), Point::new(cx, cy + r), r);
                b.line_to(Point::new(cx, y + h));
            }
            '╰' => {
                b.move_to(Point::new(x + w, cy));
                b.line_to(Point::new(cx + r, cy));
                b.arc_to(Point::new(cx, cy), Point::new(cx, cy - r), r);
                b.line_to(Point::new(cx, y));
            }
            '╯' => {
                b.move_to(Point::new(x, cy));
                b.line_to(Point::new(cx - r, cy));
                b.arc_to(Point::new(cx, cy), Point::new(cx, cy - r), r);
                b.line_to(Point::new(cx, y));
            }
            _ => unreachable!(),
        }
    });

    let mut frame = canvas::Frame::with_bounds(renderer, cell);
    frame.stroke(
        &path,
        Stroke {
            style: Style::Solid(fg),
            width: thickness,
            line_cap: LineCap::Butt,
            ..Stroke::default()
        },
    );
    renderer.draw_geometry(frame.into_geometry());
}

/// Draw diagonal line characters (U+2571–U+2573) as stroked paths.
fn draw_diagonal(
    renderer: &mut iced::Renderer,
    c: char,
    fg: Color,
    cell: Rectangle,
) {
    use iced::advanced::graphics::geometry::Renderer as _;

    let x = cell.x;
    let y = cell.y;
    let w = cell.width;
    let h = cell.height;
    let thickness = (w / 8.0).max(1.0);

    let path = canvas::Path::new(|b| {
        match c {
            '\u{2571}' => {
                // ╱ forward slash: bottom-left to top-right
                b.move_to(Point::new(x, y + h));
                b.line_to(Point::new(x + w, y));
            }
            '\u{2572}' => {
                // ╲ backslash: top-left to bottom-right
                b.move_to(Point::new(x, y));
                b.line_to(Point::new(x + w, y + h));
            }
            '\u{2573}' => {
                // ╳ X: both diagonals
                b.move_to(Point::new(x, y + h));
                b.line_to(Point::new(x + w, y));
                b.move_to(Point::new(x, y));
                b.line_to(Point::new(x + w, y + h));
            }
            _ => unreachable!(),
        }
    });

    let mut frame = canvas::Frame::with_bounds(renderer, cell);
    frame.stroke(
        &path,
        Stroke {
            style: Style::Solid(fg),
            width: thickness,
            line_cap: LineCap::Butt,
            ..Stroke::default()
        },
    );
    renderer.draw_geometry(frame.into_geometry());
}

/// Draw double-line box-drawing characters (U+2550–U+256C) as parallel strokes.
fn draw_double(
    renderer: &mut iced::Renderer,
    c: char,
    fg: Color,
    cell: Rectangle,
) {
    let x = cell.x;
    let y = cell.y;
    let w = cell.width;
    let h = cell.height;
    let cx = x + w / 2.0;
    let cy = y + h / 2.0;

    let light = (w / 8.0).max(1.0);
    // Gap between the two parallel lines, and the offset from center.
    let gap = (w / 4.0).max(2.0);
    let off = gap / 2.0;

    let quad = |renderer: &mut iced::Renderer, bounds: Rectangle| {
        renderer.fill_quad(
            Quad {
                bounds,
                border: Border::default(),
                ..Quad::default()
            },
            fg,
        );
    };

    // Horizontal double stroke: two lines offset above/below cy.
    let hd = |renderer: &mut iced::Renderer, x0: f32, x1: f32| {
        quad(renderer, Rectangle::new(
            Point::new(x0, (cy - off - light / 2.0).round()),
            Size::new(x1 - x0, light),
        ));
        quad(renderer, Rectangle::new(
            Point::new(x0, (cy + off - light / 2.0).round()),
            Size::new(x1 - x0, light),
        ));
    };

    // Vertical double stroke: two lines offset left/right of cx.
    let vd = |renderer: &mut iced::Renderer, y0: f32, y1: f32| {
        quad(renderer, Rectangle::new(
            Point::new((cx - off - light / 2.0).round(), y0),
            Size::new(light, y1 - y0),
        ));
        quad(renderer, Rectangle::new(
            Point::new((cx + off - light / 2.0).round(), y0),
            Size::new(light, y1 - y0),
        ));
    };

    // Single horizontal stroke (for double/single combos).
    let hs = |renderer: &mut iced::Renderer, x0: f32, x1: f32| {
        quad(renderer, Rectangle::new(
            Point::new(x0, (cy - light / 2.0).round()),
            Size::new(x1 - x0, light),
        ));
    };

    // Single vertical stroke (for double/single combos).
    let vs = |renderer: &mut iced::Renderer, y0: f32, y1: f32| {
        quad(renderer, Rectangle::new(
            Point::new((cx - light / 2.0).round(), y0),
            Size::new(light, y1 - y0),
        ));
    };

    // Outer edges of the double strokes, for connecting perpendicular lines.
    let lo = (cx - off - light / 2.0).round();
    let ro = (cx + off - light / 2.0).round() + light;
    let to = (cy - off - light / 2.0).round();
    let bo = (cy + off - light / 2.0).round() + light;

    match c {
        // Straight lines
        '\u{2550}' => hd(renderer, x, x + w),          // ═
        '\u{2551}' => vd(renderer, y, y + h),           // ║

        // Double corners
        '\u{2554}' => {                                  // ╔
            hd(renderer, cx, x + w);
            // Left line: from top of lower h-stroke to bottom
            quad(renderer, Rectangle::new(Point::new(lo, bo), Size::new(light, y + h - bo)));
            // Right line: from top of upper h-stroke to bottom
            quad(renderer, Rectangle::new(Point::new(ro - light, to), Size::new(light, y + h - to)));
        }
        '\u{2557}' => {                                  // ╗
            hd(renderer, x, cx);
            quad(renderer, Rectangle::new(Point::new(lo, to), Size::new(light, y + h - to)));
            quad(renderer, Rectangle::new(Point::new(ro - light, bo), Size::new(light, y + h - bo)));
        }
        '\u{255A}' => {                                  // ╚
            hd(renderer, cx, x + w);
            quad(renderer, Rectangle::new(Point::new(lo, y), Size::new(light, bo - y)));
            quad(renderer, Rectangle::new(Point::new(ro - light, y), Size::new(light, to + light - y)));
        }
        '\u{255D}' => {                                  // ╝
            hd(renderer, x, cx);
            quad(renderer, Rectangle::new(Point::new(lo, y), Size::new(light, to + light - y)));
            quad(renderer, Rectangle::new(Point::new(ro - light, y), Size::new(light, bo - y)));
        }

        // Double/single corners: single horizontal, double vertical
        '\u{2553}' => {                                  // ╓
            hs(renderer, cx, x + w);
            vd(renderer, cy, y + h);
        }
        '\u{2556}' => {                                  // ╖
            hs(renderer, x, cx);
            vd(renderer, cy, y + h);
        }
        '\u{2559}' => {                                  // ╙
            hs(renderer, cx, x + w);
            vd(renderer, y, cy);
        }
        '\u{255C}' => {                                  // ╜
            hs(renderer, x, cx);
            vd(renderer, y, cy);
        }

        // Double/single corners: double horizontal, single vertical
        '\u{2552}' => {                                  // ╒
            hd(renderer, cx, x + w);
            vs(renderer, cy, y + h);
        }
        '\u{2555}' => {                                  // ╕
            hd(renderer, x, cx);
            vs(renderer, cy, y + h);
        }
        '\u{2558}' => {                                  // ╘
            hd(renderer, cx, x + w);
            vs(renderer, y, cy);
        }
        '\u{255B}' => {                                  // ╛
            hd(renderer, x, cx);
            vs(renderer, y, cy);
        }

        // Double T-pieces
        '\u{2560}' => {                                  // ╠
            hd(renderer, cx, x + w);
            quad(renderer, Rectangle::new(Point::new(lo, y), Size::new(light, to + light - y)));
            quad(renderer, Rectangle::new(Point::new(lo, bo), Size::new(light, y + h - bo)));
            quad(renderer, Rectangle::new(Point::new(ro - light, y), Size::new(light, y + h - y)));
        }
        '\u{2563}' => {                                  // ╣
            hd(renderer, x, cx);
            quad(renderer, Rectangle::new(Point::new(lo, y), Size::new(light, y + h - y)));
            quad(renderer, Rectangle::new(Point::new(ro - light, y), Size::new(light, to + light - y)));
            quad(renderer, Rectangle::new(Point::new(ro - light, bo), Size::new(light, y + h - bo)));
        }
        '\u{2566}' => {                                  // ╦
            // Vertical double: from center down
            vd(renderer, cy, y + h);
            // Top h-line: spans full width
            quad(renderer, Rectangle::new(Point::new(x, to), Size::new(w, light)));
            // Bottom h-line: left segment and right segment (broken by vertical)
            quad(renderer, Rectangle::new(Point::new(x, bo - light), Size::new(lo + light - x, light)));
            quad(renderer, Rectangle::new(Point::new(ro - light, bo - light), Size::new(x + w - ro + light, light)));
        }
        '\u{2569}' => {                                  // ╩
            // Vertical double: from top to center
            vd(renderer, y, cy);
            // Top h-line: left segment and right segment (broken by vertical)
            quad(renderer, Rectangle::new(Point::new(x, to), Size::new(lo + light - x, light)));
            quad(renderer, Rectangle::new(Point::new(ro - light, to), Size::new(x + w - ro + light, light)));
            // Bottom h-line: spans full width
            quad(renderer, Rectangle::new(Point::new(x, bo - light), Size::new(w, light)));
        }

        // Double/single T-pieces: single horizontal, double vertical
        '\u{255F}' => {                                  // ╟
            hs(renderer, cx, x + w);
            vd(renderer, y, y + h);
        }
        '\u{2562}' => {                                  // ╢
            hs(renderer, x, cx);
            vd(renderer, y, y + h);
        }
        '\u{2565}' => {                                  // ╥
            vs(renderer, cy, y + h);
            hd(renderer, x, x + w);
        }
        '\u{2568}' => {                                  // ╨
            vs(renderer, y, cy);
            hd(renderer, x, x + w);
        }

        // Double/single T-pieces: double horizontal, single vertical
        '\u{255E}' => {                                  // ╞
            hd(renderer, cx, x + w);
            vs(renderer, y, y + h);
        }
        '\u{2561}' => {                                  // ╡
            hd(renderer, x, cx);
            vs(renderer, y, y + h);
        }
        '\u{2564}' => {                                  // ╤
            vs(renderer, cy, y + h);
            hd(renderer, x, x + w);
        }
        '\u{2567}' => {                                  // ╧
            vs(renderer, y, cy);
            hd(renderer, x, x + w);
        }

        // Double cross
        '\u{256C}' => {                                  // ╬
            // Four horizontal segments (two double lines broken by the vertical)
            quad(renderer, Rectangle::new(Point::new(x, to), Size::new(lo + light - x, light)));
            quad(renderer, Rectangle::new(Point::new(ro - light, to), Size::new(x + w - ro + light, light)));
            quad(renderer, Rectangle::new(Point::new(x, bo - light), Size::new(lo + light - x, light)));
            quad(renderer, Rectangle::new(Point::new(ro - light, bo - light), Size::new(x + w - ro + light, light)));
            // Four vertical segments (two double lines broken by the horizontal)
            quad(renderer, Rectangle::new(Point::new(lo, y), Size::new(light, to + light - y)));
            quad(renderer, Rectangle::new(Point::new(lo, bo - light), Size::new(light, y + h - bo + light)));
            quad(renderer, Rectangle::new(Point::new(ro - light, y), Size::new(light, to + light - y)));
            quad(renderer, Rectangle::new(Point::new(ro - light, bo - light), Size::new(light, y + h - bo + light)));
        }

        // Double/single crosses
        '\u{256A}' => {                                  // ╪ double horizontal, single vertical
            hd(renderer, x, x + w);
            vs(renderer, y, y + h);
        }
        '\u{256B}' => {                                  // ╫ single horizontal, double vertical
            hs(renderer, x, x + w);
            vd(renderer, y, y + h);
        }

        _ => {}
    }
}

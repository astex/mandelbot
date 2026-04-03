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

        // === Arrow characters (U+2190–U+2193) ===
        '\u{2190}'..='\u{2193}' => {
            draw_arrow(renderer, c, fg, cell);
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
        h_stroke(renderer, x, cx, t(left));
    }
    if right != S::None {
        h_stroke(renderer, cx, x + w, t(right));
    }
    if up != S::None {
        v_stroke(renderer, y, cy, t(up));
    }
    if down != S::None {
        v_stroke(renderer, cy, y + h, t(down));
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

    // Individual line helpers for when the two double lines need different extents.
    let h_upper = |renderer: &mut iced::Renderer, x0: f32, x1: f32| {
        quad(renderer, Rectangle::new(Point::new(x0, to), Size::new(x1 - x0, light)));
    };
    let h_lower = |renderer: &mut iced::Renderer, x0: f32, x1: f32| {
        quad(renderer, Rectangle::new(Point::new(x0, bo - light), Size::new(x1 - x0, light)));
    };
    let v_left = |renderer: &mut iced::Renderer, y0: f32, y1: f32| {
        quad(renderer, Rectangle::new(Point::new(lo, y0), Size::new(light, y1 - y0)));
    };
    let v_right = |renderer: &mut iced::Renderer, y0: f32, y1: f32| {
        quad(renderer, Rectangle::new(Point::new(ro - light, y0), Size::new(light, y1 - y0)));
    };

    match c {
        // Straight lines
        '\u{2550}' => hd(renderer, x, x + w),          // ═
        '\u{2551}' => vd(renderer, y, y + h),           // ║

        // Double corners — each is two nested L-shapes.
        '\u{2554}' => {                                  // ╔ right + down
            h_upper(renderer, lo, x + w);                // outer: h from left-v to right edge
            v_left(renderer, to, y + h);                 // outer: v from upper-h to bottom edge
            h_lower(renderer, ro - light, x + w);        // inner: h from right-v to right edge
            v_right(renderer, bo, y + h);                // inner: v from lower-h to bottom edge
        }
        '\u{2557}' => {                                  // ╗ left + down
            h_upper(renderer, x, ro);                    // outer: h from left edge to right-v
            v_right(renderer, to, y + h);                // outer: v from upper-h to bottom edge
            h_lower(renderer, x, lo + light);            // inner: h from left edge to left-v
            v_left(renderer, bo, y + h);                 // inner: v from lower-h to bottom edge
        }
        '\u{255A}' => {                                  // ╚ right + up
            h_lower(renderer, lo, x + w);                // outer: h from left-v to right edge
            v_left(renderer, y, bo - light);             // outer: v from top edge to lower-h
            h_upper(renderer, ro - light, x + w);        // inner: h from right-v to right edge
            v_right(renderer, y, to + light);            // inner: v from top edge to upper-h
        }
        '\u{255D}' => {                                  // ╝ left + up
            h_lower(renderer, x, ro);                    // outer: h from left edge to right-v
            v_right(renderer, y, bo - light);            // outer: v from top edge to lower-h
            h_upper(renderer, x, lo + light);            // inner: h from left edge to left-v
            v_left(renderer, y, to + light);             // inner: v from top edge to upper-h
        }

        // Double/single corners: single horizontal, double vertical
        '\u{2553}' => {                                  // ╓ right(s) + down(d)
            hs(renderer, cx, x + w);
            vd(renderer, cy, y + h);
        }
        '\u{2556}' => {                                  // ╖ left(s) + down(d)
            hs(renderer, x, cx);
            vd(renderer, cy, y + h);
        }
        '\u{2559}' => {                                  // ╙ right(s) + up(d)
            hs(renderer, cx, x + w);
            vd(renderer, y, cy);
        }
        '\u{255C}' => {                                  // ╜ left(s) + up(d)
            hs(renderer, x, cx);
            vd(renderer, y, cy);
        }

        // Double/single corners: double horizontal, single vertical
        '\u{2552}' => {                                  // ╒ right(d) + down(s)
            hd(renderer, cx, x + w);
            vs(renderer, cy, y + h);
        }
        '\u{2555}' => {                                  // ╕ left(d) + down(s)
            hd(renderer, x, cx);
            vs(renderer, cy, y + h);
        }
        '\u{2558}' => {                                  // ╘ right(d) + up(s)
            hd(renderer, cx, x + w);
            vs(renderer, y, cy);
        }
        '\u{255B}' => {                                  // ╛ left(d) + up(s)
            hd(renderer, x, cx);
            vs(renderer, y, cy);
        }

        // Double T-pieces — continuous line on one axis, two stubs on the other.
        '\u{2560}' => {                                  // ╠ right + up/down
            h_upper(renderer, ro - light, x + w);        // upper h from right-v
            h_lower(renderer, ro - light, x + w);        // lower h from right-v
            v_left(renderer, y, y + h);                  // left v continuous
            v_right(renderer, y, to + light);            // right v: top segment
            v_right(renderer, bo - light, y + h);        // right v: bottom segment
        }
        '\u{2563}' => {                                  // ╣ left + up/down
            h_upper(renderer, x, lo + light);            // upper h to left-v
            h_lower(renderer, x, lo + light);            // lower h to left-v
            v_right(renderer, y, y + h);                 // right v continuous
            v_left(renderer, y, to + light);             // left v: top segment
            v_left(renderer, bo - light, y + h);         // left v: bottom segment
        }
        '\u{2566}' => {                                  // ╦ left/right + down
            h_upper(renderer, x, x + w);                 // upper h continuous
            h_lower(renderer, x, lo + light);            // lower h: left segment
            h_lower(renderer, ro - light, x + w);        // lower h: right segment
            v_left(renderer, bo - light, y + h);         // left v from lower-h
            v_right(renderer, bo - light, y + h);        // right v from lower-h
        }
        '\u{2569}' => {                                  // ╩ left/right + up
            h_lower(renderer, x, x + w);                 // lower h continuous
            h_upper(renderer, x, lo + light);            // upper h: left segment
            h_upper(renderer, ro - light, x + w);        // upper h: right segment
            v_left(renderer, y, to + light);             // left v to upper-h
            v_right(renderer, y, to + light);            // right v to upper-h
        }

        // Double/single T-pieces: single horizontal, double vertical
        '\u{255F}' => {                                  // ╟ right(s) + up/down(d)
            hs(renderer, cx, x + w);
            vd(renderer, y, y + h);
        }
        '\u{2562}' => {                                  // ╢ left(s) + up/down(d)
            hs(renderer, x, cx);
            vd(renderer, y, y + h);
        }
        '\u{2565}' => {                                  // ╥ left/right(s) + down(d)
            hs(renderer, x, x + w);
            vd(renderer, cy, y + h);
        }
        '\u{2568}' => {                                  // ╨ left/right(s) + up(d)
            hs(renderer, x, x + w);
            vd(renderer, y, cy);
        }

        // Double/single T-pieces: double horizontal, single vertical
        '\u{255E}' => {                                  // ╞ right(d) + up/down(s)
            hd(renderer, cx, x + w);
            vs(renderer, y, y + h);
        }
        '\u{2561}' => {                                  // ╡ left(d) + up/down(s)
            hd(renderer, x, cx);
            vs(renderer, y, y + h);
        }
        '\u{2564}' => {                                  // ╤ left/right(d) + down(s)
            hd(renderer, x, x + w);
            vs(renderer, cy, y + h);
        }
        '\u{2567}' => {                                  // ╧ left/right(d) + up(s)
            hd(renderer, x, x + w);
            vs(renderer, y, cy);
        }

        // Double cross
        '\u{256C}' => {                                  // ╬
            // Upper h-line: left and right segments
            h_upper(renderer, x, lo + light);
            h_upper(renderer, ro - light, x + w);
            // Lower h-line: left and right segments
            h_lower(renderer, x, lo + light);
            h_lower(renderer, ro - light, x + w);
            // Left v-line: top and bottom segments
            v_left(renderer, y, to + light);
            v_left(renderer, bo - light, y + h);
            // Right v-line: top and bottom segments
            v_right(renderer, y, to + light);
            v_right(renderer, bo - light, y + h);
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

/// Draw arrow characters (U+2190–U+2193) as a line shaft with a filled arrowhead.
/// The shaft matches box-drawing light line thickness so arrows connect seamlessly
/// with ─ and │ characters.
fn draw_arrow(
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
    let cx = x + w / 2.0;
    let cy = y + h / 2.0;
    let thickness = (w / 8.0).max(1.0);

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

    // Arrowhead dimensions proportional to cell size.
    let h_arrow_len = w * 0.35;   // length along shaft for horizontal arrows
    let h_arrow_half = h * 0.3;   // half-width perpendicular for horizontal arrows
    let v_arrow_len = h * 0.35;   // length along shaft for vertical arrows
    let v_arrow_half = w * 0.4;   // half-width perpendicular for vertical arrows

    // Match box-drawing centering: round the shaft position the same way.
    let shaft_y = (cy - thickness / 2.0).round();
    let shaft_cy = shaft_y + thickness / 2.0; // true visual center of the shaft
    let shaft_x = (cx - thickness / 2.0).round();
    let shaft_cx = shaft_x + thickness / 2.0;

    let mut frame = canvas::Frame::with_bounds(renderer, cell);

    match c {
        '\u{2190}' => {
            // ← leftward: shaft from right edge to arrowhead base, head at left
            let base = x + h_arrow_len;
            quad(renderer, Rectangle::new(
                Point::new(base, shaft_y),
                Size::new(x + w - base, thickness),
            ));
            let head = canvas::Path::new(|b| {
                b.move_to(Point::new(x, shaft_cy));
                b.line_to(Point::new(base, shaft_cy - h_arrow_half));
                b.line_to(Point::new(base, shaft_cy + h_arrow_half));
                b.close();
            });
            frame.fill(&head, fg);
        }
        '\u{2191}' => {
            // ↑ upward: shaft from bottom edge to arrowhead base, head at top
            let base = y + v_arrow_len;
            quad(renderer, Rectangle::new(
                Point::new(shaft_x, base),
                Size::new(thickness, y + h - base),
            ));
            let head = canvas::Path::new(|b| {
                b.move_to(Point::new(shaft_cx, y));
                b.line_to(Point::new(shaft_cx - v_arrow_half, base));
                b.line_to(Point::new(shaft_cx + v_arrow_half, base));
                b.close();
            });
            frame.fill(&head, fg);
        }
        '\u{2192}' => {
            // → rightward: shaft from left edge to arrowhead base, head at right
            let base = x + w - h_arrow_len;
            quad(renderer, Rectangle::new(
                Point::new(x, shaft_y),
                Size::new(base - x, thickness),
            ));
            let head = canvas::Path::new(|b| {
                b.move_to(Point::new(x + w, shaft_cy));
                b.line_to(Point::new(base, shaft_cy - h_arrow_half));
                b.line_to(Point::new(base, shaft_cy + h_arrow_half));
                b.close();
            });
            frame.fill(&head, fg);
        }
        '\u{2193}' => {
            // ↓ downward: shaft from top edge to arrowhead base, head at bottom
            let base = y + h - v_arrow_len;
            quad(renderer, Rectangle::new(
                Point::new(shaft_x, y),
                Size::new(thickness, base - y),
            ));
            let head = canvas::Path::new(|b| {
                b.move_to(Point::new(shaft_cx, y + h));
                b.line_to(Point::new(shaft_cx - v_arrow_half, base));
                b.line_to(Point::new(shaft_cx + v_arrow_half, base));
                b.close();
            });
            frame.fill(&head, fg);
        }
        _ => unreachable!(),
    }

    renderer.draw_geometry(frame.into_geometry());
}

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

        // Double lines and double/single combinations (U+2550–U+256C)
        // These have more complex geometry; fall back to font for now.
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

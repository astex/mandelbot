use alacritty_terminal::term::cell::Flags;

use iced::advanced::renderer::Quad;
use iced::advanced::Renderer as _;
use iced::{Border, Color, Point, Rectangle, Size};

/// Draw the appropriate underline decoration for the given cell flags.
pub fn draw_underline(
    renderer: &mut iced::Renderer,
    flags: Flags,
    x: f32,
    y: f32,
    width: f32,
    thickness: f32,
    color: Color,
) {
    if flags.contains(Flags::UNDERCURL) {
        draw_curly(renderer, x, y, width, thickness, color);
    } else if flags.contains(Flags::DOTTED_UNDERLINE) {
        draw_dotted(renderer, x, y, width, thickness, color);
    } else if flags.contains(Flags::DASHED_UNDERLINE) {
        draw_dashed(renderer, x, y, width, thickness, color);
    } else if flags.contains(Flags::DOUBLE_UNDERLINE) {
        draw_solid(renderer, x, y - thickness * 2.0, width, thickness, color);
        draw_solid(renderer, x, y, width, thickness, color);
    } else {
        draw_solid(renderer, x, y, width, thickness, color);
    }
}

fn draw_solid(
    renderer: &mut iced::Renderer,
    x: f32,
    y: f32,
    width: f32,
    thickness: f32,
    color: Color,
) {
    renderer.fill_quad(
        Quad {
            bounds: Rectangle::new(Point::new(x, y), Size::new(width, thickness)),
            border: Border::default(),
            ..Quad::default()
        },
        color,
    );
}

fn draw_dotted(
    renderer: &mut iced::Renderer,
    x: f32,
    y: f32,
    width: f32,
    thickness: f32,
    color: Color,
) {
    let dot = thickness;
    let stride = dot * 4.0;
    let count = (width / stride).floor().max(1.0) as usize;
    for i in 0..count {
        renderer.fill_quad(
            Quad {
                bounds: Rectangle::new(
                    Point::new(x + i as f32 * stride, y),
                    Size::new(dot, thickness),
                ),
                border: Border::default(),
                ..Quad::default()
            },
            color,
        );
    }
}

fn draw_dashed(
    renderer: &mut iced::Renderer,
    x: f32,
    y: f32,
    width: f32,
    thickness: f32,
    color: Color,
) {
    let dash_width = width * 0.4;
    let gap = width * 0.1;
    let stride = dash_width + gap;
    let mut cx = x;
    while cx < x + width {
        let w = dash_width.min(x + width - cx);
        renderer.fill_quad(
            Quad {
                bounds: Rectangle::new(Point::new(cx, y), Size::new(w, thickness)),
                border: Border::default(),
                ..Quad::default()
            },
            color,
        );
        cx += stride;
    }
}

fn draw_curly(
    renderer: &mut iced::Renderer,
    x: f32,
    y: f32,
    width: f32,
    thickness: f32,
    color: Color,
) {
    // Trace connected semicircles: up then down, each spanning half the cell.
    let amplitude = thickness * 2.0;
    let steps = 16_usize;
    let step_w = width / steps as f32;

    for i in 0..steps {
        let t = i as f32 / steps as f32;
        // First half: upward semicircle. Second half: downward semicircle.
        let dy = if t < 0.5 {
            let a = t * 2.0 * std::f32::consts::PI;
            -a.sin() * amplitude
        } else {
            let a = (t - 0.5) * 2.0 * std::f32::consts::PI;
            a.sin() * amplitude
        };
        renderer.fill_quad(
            Quad {
                bounds: Rectangle::new(
                    Point::new(x + i as f32 * step_w, y + dy),
                    Size::new(step_w.ceil(), thickness),
                ),
                border: Border::default(),
                ..Quad::default()
            },
            color,
        );
    }
}

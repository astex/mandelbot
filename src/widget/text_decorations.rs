use iced::advanced::renderer::Quad;
use iced::advanced::Renderer as _;
use iced::{Border, Color, Point, Rectangle, Size};

pub fn draw_solid_underline(
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

pub fn draw_dotted_underline(
    renderer: &mut iced::Renderer,
    x: f32,
    y: f32,
    width: f32,
    thickness: f32,
    color: Color,
) {
    let dot_width = thickness;
    let gap = thickness * 2.0;
    let stride = dot_width + gap;
    let mut cx = x;
    while cx < x + width {
        let w = dot_width.min(x + width - cx);
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

pub fn draw_dashed_underline(
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

pub fn draw_curly_underline(
    renderer: &mut iced::Renderer,
    x: f32,
    y: f32,
    width: f32,
    thickness: f32,
    color: Color,
) {
    // Approximate a sine wave with small quads.
    let amplitude = thickness * 2.0;
    let steps = (width / thickness).ceil().max(4.0) as usize;
    let step_width = width / steps as f32;
    for i in 0..steps {
        let t = i as f32 / steps as f32;
        let angle = t * std::f32::consts::TAU;
        let dy = angle.sin() * amplitude;
        renderer.fill_quad(
            Quad {
                bounds: Rectangle::new(
                    Point::new(x + i as f32 * step_width, y + dy),
                    Size::new(step_width.ceil(), thickness),
                ),
                border: Border::default(),
                ..Quad::default()
            },
            color,
        );
    }
}

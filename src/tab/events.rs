use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::term::{ClipboardType, Config};
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::vte::ansi::Rgb;
use alacritty_terminal::Term;

/// Theme colors converted to alacritty's Rgb for responding to color
/// queries.
#[derive(Clone, Copy)]
pub(crate) struct TermColors {
    pub(crate) fg: Rgb,
    pub(crate) bg: Rgb,
    pub(crate) cursor: Rgb,
}

impl Default for TermColors {
    fn default() -> Self {
        Self {
            fg: Rgb { r: 0x83, g: 0x94, b: 0x96 },
            bg: Rgb { r: 0x00, g: 0x2b, b: 0x36 },
            cursor: Rgb { r: 0x83, g: 0x94, b: 0x96 },
        }
    }
}

/// A clipboard store request captured from the terminal.
pub struct ClipboardStoreRequest {
    pub clipboard_type: ClipboardType,
    pub text: String,
}

/// A clipboard load request captured from the terminal.
/// The `formatter` converts clipboard text into the escape sequence
/// to write back.
pub struct ClipboardLoadRequest {
    pub clipboard_type: ClipboardType,
    pub formatter:
        Arc<dyn Fn(&str) -> String + Sync + Send + 'static>,
}

#[derive(Clone)]
pub(crate) struct TermEventListener {
    pub(crate) title: Arc<Mutex<Option<String>>>,
    pub(crate) bell: Arc<AtomicBool>,
    pub(crate) colors: Arc<Mutex<TermColors>>,
    pub(crate) window_size: Arc<Mutex<WindowSize>>,
    /// Responses to write back to the PTY, drained after each feed.
    pub(crate) pty_responses: Arc<Mutex<Vec<String>>>,
    pub(crate) clipboard_stores:
        Arc<Mutex<Vec<ClipboardStoreRequest>>>,
    pub(crate) clipboard_loads:
        Arc<Mutex<Vec<ClipboardLoadRequest>>>,
}

impl TermEventListener {
    pub(crate) fn new() -> Self {
        Self {
            title: Arc::new(Mutex::new(None)),
            bell: Arc::new(AtomicBool::new(false)),
            colors: Arc::new(Mutex::new(TermColors::default())),
            window_size: Arc::new(Mutex::new(WindowSize {
                num_lines: 24,
                num_cols: 80,
                cell_width: 0,
                cell_height: 0,
            })),
            pty_responses: Arc::new(Mutex::new(Vec::new())),
            clipboard_stores: Arc::new(Mutex::new(Vec::new())),
            clipboard_loads: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl EventListener for TermEventListener {
    fn send_event(&self, event: Event) {
        match event {
            Event::Title(t) => {
                *self.title.lock().unwrap() = Some(t);
            }
            Event::ResetTitle => {
                *self.title.lock().unwrap() = None;
            }
            Event::Bell => {
                self.bell.store(true, Ordering::Relaxed);
            }
            Event::ColorRequest(index, callback) => {
                let colors = *self.colors.lock().unwrap();
                let rgb = match index {
                    // OSC 10 = foreground, 11 = background,
                    // 12 = cursor.
                    11 => colors.bg,
                    12 => colors.cursor,
                    _ => colors.fg,
                };
                let response = callback(rgb);
                self.pty_responses
                    .lock()
                    .unwrap()
                    .push(response);
            }
            Event::TextAreaSizeRequest(callback) => {
                let size = *self.window_size.lock().unwrap();
                let response = callback(size);
                self.pty_responses
                    .lock()
                    .unwrap()
                    .push(response);
            }
            Event::ClipboardStore(clipboard_type, text) => {
                self.clipboard_stores
                    .lock()
                    .unwrap()
                    .push(ClipboardStoreRequest {
                        clipboard_type,
                        text,
                    });
            }
            Event::ClipboardLoad(clipboard_type, formatter) => {
                self.clipboard_loads
                    .lock()
                    .unwrap()
                    .push(ClipboardLoadRequest {
                        clipboard_type,
                        formatter,
                    });
            }
            _ => {}
        }
    }
}

pub(crate) type TermInstance = Term<TermEventListener>;

/// Create a new terminal instance with the given dimensions.
pub(crate) fn new_term(
    cols: usize,
    rows: usize,
) -> (TermInstance, TermEventListener) {
    let size = TermSize::new(cols, rows);
    let listener = TermEventListener::new();
    let term = Term::new(
        Config::default(),
        &size,
        listener.clone(),
    );
    (term, listener)
}

/// Convert an iced `Color` (0.0–1.0 floats) to alacritty's `Rgb`.
pub(crate) fn color_to_rgb(c: iced::Color) -> Rgb {
    Rgb {
        r: (c.r * 255.0).round() as u8,
        g: (c.g * 255.0).round() as u8,
        b: (c.b * 255.0).round() as u8,
    }
}

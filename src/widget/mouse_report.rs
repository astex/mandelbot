//! Encoding of xterm mouse reports.
//!
//! Pure functions: grid coordinates and modifiers in, PTY bytes out. The
//! caller decides *whether* to report (based on `TermMode`); these functions
//! only decide *how*.

use alacritty_terminal::term::TermMode;

use iced::keyboard;

/// Largest 1-based coordinate representable in the legacy X10 encoding —
/// `32 + 223 == 255`, the top of a single byte.
const X10_MAX_COORD: usize = 223;

/// Largest 1-based coordinate xterm accepts in UTF-8 extended mode.
const UTF8_MAX_COORD: usize = 2015;

/// Button identity, valued as the low bits of the report's button byte.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    /// Motion with no button held.
    NoButton,
    WheelUp,
    WheelDown,
}

impl MouseButton {
    fn code(self) -> u8 {
        match self {
            Self::Left => 0,
            Self::Middle => 1,
            Self::Right => 2,
            Self::NoButton => 3,
            Self::WheelUp => 64,
            Self::WheelDown => 65,
        }
    }
}

/// What happened to the button being reported.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MouseAction {
    Press,
    Release,
    /// Cursor moved into a new cell.
    Motion,
}

/// Translate an iced button into a reportable one.
///
/// Returns `None` for buttons the protocol has no encoding for.
pub fn report_button(button: iced::mouse::Button) -> Option<MouseButton> {
    use iced::mouse::Button;

    match button {
        Button::Left => Some(MouseButton::Left),
        Button::Middle => Some(MouseButton::Middle),
        Button::Right => Some(MouseButton::Right),
        _ => None,
    }
}

/// Encode a single mouse report.
///
/// `col` and `row` are 0-based coordinates in the *visible screen*, not the
/// scrollback-relative grid; the protocol's 1-based offset is applied here.
pub fn encode(
    mode: TermMode,
    action: MouseAction,
    button: MouseButton,
    col: usize,
    row: usize,
    modifiers: keyboard::Modifiers,
) -> Vec<u8> {
    let sgr = mode.contains(TermMode::SGR_MOUSE);

    // Legacy encodings have no room for the button identity on release, so
    // they report the generic "button 3" instead. SGR keeps the identity and
    // distinguishes release with a lowercase final byte.
    let mut code = if action == MouseAction::Release && !sgr {
        MouseButton::NoButton.code()
    } else {
        button.code()
    };

    if action == MouseAction::Motion {
        code += 32;
    }
    if modifiers.shift() {
        code += 4;
    }
    if modifiers.alt() {
        code += 8;
    }
    if modifiers.control() {
        code += 16;
    }

    if sgr {
        let final_byte = if action == MouseAction::Release {
            'm'
        } else {
            'M'
        };
        return format!(
            "\x1b[<{};{};{}{}",
            code,
            col + 1,
            row + 1,
            final_byte,
        )
        .into_bytes();
    }

    let mut out = b"\x1b[M".to_vec();
    if mode.contains(TermMode::UTF8_MOUSE) {
        push_utf8(&mut out, 32 + code as u32);
        push_utf8(&mut out, (32 + (col + 1).min(UTF8_MAX_COORD)) as u32);
        push_utf8(&mut out, (32 + (row + 1).min(UTF8_MAX_COORD)) as u32);
    } else {
        out.push(32 + code);
        out.push((32 + (col + 1).min(X10_MAX_COORD)) as u8);
        out.push((32 + (row + 1).min(X10_MAX_COORD)) as u8);
    }
    out
}

/// Arrow-key sequences for alternate scroll mode.
///
/// When the alternate screen is up and the application enabled mode 1007 but
/// not mouse reporting, the wheel is translated into cursor-key presses. This
/// is what makes `less` and friends scroll.
///
/// `lines` is positive for scrolling up (towards older content).
pub fn alternate_scroll(mode: TermMode, lines: i32) -> Option<Vec<u8>> {
    if lines == 0 {
        return None;
    }

    let seq: &[u8] = match (lines > 0, mode.contains(TermMode::APP_CURSOR)) {
        (true, true) => b"\x1bOA",
        (true, false) => b"\x1b[A",
        (false, true) => b"\x1bOB",
        (false, false) => b"\x1b[B",
    };

    let mut out = Vec::with_capacity(seq.len() * lines.unsigned_abs() as usize);
    for _ in 0..lines.unsigned_abs() {
        out.extend_from_slice(seq);
    }
    Some(out)
}

/// Push a value as UTF-8, matching xterm's extended coordinate encoding.
fn push_utf8(out: &mut Vec<u8>, value: u32) {
    match char::from_u32(value) {
        Some(c) => {
            let mut buf = [0u8; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        }
        None => out.push(value as u8),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: keyboard::Modifiers = keyboard::Modifiers::empty();

    fn sgr() -> TermMode {
        TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE
    }

    fn x10() -> TermMode {
        TermMode::MOUSE_REPORT_CLICK
    }

    #[test]
    fn sgr_press_is_one_based() {
        let bytes = encode(
            sgr(),
            MouseAction::Press,
            MouseButton::Left,
            0,
            0,
            NONE,
        );
        assert_eq!(bytes, b"\x1b[<0;1;1M");
    }

    #[test]
    fn sgr_release_uses_lowercase_final_byte() {
        let bytes = encode(
            sgr(),
            MouseAction::Release,
            MouseButton::Right,
            9,
            4,
            NONE,
        );
        assert_eq!(bytes, b"\x1b[<2;10;5m");
    }

    #[test]
    fn sgr_drag_adds_motion_bit() {
        let bytes = encode(
            sgr(),
            MouseAction::Motion,
            MouseButton::Left,
            3,
            7,
            NONE,
        );
        assert_eq!(bytes, b"\x1b[<32;4;8M");
    }

    #[test]
    fn sgr_motion_without_button() {
        let bytes = encode(
            sgr(),
            MouseAction::Motion,
            MouseButton::NoButton,
            0,
            0,
            NONE,
        );
        assert_eq!(bytes, b"\x1b[<35;1;1M");
    }

    #[test]
    fn sgr_wheel_buttons() {
        let up = encode(
            sgr(),
            MouseAction::Press,
            MouseButton::WheelUp,
            0,
            0,
            NONE,
        );
        assert_eq!(up, b"\x1b[<64;1;1M");

        let down = encode(
            sgr(),
            MouseAction::Press,
            MouseButton::WheelDown,
            0,
            0,
            NONE,
        );
        assert_eq!(down, b"\x1b[<65;1;1M");
    }

    #[test]
    fn modifier_bits() {
        let shift = encode(
            sgr(),
            MouseAction::Press,
            MouseButton::Left,
            0,
            0,
            keyboard::Modifiers::SHIFT,
        );
        assert_eq!(shift, b"\x1b[<4;1;1M");

        let alt = encode(
            sgr(),
            MouseAction::Press,
            MouseButton::Left,
            0,
            0,
            keyboard::Modifiers::ALT,
        );
        assert_eq!(alt, b"\x1b[<8;1;1M");

        let ctrl = encode(
            sgr(),
            MouseAction::Press,
            MouseButton::Left,
            0,
            0,
            keyboard::Modifiers::CTRL,
        );
        assert_eq!(ctrl, b"\x1b[<16;1;1M");

        let all = encode(
            sgr(),
            MouseAction::Press,
            MouseButton::Middle,
            0,
            0,
            keyboard::Modifiers::SHIFT
                | keyboard::Modifiers::ALT
                | keyboard::Modifiers::CTRL,
        );
        assert_eq!(all, b"\x1b[<29;1;1M");
    }

    #[test]
    fn x10_offsets_coordinates_by_32() {
        let bytes = encode(
            x10(),
            MouseAction::Press,
            MouseButton::Left,
            0,
            0,
            NONE,
        );
        assert_eq!(bytes, b"\x1b[M\x20\x21\x21");
    }

    #[test]
    fn x10_release_reports_button_three() {
        let bytes = encode(
            x10(),
            MouseAction::Release,
            MouseButton::Right,
            2,
            2,
            NONE,
        );
        assert_eq!(bytes, b"\x1b[M\x23\x23\x23");
    }

    #[test]
    fn x10_clamps_coordinates_at_223() {
        let bytes = encode(
            x10(),
            MouseAction::Press,
            MouseButton::Left,
            499,
            300,
            NONE,
        );
        assert_eq!(bytes, b"\x1b[M\x20\xff\xff");
    }

    #[test]
    fn x10_wheel_stays_in_one_byte() {
        let bytes = encode(
            x10(),
            MouseAction::Press,
            MouseButton::WheelDown,
            0,
            0,
            NONE,
        );
        assert_eq!(bytes, b"\x1b[M\x61\x21\x21");
    }

    #[test]
    fn utf8_mode_encodes_wide_coordinates() {
        let mode = TermMode::MOUSE_REPORT_CLICK | TermMode::UTF8_MOUSE;
        let bytes = encode(
            mode,
            MouseAction::Press,
            MouseButton::Left,
            299,
            0,
            NONE,
        );
        let mut expected = b"\x1b[M\x20".to_vec();
        expected.extend_from_slice(
            char::from_u32(332).unwrap().to_string().as_bytes(),
        );
        expected.push(0x21);
        assert_eq!(bytes, expected);
    }

    #[test]
    fn sgr_wins_over_utf8() {
        let mode = sgr() | TermMode::UTF8_MOUSE;
        let bytes = encode(
            mode,
            MouseAction::Press,
            MouseButton::Left,
            499,
            0,
            NONE,
        );
        assert_eq!(bytes, b"\x1b[<0;500;1M");
    }

    #[test]
    fn alternate_scroll_emits_arrow_per_line() {
        let mode = TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL;
        assert_eq!(
            alternate_scroll(mode, 3),
            Some(b"\x1b[A\x1b[A\x1b[A".to_vec()),
        );
        assert_eq!(
            alternate_scroll(mode, -2),
            Some(b"\x1b[B\x1b[B".to_vec()),
        );
        assert_eq!(alternate_scroll(mode, 0), None);
    }

    #[test]
    fn alternate_scroll_honors_app_cursor() {
        let mode = TermMode::ALT_SCREEN
            | TermMode::ALTERNATE_SCROLL
            | TermMode::APP_CURSOR;
        assert_eq!(alternate_scroll(mode, 1), Some(b"\x1bOA".to_vec()));
        assert_eq!(alternate_scroll(mode, -1), Some(b"\x1bOB".to_vec()));
    }
}

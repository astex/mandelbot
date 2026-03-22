pub const TAB: u8 = 0x09;
pub const SPACE: u8 = 0x20;
pub const DEL: u8 = 0x7f;

pub const CTRL_C: u8 = 0x03;
pub const ESCAPE: u8 = 0x1b;

pub const ARROW_UP: &[u8] = b"\x1b[A";
pub const ARROW_DOWN: &[u8] = b"\x1b[B";
pub const ARROW_RIGHT: &[u8] = b"\x1b[C";
pub const ARROW_LEFT: &[u8] = b"\x1b[D";

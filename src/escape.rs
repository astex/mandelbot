pub const BACKSPACE: u8 = 0x08;

pub const CURSOR_UP: char = 'A';
pub const CURSOR_DOWN: char = 'B';
pub const CURSOR_FORWARD: char = 'C';
pub const CURSOR_BACK: char = 'D';
pub const CURSOR_POSITION: char = 'H';

pub const ERASE_DISPLAY_CURSOR_TO_END: (char, u16) = ('J', 0);
pub const ERASE_DISPLAY_START_TO_CURSOR: (char, u16) = ('J', 1);
pub const ERASE_DISPLAY_ENTIRE: (char, u16) = ('J', 2);

pub const ERASE_LINE_CURSOR_TO_END: (char, u16) = ('K', 0);
pub const ERASE_LINE_START_TO_CURSOR: (char, u16) = ('K', 1);
pub const ERASE_LINE_ENTIRE: (char, u16) = ('K', 2);

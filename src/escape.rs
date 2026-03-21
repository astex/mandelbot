pub const ERASE_DISPLAY_CURSOR_TO_END: (char, u16) = ('J', 0);
pub const ERASE_DISPLAY_START_TO_CURSOR: (char, u16) = ('J', 1);
pub const ERASE_DISPLAY_ENTIRE: (char, u16) = ('J', 2);

pub const ERASE_LINE_CURSOR_TO_END: (char, u16) = ('K', 0);
pub const ERASE_LINE_START_TO_CURSOR: (char, u16) = ('K', 1);
pub const ERASE_LINE_ENTIRE: (char, u16) = ('K', 2);

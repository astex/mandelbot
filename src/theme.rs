use iced::Color;

pub struct TerminalTheme {
    pub fg: Color,
    pub bg: Color,
    pub black: Color,
    pub red: Color,
    pub green: Color,
    pub yellow: Color,
    pub blue: Color,
    pub magenta: Color,
    pub cyan: Color,
    pub white: Color,
    pub bright_black: Color,
    pub bright_red: Color,
    pub bright_green: Color,
    pub bright_yellow: Color,
    pub bright_blue: Color,
    pub bright_magenta: Color,
    pub bright_cyan: Color,
    pub bright_white: Color,
}

pub fn solarized_dark() -> TerminalTheme {
    TerminalTheme {
        bg: Color::from_rgb8(0x00, 0x2b, 0x36),
        fg: Color::from_rgb8(0x83, 0x94, 0x96),
        black: Color::from_rgb8(0x07, 0x36, 0x42),
        red: Color::from_rgb8(0xdc, 0x32, 0x2f),
        green: Color::from_rgb8(0x85, 0x99, 0x00),
        yellow: Color::from_rgb8(0xb5, 0x89, 0x00),
        blue: Color::from_rgb8(0x26, 0x8b, 0xd2),
        magenta: Color::from_rgb8(0xd3, 0x36, 0x82),
        cyan: Color::from_rgb8(0x2a, 0xa1, 0x98),
        white: Color::from_rgb8(0xee, 0xe8, 0xd5),
        bright_black: Color::from_rgb8(0x00, 0x2b, 0x36),
        bright_red: Color::from_rgb8(0xcb, 0x4b, 0x16),
        bright_green: Color::from_rgb8(0x58, 0x6e, 0x75),
        bright_yellow: Color::from_rgb8(0x65, 0x7b, 0x83),
        bright_blue: Color::from_rgb8(0x83, 0x94, 0x96),
        bright_magenta: Color::from_rgb8(0x6c, 0x71, 0xc4),
        bright_cyan: Color::from_rgb8(0x93, 0xa1, 0xa1),
        bright_white: Color::from_rgb8(0xfd, 0xf6, 0xe3),
    }
}

pub fn solarized_light() -> TerminalTheme {
    TerminalTheme {
        bg: Color::from_rgb8(0xfd, 0xf6, 0xe3),
        fg: Color::from_rgb8(0x65, 0x7b, 0x83),
        black: Color::from_rgb8(0x07, 0x36, 0x42),
        red: Color::from_rgb8(0xdc, 0x32, 0x2f),
        green: Color::from_rgb8(0x85, 0x99, 0x00),
        yellow: Color::from_rgb8(0xb5, 0x89, 0x00),
        blue: Color::from_rgb8(0x26, 0x8b, 0xd2),
        magenta: Color::from_rgb8(0xd3, 0x36, 0x82),
        cyan: Color::from_rgb8(0x2a, 0xa1, 0x98),
        white: Color::from_rgb8(0xee, 0xe8, 0xd5),
        bright_black: Color::from_rgb8(0x00, 0x2b, 0x36),
        bright_red: Color::from_rgb8(0xcb, 0x4b, 0x16),
        bright_green: Color::from_rgb8(0x58, 0x6e, 0x75),
        bright_yellow: Color::from_rgb8(0x65, 0x7b, 0x83),
        bright_blue: Color::from_rgb8(0x83, 0x94, 0x96),
        bright_magenta: Color::from_rgb8(0x6c, 0x71, 0xc4),
        bright_cyan: Color::from_rgb8(0x93, 0xa1, 0xa1),
        bright_white: Color::from_rgb8(0xfd, 0xf6, 0xe3),
    }
}

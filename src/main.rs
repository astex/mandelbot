mod escape;
mod keys;
mod pty;
mod terminal;
mod ui;

fn main() -> iced::Result {
    iced::application(ui::Terminal::boot, ui::Terminal::update, ui::Terminal::view)
        .title("Mandelbot")
        .subscription(ui::Terminal::subscription)
        .theme(ui::Terminal::theme)
        .run()
}

mod pty;
mod terminal;
mod ui;

fn main() -> iced::Result {
    iced::application(ui::Terminal::boot, ui::Terminal::update, ui::Terminal::view)
        .title("Squeak")
        .subscription(ui::Terminal::subscription)
        .theme(ui::Terminal::theme)
        .run()
}

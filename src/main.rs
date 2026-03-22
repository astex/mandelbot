mod config;
mod keys;
mod pty;
mod terminal;
mod theme;
mod ui;

fn main() -> iced::Result {
    let cfg = config::Config::load();
    let window_size = ui::initial_window_size(&cfg);

    iced::application(ui::Terminal::boot, ui::Terminal::update, ui::Terminal::view)
        .title("Mandelbot")
        .subscription(ui::Terminal::subscription)
        .theme(ui::Terminal::theme)
        .window_size(window_size)
        .run()
}

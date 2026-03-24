mod config;
mod keys;
mod pty;
mod terminal;
mod theme;
mod ui;
mod widget;

fn main() -> iced::Result {
    let cfg = config::Config::load();
    let window_size = ui::initial_window_size(&cfg);

    iced::application(ui::App::boot, ui::App::update, ui::App::view)
        .title("Mandelbot")
        .subscription(ui::App::subscription)
        .theme(ui::App::theme)
        .window_size(window_size)
        .run()
}

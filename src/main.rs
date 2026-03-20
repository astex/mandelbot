mod pty;
mod terminal;
mod ui;

fn main() -> iced::Result {
    let buffer = terminal::new_shared(24, 80);

    let mut pty_handle =
        pty::PtyHandle::spawn("/bin/bash", 24, 80).expect("failed to spawn PTY");

    let reader = pty_handle.take_reader();
    let writer = pty_handle.take_writer();

    ui::init(buffer, writer, reader);

    iced::application(ui::Terminal::boot, ui::Terminal::update, ui::Terminal::view)
        .title("Squeak")
        .subscription(ui::Terminal::subscription)
        .theme(ui::Terminal::theme)
        .run()
}

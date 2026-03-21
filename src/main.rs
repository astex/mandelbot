mod pty;
mod terminal;
mod ui;

fn main() -> iced::Result {
    iced::application(
        || {
            let mut pty_handle =
                pty::PtyHandle::spawn("/bin/bash", 24, 80).expect("failed to spawn PTY");

            let reader = pty_handle.take_reader();
            let writer = pty_handle.take_writer();

            ui::Terminal::new(writer, reader)
        },
        ui::Terminal::update,
        ui::Terminal::view,
    )
    .title("Squeak")
    .subscription(ui::Terminal::subscription)
    .theme(ui::Terminal::theme)
    .run()
}

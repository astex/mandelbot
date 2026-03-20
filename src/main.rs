mod pty;
mod terminal;
mod ui;

use std::sync::Mutex;

fn main() -> iced::Result {
    let buffer = terminal::new_shared(24, 80);

    let mut pty_handle = pty::PtyHandle::spawn("/bin/bash", 24, 80).expect("failed to spawn PTY");

    let reader = pty_handle.take_reader();
    let writer = pty_handle.take_writer();

    // Wrap in Mutex<Option<...>> so the Fn closure can take ownership once.
    let init = Mutex::new(Some((buffer, writer, reader)));

    iced::application(
        move || {
            let (buffer, writer, reader) = init
                .lock()
                .unwrap()
                .take()
                .expect("boot called more than once");
            ui::Terminal::new(buffer, writer, reader)
        },
        ui::Terminal::update,
        ui::Terminal::view,
    )
    .title("Squeak")
    .subscription(ui::Terminal::subscription)
    .theme(ui::Terminal::theme)
    .run()
}

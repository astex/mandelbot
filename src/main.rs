mod agent;
mod config;
mod pty;
mod terminal;
mod ui;

use std::io::Read;
use std::thread;

use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QUrl};

fn main() {
    let cfg = config::Config::load();
    let buffer = terminal::new_shared(24, 80);

    let mut pty_handle =
        pty::PtyHandle::spawn(&cfg.shell, 24, 80).expect("failed to spawn PTY");

    let mut reader = pty_handle.take_reader();
    let writer = pty_handle.take_writer();

    // Background thread: PTY stdout -> terminal buffer
    let read_buffer = buffer.clone();
    thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let mut tb = read_buffer.lock().unwrap();
                    tb.feed(&buf[..n]);
                }
            }
        }
    });

    ui::init_globals(buffer, writer);

    let mut app = QGuiApplication::new();
    let mut engine = QQmlApplicationEngine::new();

    if let Some(engine) = engine.as_mut() {
        engine.load(&QUrl::from("qrc:/qt/qml/com/squeak/terminal/qml/main.qml"));
    }

    if let Some(app) = app.as_mut() {
        let _exit_code = app.exec();
    }
}

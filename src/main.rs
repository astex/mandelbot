mod agent;
mod config;
mod pty;
mod terminal;
mod ui;

use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QUrl};

fn main() {
    let cfg = config::Config::new();
    let buffer = terminal::new_shared(24, 80);

    let mut pty_handle =
        pty::PtyHandle::spawn(&cfg.shell, 24, 80).expect("failed to spawn PTY");

    let reader = pty_handle.take_reader();
    let writer = pty_handle.take_writer();

    ui::init_globals(buffer, writer, reader);

    let mut app = QGuiApplication::new();
    let mut engine = QQmlApplicationEngine::new();

    if let Some(engine) = engine.as_mut() {
        engine.load(&QUrl::from("qrc:/qt/qml/com/squeak/terminal/qml/main.qml"));
    }

    if let Some(app) = app.as_mut() {
        let _exit_code = app.exec();
    }
}

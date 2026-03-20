use std::io::{Read, Write};
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;

use cxx_qt::Threading;

use cxx_qt_lib::QString;

use crate::terminal::SharedBuffer;

static WRITER: OnceLock<Arc<Mutex<Box<dyn Write + Send>>>> = OnceLock::new();
static READER: OnceLock<Mutex<Option<Box<dyn Read + Send>>>> = OnceLock::new();
static BUFFER: OnceLock<SharedBuffer> = OnceLock::new();

pub fn init_globals(
    buffer: SharedBuffer,
    writer: Box<dyn Write + Send>,
    reader: Box<dyn Read + Send>,
) {
    BUFFER.set(buffer).ok();
    WRITER.set(Arc::new(Mutex::new(writer))).ok();
    READER.set(Mutex::new(Some(reader))).ok();
}

#[cxx_qt::bridge]
mod ffi {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(QString, screen_text)]
        type TerminalView = super::TerminalViewRust;

        #[qinvokable]
        fn key_pressed(self: Pin<&mut TerminalView>, text: &QString, key: i32);

        #[qinvokable]
        fn start_reader(self: Pin<&mut TerminalView>);
    }

    impl cxx_qt::Threading for TerminalView {}
}

pub struct TerminalViewRust {
    screen_text: QString,
}

impl Default for TerminalViewRust {
    fn default() -> Self {
        Self {
            screen_text: QString::from(""),
        }
    }
}

impl ffi::TerminalView {
    fn key_pressed(self: Pin<&mut Self>, text: &QString, key: i32) {
        let s = text.to_string();
        let bytes = match key {
            0x01000004 | 0x01000005 => b"\r".to_vec(), // Return/Enter
            0x01000003 => vec![0x7f],                   // Backspace
            _ if !s.is_empty() => s.into_bytes(),
            _ => return,
        };

        if let Some(writer) = WRITER.get() {
            if let Ok(mut w) = writer.lock() {
                let _ = w.write_all(&bytes);
                let _ = w.flush();
            }
        }
    }

    fn start_reader(self: Pin<&mut Self>) {
        let qt_thread = self.qt_thread();

        let mut reader = READER
            .get()
            .and_then(|r| r.lock().ok()?.take())
            .expect("reader already taken or not initialized");

        let buffer = BUFFER.get().expect("buffer not initialized").clone();

        thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let text = {
                            let mut tb = buffer.lock().unwrap();
                            tb.feed(&buf[..n]);
                            QString::from(&tb.screen_text() as &str)
                        };
                        qt_thread
                            .queue(move |mut qobject: Pin<&mut ffi::TerminalView>| {
                                qobject.as_mut().set_screen_text(text);
                            })
                            .ok();
                    }
                }
            }
        });
    }
}

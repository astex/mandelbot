use cxx_qt_lib::QString;
use std::io::Write;
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock};

use crate::terminal::SharedBuffer;

static BUFFER: OnceLock<SharedBuffer> = OnceLock::new();
static WRITER: OnceLock<Arc<Mutex<Box<dyn Write + Send>>>> = OnceLock::new();

pub fn init_globals(buffer: SharedBuffer, writer: Box<dyn Write + Send>) {
    BUFFER.set(buffer).ok();
    WRITER.set(Arc::new(Mutex::new(writer))).ok();
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
        fn refresh(self: Pin<&mut TerminalView>);
    }
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

    fn refresh(mut self: Pin<&mut Self>) {
        let text = match BUFFER.get() {
            Some(buf) => {
                let b = buf.lock().unwrap();
                QString::from(&b.screen_text() as &str)
            }
            None => QString::from(""),
        };
        self.as_mut().set_screen_text(text);
    }
}

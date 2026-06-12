//! Spike: validate rfd::AsyncFileDialog under Iced 0.14's winit backend on
//! macOS, per docs/superpowers/specs/2026-06-05-native-folder-dialog-design.md.
//!
//! Pattern under test: build the dialog future synchronously in `update()`
//! (main thread), await it via `Task::perform` (background executor).
//!
//! Run with: cargo run --example rfd_spike
//! Validate: click [ Browse… ] and pick a folder (path shown + printed),
//! then click again and press Cancel (shows "cancelled"). The window must
//! stay responsive while the dialog is open.

use std::path::PathBuf;

use iced::Task;
use iced::widget::{button, column, text};

#[derive(Debug, Clone)]
enum Message {
    OpenDialog,
    DialogResult(Option<PathBuf>),
}

#[derive(Default)]
struct Spike {
    dialog_open: bool,
    result: String,
}

impl Spike {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::OpenDialog => {
                if self.dialog_open {
                    return Task::none();
                }
                self.dialog_open = true;
                eprintln!(
                    "spawning dialog on thread {:?}",
                    std::thread::current().name()
                );
                let home = std::env::var("HOME").unwrap_or_default();
                let dialog = rfd::AsyncFileDialog::new()
                    .set_title("Open Project")
                    .set_directory(home)
                    .pick_folder();
                Task::perform(dialog, |handle| {
                    Message::DialogResult(handle.map(|h| h.path().to_path_buf()))
                })
            }
            Message::DialogResult(path) => {
                self.dialog_open = false;
                self.result = match path {
                    Some(p) => format!("picked: {}", p.display()),
                    None => "cancelled".to_string(),
                };
                eprintln!("{}", self.result);
                Task::none()
            }
        }
    }

    fn view(&self) -> iced::Element<'_, Message> {
        column![
            button("[ Browse… ]").on_press(Message::OpenDialog),
            text(&self.result),
        ]
        .spacing(10)
        .padding(20)
        .into()
    }
}

fn main() -> iced::Result {
    iced::application(Spike::default, Spike::update, Spike::view)
        .title("rfd spike")
        .run()
}

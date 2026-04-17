use std::time::Duration;

use iced::Task;

use crate::ui::Message;

pub const TOAST_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub struct Toast {
    pub id: usize,
    pub source_tab_id: usize,
    pub message: String,
    pub prompt: Option<String>,
    pub target_tab_id: Option<usize>,
}

pub fn schedule_dismiss(toast_id: usize) -> Task<Message> {
    Task::perform(
        async move {
            let (tx, rx) = futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                std::thread::sleep(TOAST_TIMEOUT);
                let _ = tx.send(());
            });
            let _ = rx.await;
            toast_id
        },
        Message::DismissToast,
    )
}

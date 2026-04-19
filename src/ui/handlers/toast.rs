use iced::Task;

use crate::tab::AgentRank;
use crate::toast::{self, Toast};

use super::super::{App, Message};

impl App {
    pub(in crate::ui) fn handle_show_toast(
        &mut self,
        source_tab_id: usize,
        message: String,
        prompt: Option<String>,
        target_tab_id: Option<usize>,
    ) -> Task<Message> {
        let id = self.next_toast_id;
        self.next_toast_id += 1;
        self.toasts.push(Toast {
            id,
            source_tab_id,
            message,
            prompt,
            target_tab_id,
        });
        toast::schedule_dismiss(id)
    }

    pub(in crate::ui) fn handle_focus_from_toast(&mut self, toast_id: usize) -> Task<Message> {
        let Some(idx) = self.toasts.iter().position(|t| t.id == toast_id) else {
            return Task::none();
        };
        let toast = self.toasts.remove(idx);
        let Some(target) = toast.target_tab_id else {
            return Task::none();
        };
        if self.tabs.iter().any(|t| t.id == target) {
            self.focus_tab(target);
        }
        Task::none()
    }

    pub(in crate::ui) fn handle_dismiss_toast(&mut self, toast_id: usize) -> Task<Message> {
        self.toasts.retain(|t| t.id != toast_id);
        Task::none()
    }

    pub(in crate::ui) fn handle_spawn_from_toast(&mut self, toast_id: usize) -> Task<Message> {
        let Some(idx) = self.toasts.iter().position(|t| t.id == toast_id) else {
            return Task::none();
        };
        let toast = self.toasts.remove(idx);
        let Some(prompt) = toast.prompt else {
            return Task::none();
        };
        let Some(source) = self.tabs.iter().find(|t| t.id == toast.source_tab_id) else {
            return Task::none();
        };
        let (rank, project_dir, parent_id) = match source.rank {
            AgentRank::Home => return Task::none(),
            AgentRank::Project => (AgentRank::Task, source.project_dir.clone(), Some(source.id)),
            AgentRank::Task => {
                let dir = self.project_dir_for_tab(source.id);
                (AgentRank::Task, dir, Some(source.id))
            }
        };
        let (new_tab_id, task) = self.spawn_tab(
            true, rank, project_dir, parent_id, Some(prompt),
            None, None, None,
        );
        self.focus_tab(new_tab_id);
        task
    }
}

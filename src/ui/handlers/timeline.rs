use iced::Task;

use super::super::{
    terminal_size_with_reserved, App, CheckpointReason, Message, TimelineDir, TimelineMode,
};

impl App {
    pub(in crate::ui) fn handle_toggle_timeline(&mut self, tab_id: usize) -> Task<Message> {
        let mut opened = false;
        if let Some(mut tab) = self.tabs.snapshot(tab_id) {
            tab.timeline_visible = !tab.timeline_visible;
            if !tab.timeline_visible {
                tab.timeline_cursor = None;
            } else {
                opened = true;
            }
            self.tabs.write(tab);
        }
        if !opened {
            self.resize_tab_for_timeline(tab_id);
            return Task::none();
        }
        let need_ckpt = self
            .tabs
            .iter()
            .find(|t| t.id == tab_id)
            .and_then(|tab| {
                self.ckpt_store.head_of(&tab.uuid).map(|tip| {
                    crate::widget::timeline::has_uncheckpointed_tail(
                        &self.ckpt_store,
                        tab,
                        tip,
                    )
                })
            })
            .unwrap_or(false);
        let ckpt_task = if need_ckpt {
            self.kick_checkpoint(tab_id, CheckpointReason::TimelineOpen)
        } else {
            Task::none()
        };
        self.resize_tab_for_timeline(tab_id);
        let scroll_task = if need_ckpt {
            Task::none()
        } else if let Some(tab) = self.tabs.get(tab_id) {
            crate::widget::timeline::scroll_to_cursor(
                &self.ckpt_store,
                tab,
                &self.config,
            )
        } else {
            Task::none()
        };
        Task::batch([ckpt_task, scroll_task])
    }

    pub(in crate::ui) fn handle_timeline_scrub(
        &mut self,
        tab_id: usize,
        dir: TimelineDir,
    ) -> Task<Message> {
        let next = self
            .tabs
            .iter()
            .find(|t| t.id == tab_id)
            .and_then(|tab| crate::widget::timeline::move_cursor(&self.ckpt_store, tab, dir));
        if let Some(id) = next {
            if let Some(mut tab) = self.tabs.snapshot(tab_id) {
                tab.timeline_cursor = Some(id);
                tab.redo_path.clear();
                self.tabs.write(tab);
            }
            if let Some(tab) = self.tabs.get(tab_id) {
                return crate::widget::timeline::scroll_to_cursor(
                    &self.ckpt_store,
                    tab,
                    &self.config,
                );
            }
        }
        Task::none()
    }

    pub(in crate::ui) fn handle_timeline_activate(
        &mut self,
        tab_id: usize,
        mode: TimelineMode,
    ) -> Task<Message> {
        let Some(tab) = self.tabs.get(tab_id) else {
            return Task::none();
        };
        let tip = self.ckpt_store.head_of(&tab.uuid).cloned();
        let Some(ckpt_id) =
            crate::widget::timeline::effective_cursor(tab, tip.as_deref())
        else {
            return Task::none();
        };
        match mode {
            TimelineMode::Replace => self.handle_replace(tab_id, ckpt_id),
            TimelineMode::Fork => self.handle_fork(tab_id, ckpt_id, None),
        }
    }

    /// Resize a tab's PTY/term to account for its current timeline
    /// visibility. Called when the timeline is toggled so the agent's
    /// bottom rows don't end up hidden behind the strip.
    pub(super) fn resize_tab_for_timeline(&mut self, tab_id: usize) {
        let Some(size) = self.window_size else {
            return;
        };
        let cw = self.config.char_width();
        let ch = self.config.char_height();
        let reserved = {
            let Some(tab) = self.tabs.get(tab_id) else {
                return;
            };
            crate::widget::timeline::pixel_height(&self.ckpt_store, tab, &self.config)
        };
        let (rows, cols) = terminal_size_with_reserved(size, cw, ch, reserved);
        let Some(tab) = self.tabs.get(tab_id) else {
            return;
        };
        tab.resize(rows, cols, size.width as u16, size.height as u16);
        tab.set_window_size(alacritty_terminal::event::WindowSize {
            num_lines: rows as u16,
            num_cols: cols as u16,
            cell_width: cw as u16,
            cell_height: ch as u16,
        });
    }
}

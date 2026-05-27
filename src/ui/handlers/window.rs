use std::path::PathBuf;

use iced::{Size, Task};

use crate::tab::AgentRank;

use super::super::{
    terminal_size_with_reserved, App, Message, MAX_TAB_BAR_FRACTION, MIN_TAB_BAR_WIDTH,
};

impl App {
    pub(in crate::ui) fn handle_window_resized(&mut self, size: Size) -> Task<Message> {
        if self.window_size.is_none() {
            self.window_size = Some(size);
            let home = std::env::var("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("."));
            let mandelbot_dir = home.join(".mandelbot");
            let _ = std::fs::create_dir_all(&mandelbot_dir);
            let initialized = mandelbot_dir.join(".initialized");
            let first_run_prompt = if !initialized.exists() {
                let _ = std::fs::write(&initialized, "");
                Some("/mandelbot-features".to_string())
            } else {
                None
            };
            let (id, task) = self.spawn_tab(true, AgentRank::Home, Some(home), None, first_run_prompt, None, None, None);
            self.focus_tab(id);
            if let Some(mut tab) = self.tabs.snapshot(self.tabs.active_id()) {
                tab.title = Some("home".into());
                self.tabs.write(tab);
            }
            return task;
        }

        self.window_size = Some(size);
        // Clamp the bar to the new window width — a shrink can leave
        // the bar wider than half the window, which would push the
        // terminal off-screen.
        let new_max = (size.width * MAX_TAB_BAR_FRACTION).max(MIN_TAB_BAR_WIDTH);
        if self.tab_bar_width() > new_max {
            self.set_tab_bar_width(new_max);
        }
        self.resize_all_tabs(size);
        Task::none()
    }

    /// Push the current `(window_size, tab_bar_width)` down to every
    /// tab's terminal grid + PTY.  Shared by `handle_window_resized`
    /// and the drag handler.
    pub(in crate::ui) fn resize_all_tabs(&mut self, size: Size) {
        let cw = self.config.char_width();
        let ch = self.config.char_height();
        let bar_w = self.tab_bar_width();
        let store = &self.ckpt_store;
        let cfg = &self.config;
        for tab in self.tabs.iter() {
            let reserved = crate::widget::timeline::pixel_height(store, tab, cfg);
            let (rows, cols) = terminal_size_with_reserved(size, cw, ch, bar_w, reserved);
            tab.resize(rows, cols, size.width as u16, size.height as u16);
            tab.set_window_size(alacritty_terminal::event::WindowSize {
                num_lines: rows as u16,
                num_cols: cols as u16,
                cell_width: cw as u16,
                cell_height: ch as u16,
            });
        }
    }

    pub(in crate::ui) fn handle_tab_bar_drag_start(&mut self) -> Task<Message> {
        self.set_tab_bar_dragging(true);
        Task::none()
    }

    pub(in crate::ui) fn handle_tab_bar_drag_move(&mut self, cursor_x: f32) -> Task<Message> {
        if !self.tab_bar_dragging() {
            return Task::none();
        }
        let Some(window) = self.window_size else {
            return Task::none();
        };
        let max = (window.width * MAX_TAB_BAR_FRACTION).max(MIN_TAB_BAR_WIDTH);
        let clamped = cursor_x.clamp(MIN_TAB_BAR_WIDTH, max);
        // No-op if the rounded width didn't actually change — avoids a
        // PTY resize storm on every sub-pixel cursor motion.
        if (clamped - self.tab_bar_width()).abs() < 0.5 {
            return Task::none();
        }
        self.set_tab_bar_width(clamped);
        self.resize_all_tabs(window);
        Task::none()
    }

    pub(in crate::ui) fn handle_tab_bar_drag_end(&mut self) -> Task<Message> {
        self.set_tab_bar_dragging(false);
        Task::none()
    }
}

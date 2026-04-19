use std::path::PathBuf;

use iced::{Size, Task};

use crate::tab::AgentRank;

use super::super::{terminal_size_with_reserved, App, Message};

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
            if let Some(mut tab) = self.tabs.snapshot(self.active_tab_id) {
                tab.title = Some("home".into());
                self.tabs.write(tab);
            }
            return task;
        }

        self.window_size = Some(size);
        let cw = self.config.char_width();
        let ch = self.config.char_height();
        let store = &self.ckpt_store;
        let cfg = &self.config;
        for tab in self.tabs.iter() {
            let reserved = crate::widget::timeline::pixel_height(store, tab, cfg);
            let (rows, cols) = terminal_size_with_reserved(size, cw, ch, reserved);
            tab.resize(rows, cols, size.width as u16, size.height as u16);
            tab.set_window_size(alacritty_terminal::event::WindowSize {
                num_lines: rows as u16,
                num_cols: cols as u16,
                cell_width: cw as u16,
                cell_height: ch as u16,
            });
        }
        Task::none()
    }
}

use std::path::PathBuf;

use iced::Task;

use alacritty_terminal::index::{Point as GridPoint, Side};
use alacritty_terminal::selection::Selection;

use crate::tab::{AgentRank, AgentStatus, TerminalTab};

use super::super::{terminal_size, App, Message, PendingKey};

impl App {
    pub(in crate::ui) fn active_tab(&self) -> Option<&TerminalTab> {
        self.tabs.get(self.active_tab_id)
    }

    pub(in crate::ui) fn handle_tab_output(
        &mut self,
        tab_id: usize,
        bg_tasks: usize,
        pr_number: Option<u32>,
    ) -> Task<Message> {
        let mut tasks: Vec<Task<Message>> = Vec::new();
        if let Some(tab) = self.tabs.get_mut(tab_id) {
            tab.background_tasks = bg_tasks;
            tab.pr_scraped = pr_number;
            if !tab.is_claude {
                if let Some(title) = tab.take_osc_title() {
                    tab.title = Some(title);
                }
            }
            if tab.take_bell() {
                return self.bell_flashes.trigger(tab_id);
            }

            for store in tab.take_clipboard_stores() {
                let task = match store.clipboard_type {
                    alacritty_terminal::term::ClipboardType::Clipboard => {
                        iced::clipboard::write(store.text)
                    }
                    alacritty_terminal::term::ClipboardType::Selection => {
                        iced::clipboard::write_primary(store.text)
                    }
                };
                tasks.push(task);
            }

            for load in tab.take_clipboard_loads() {
                let task = match load.clipboard_type {
                    alacritty_terminal::term::ClipboardType::Clipboard => {
                        iced::clipboard::read()
                    }
                    alacritty_terminal::term::ClipboardType::Selection => {
                        iced::clipboard::read_primary()
                    }
                };
                let task = task.map(move |content| {
                    let response = content.map(|text| (load.formatter)(&text));
                    Message::ClipboardLoadResult(tab_id, response)
                });
                tasks.push(task);
            }
        }
        Task::batch(tasks)
    }

    pub(in crate::ui) fn handle_shell_exited(
        &mut self,
        tab_id: usize,
        exit_code: Option<u32>,
    ) -> Task<Message> {
        match exit_code {
            Some(0) | None => self.close_tab(tab_id),
            Some(_code) => {
                if let Some(tab) = self.tabs.get_mut(tab_id) {
                    tab.status = AgentStatus::Error;
                }
                Task::none()
            }
        }
    }

    pub(in crate::ui) fn handle_set_title(&mut self, tab_id: usize, title: String) -> Task<Message> {
        if let Some(tab) = self.tabs.get_mut(tab_id) {
            tab.title = Some(title);
        }
        Task::none()
    }

    pub(in crate::ui) fn handle_mcp_spawn_agent(
        &mut self,
        requesting_tab_id: usize,
        working_directory: Option<PathBuf>,
        project_tab_id: Option<usize>,
        prompt: Option<String>,
        branch: Option<String>,
        model_override: Option<String>,
        base: Option<String>,
    ) -> Task<Message> {
        let requester = self.tabs.get(requesting_tab_id);
        let Some(requester) = requester else {
            self.respond_to_tab(requesting_tab_id, serde_json::json!({"error": "unknown tab"}));
            return Task::none();
        };

        let (rank, project_dir, parent_id) = match requester.rank {
            AgentRank::Home => {
                if let Some(ptid) = project_tab_id {
                    let project = self.tabs.get(ptid);
                    let Some(project) = project else {
                        self.respond_to_tab(requesting_tab_id, serde_json::json!({"error": "unknown project tab"}));
                        return Task::none();
                    };
                    if project.rank != AgentRank::Project {
                        self.respond_to_tab(requesting_tab_id, serde_json::json!({"error": "target tab is not a project agent"}));
                        return Task::none();
                    }
                    let dir = project.project_dir.clone();
                    (AgentRank::Task, dir, Some(ptid))
                } else {
                    let Some(wd) = working_directory else {
                        self.respond_to_tab(requesting_tab_id, serde_json::json!({"error": "working_directory or project_tab_id required from home agent"}));
                        return Task::none();
                    };
                    let canonical = std::fs::canonicalize(&wd).unwrap_or(wd);
                    if let Some(existing) = self.find_project_for_dir(&canonical) {
                        self.respond_to_tab(requesting_tab_id, serde_json::json!({"tab_id": existing}));
                        self.focus_tab(existing);
                        return Task::none();
                    }
                    (AgentRank::Project, Some(canonical), Some(requesting_tab_id))
                }
            }
            AgentRank::Project => {
                let dir = requester.project_dir.clone();
                (AgentRank::Task, dir, Some(requesting_tab_id))
            }
            AgentRank::Task => {
                let dir = self.project_dir_for_tab(requesting_tab_id);
                (AgentRank::Task, dir, Some(requesting_tab_id))
            }
        };

        let (new_tab_id, task) = self.spawn_tab(true, rank, project_dir, parent_id, prompt, branch, model_override, base);
        self.respond_to_tab(requesting_tab_id, serde_json::json!({"tab_id": new_tab_id}));
        task
    }

    pub(in crate::ui) fn handle_set_status(&mut self, tab_id: usize, status: AgentStatus) -> Task<Message> {
        if let Some(tab) = self.tabs.get_mut(tab_id) {
            tab.status = status;
        }
        Task::none()
    }

    pub(in crate::ui) fn handle_set_pr(&mut self, tab_id: usize, pr: Option<u32>) -> Task<Message> {
        if let Some(tab) = self.tabs.get_mut(tab_id) {
            tab.pr_override = pr;
        }
        Task::none()
    }

    pub(in crate::ui) fn handle_wakeup_at(&mut self, tab_id: usize, epoch_ms: u64) -> Task<Message> {
        if let Some(tab) = self.tabs.get_mut(tab_id) {
            tab.next_wakeup_at_ms = Some(epoch_ms);
        }
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let delay_ms = epoch_ms.saturating_sub(now_ms);
        Task::perform(
            async move {
                let (tx, rx) =
                    futures::channel::oneshot::channel();
                std::thread::spawn(move || {
                    std::thread::sleep(
                        std::time::Duration::from_millis(
                            delay_ms,
                        ),
                    );
                    let _ = tx.send(());
                });
                let _ = rx.await;
            },
            move |_| {
                Message::WakeupExpired(tab_id, epoch_ms)
            },
        )
    }

    pub(in crate::ui) fn handle_wakeup_expired(&mut self, tab_id: usize, epoch_ms: u64) -> Task<Message> {
        if let Some(tab) = self.tabs.get_mut(tab_id) {
            if tab.next_wakeup_at_ms == Some(epoch_ms) {
                tab.next_wakeup_at_ms = None;
            }
        }
        Task::none()
    }

    pub(in crate::ui) fn handle_pty_input(&mut self, bytes: Vec<u8>) -> Task<Message> {
        if let Some(tab) = self.active_tab_mut() {
            tab.write_input(&bytes);
            if tab.is_claude
                && tab.status == AgentStatus::NeedsReview
                && bytes == b"\r"
            {
                tab.status = AgentStatus::Working;
            }
        }
        Task::none()
    }

    pub(in crate::ui) fn handle_scroll(&mut self, delta: i32) -> Task<Message> {
        if let Some(tab) = self.active_tab_mut() {
            tab.scroll(delta);
        }
        Task::none()
    }

    pub(in crate::ui) fn handle_scroll_to(&mut self, offset: usize) -> Task<Message> {
        if let Some(tab) = self.active_tab_mut() {
            tab.scroll_to(offset);
        }
        Task::none()
    }

    pub(in crate::ui) fn handle_new_tab(&mut self) -> Task<Message> {
        let (id, task) = self.spawn_tab(false, AgentRank::Home, None, None, None, None, None, None);
        self.focus_tab(id);
        task
    }

    pub(in crate::ui) fn handle_spawn_agent(&mut self) -> Task<Message> {
        match self.active_rank() {
            Some(AgentRank::Home) => self.spawn_pending_project_tab(),
            Some(AgentRank::Project | AgentRank::Task) => {
                let parent_id = self.active_tab()
                    .and_then(|t| if t.rank == AgentRank::Task { t.parent_id } else { Some(t.id) });
                let project_dir = self.project_dir_for_tab(self.active_tab_id);
                if let (Some(pid), Some(dir)) = (parent_id, project_dir) {
                    let (id, task) = self.spawn_tab(true, AgentRank::Task, Some(dir), Some(pid), None, None, None, None);
                    self.focus_tab(id);
                    task
                } else {
                    Task::none()
                }
            }
            None => Task::none(),
        }
    }

    pub(in crate::ui) fn handle_spawn_child(&mut self) -> Task<Message> {
        match self.active_rank() {
            Some(AgentRank::Home) => self.spawn_pending_project_tab(),
            Some(AgentRank::Project | AgentRank::Task) => {
                if let Some(dir) = self.project_dir_for_tab(self.active_tab_id) {
                    let (id, task) = self.spawn_tab(true, AgentRank::Task, Some(dir), Some(self.active_tab_id), None, None, None, None);
                    self.focus_tab(id);
                    task
                } else {
                    Task::none()
                }
            }
            None => Task::none(),
        }
    }

    fn spawn_pending_project_tab(&mut self) -> Task<Message> {
        let Some(size) = self.window_size else {
            return Task::none();
        };
        let (rows, cols) = terminal_size(size, self.config.char_width(), self.config.char_height());
        let home_id = self.active_tab_id;
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        let tab = TerminalTab::new_pending(id, rows, cols, home_id);
        self.tabs.push(tab);
        self.focus_tab(id);
        Task::none()
    }

    pub(in crate::ui) fn handle_navigate_sibling(&mut self, delta: i32) -> Task<Message> {
        let order = self.tabs.display_order();
        if let Some(idx) = order.iter().position(|&id| id == self.active_tab_id) {
            let new_idx = (idx as i32 + delta)
                .rem_euclid(order.len() as i32) as usize;
            let target = order[new_idx];
            self.focus_tab(target);
        }
        Task::none()
    }

    pub(in crate::ui) fn handle_navigate_rank(&mut self, delta: i32) -> Task<Message> {
        if delta > 0 {
            if let Some(child) = self.first_child(self.active_tab_id) {
                self.focus_tab(child);
            }
        } else {
            if let Some(tab) = self.active_tab() {
                if let Some(pid) = tab.parent_id {
                    self.focus_tab(pid);
                }
            }
        }
        Task::none()
    }

    pub(in crate::ui) fn handle_focus_previous_tab(&mut self) -> Task<Message> {
        if let Some(prev) = self.prev_active_tab_id {
            if self.tabs.contains(prev) {
                self.focus_tab(prev);
            }
        }
        Task::none()
    }

    pub(in crate::ui) fn handle_next_idle(&mut self) -> Task<Message> {
        let order = self.tabs.display_order();
        let cur = order.iter().position(|&id| id == self.active_tab_id).unwrap_or(0);

        let candidates: Vec<usize> = order.iter()
            .copied()
            .cycle()
            .skip(cur + 1)
            .take(order.len())
            .collect();

        let status_of = |id: usize| -> Option<(AgentStatus, AgentRank)> {
            self.tabs.get(id).map(|t| (t.status, t.rank))
        };

        let target = candidates.iter().find(|&&id| matches!(status_of(id), Some((AgentStatus::Blocked, _))))
            .or_else(|| candidates.iter().find(|&&id| matches!(status_of(id), Some((AgentStatus::NeedsReview, _)))))
            .or_else(|| candidates.iter().find(|&&id| matches!(status_of(id), Some((AgentStatus::Idle, AgentRank::Task)))))
            .or_else(|| candidates.iter().find(|&&id| matches!(status_of(id), Some((AgentStatus::Idle, AgentRank::Project)))));

        if let Some(&id) = target {
            self.focus_tab(id);
        }
        Task::none()
    }

    pub(in crate::ui) fn handle_pending_input(&mut self, key: PendingKey) -> Task<Message> {
        let tab_id = self.active_tab_id;
        let tab = self.tabs.get_mut(tab_id);
        let Some(tab) = tab else { return Task::none() };
        let Some(input) = &mut tab.pending_input else { return Task::none() };

        match key {
            PendingKey::Char(c) => {
                input.push(c);
                Task::none()
            }
            PendingKey::Backspace => {
                input.pop();
                Task::none()
            }
            PendingKey::Cancel => {
                self.close_tab(tab_id)
            }
            PendingKey::Submit => {
                let raw_path = input.clone();
                let expanded = if raw_path.starts_with('~') {
                    let home = std::env::var("HOME").unwrap_or_default();
                    raw_path.replacen('~', &home, 1)
                } else {
                    raw_path
                };
                let path = PathBuf::from(&expanded);
                let canonical = std::fs::canonicalize(&path)
                    .unwrap_or(path);

                if let Some(existing) = self.find_project_for_dir(&canonical) {
                    self.focus_tab(existing);
                    return self.close_tab(tab_id);
                }

                let parent_id = match self.tabs.get(tab_id) {
                    Some(t) => t.parent_id,
                    None => return Task::none(),
                };
                self.tabs.remove(tab_id);

                let (id, task) = self.spawn_tab(
                    true,
                    AgentRank::Project,
                    Some(canonical),
                    parent_id,
                    None,
                    None,
                    None,
                    None,
                );
                self.focus_tab(id);
                task
            }
        }
    }

    pub(in crate::ui) fn handle_mcp_close_tab(
        &mut self,
        requesting_tab_id: usize,
        target_tab_id: usize,
    ) -> Task<Message> {
        let authorized = if requesting_tab_id == target_tab_id {
            true
        } else {
            let mut current = Some(target_tab_id);
            let mut found = false;
            while let Some(id) = current {
                let tab = self.tabs.get(id);
                match tab {
                    Some(t) => {
                        if t.parent_id == Some(requesting_tab_id) {
                            found = true;
                            break;
                        }
                        current = t.parent_id;
                    }
                    None => break,
                }
            }
            found
        };

        if !authorized {
            self.respond_to_tab(requesting_tab_id, serde_json::json!({
                "error": "not authorized to close that tab"
            }));
            return Task::none();
        }

        let mut to_close = vec![target_tab_id];
        let mut i = 0;
        while i < to_close.len() {
            let parent = to_close[i];
            for tab in self.tabs.iter() {
                if tab.parent_id == Some(parent) && !to_close.contains(&tab.id) {
                    to_close.push(tab.id);
                }
            }
            i += 1;
        }

        if to_close.contains(&self.active_tab_id) {
            let (root_idx, root_parent) = self.tabs.index_of(target_tab_id)
                .and_then(|i| self.tabs.get_by_index(i).map(|t| (i, t.parent_id)))
                .unwrap_or((0, None));
            let new_id = self
                .pick_focus_after_close(root_parent, root_idx, &to_close)
                .or_else(|| {
                    self.tabs.iter()
                        .enumerate()
                        .filter(|(_, t)| !to_close.contains(&t.id))
                        .min_by_key(|(idx, _)| {
                            (*idx as isize - root_idx as isize).unsigned_abs()
                        })
                        .map(|(_, t)| t.id)
                });
            if let Some(id) = new_id {
                self.focus_tab(id);
            }
        }
        if self.prev_active_tab_id.is_some_and(|id| to_close.contains(&id)) {
            self.prev_active_tab_id = None;
        }

        let count = to_close.len();
        self.tabs.retain(|t| !to_close.contains(&t.id));

        if self.tabs.is_empty() {
            return iced::exit();
        }

        self.respond_to_tab(requesting_tab_id, serde_json::json!({
            "message": format!("Closed {count} tab(s)")
        }));
        Task::none()
    }

    pub(in crate::ui) fn handle_select_tab(&mut self, tab_id: usize) -> Task<Message> {
        if self.tabs.contains(tab_id) {
            self.focus_tab(tab_id);
        }
        Task::none()
    }

    pub(in crate::ui) fn handle_select_tab_by_index(&mut self, index: usize) -> Task<Message> {
        let target = self.tabs.number_assignments()
            .iter()
            .find(|&(_, &n)| n == index)
            .map(|(&id, _)| id);
        if let Some(tab_id) = target {
            self.focus_tab(tab_id);
        }
        Task::none()
    }

    pub(in crate::ui) fn handle_toggle_fold_tab(&mut self, tab_id: usize) -> Task<Message> {
        let foldable = self.tabs.get(tab_id)
            .is_some_and(|t| t.is_claude && t.rank != AgentRank::Home);
        if !foldable {
            return Task::none();
        }
        if self.tabs.is_folded(tab_id) {
            self.tabs.unfold(tab_id);
        } else if self.tabs.has_claude_children(tab_id) {
            self.tabs.fold(tab_id);
        }
        Task::none()
    }

    pub(in crate::ui) fn handle_set_selection(&mut self, sel: Option<Selection>) -> Task<Message> {
        if let Some(tab) = self.active_tab_mut() {
            tab.set_selection(sel);
        }
        Task::none()
    }

    pub(in crate::ui) fn handle_update_selection(&mut self, point: GridPoint, side: Side) -> Task<Message> {
        if let Some(tab) = self.active_tab_mut() {
            tab.update_selection(point, side);
        }
        Task::none()
    }

    pub(in crate::ui) fn handle_clipboard_load_result(
        &mut self,
        tab_id: usize,
        response: Option<String>,
    ) -> Task<Message> {
        if let Some(response) = response {
            if let Some(tab) = self.tabs.get_mut(tab_id) {
                tab.write_input(response.as_bytes());
            }
        }
        Task::none()
    }

    pub(in crate::ui) fn handle_open_pr(&mut self, tab_id: usize) -> Task<Message> {
        if let Some(tab) = self.tabs.get(tab_id) {
            if let (Some(pr), Some(dir)) = (tab.pr_number(), &tab.project_dir) {
                if let Some(slug) = crate::links::github_slug_for_dir(dir) {
                    let url = format!("https://github.com/{slug}/pull/{pr}");
                    let _ = open::that(url);
                }
            }
        }
        Task::none()
    }

    pub(in crate::ui) fn handle_tab_ready(
        &mut self,
        tab_id: usize,
        worktree_dir: Option<PathBuf>,
        session_id: Option<String>,
    ) -> Task<Message> {
        if let Some(tab) = self.tabs.get_mut(tab_id) {
            tab.worktree_dir = worktree_dir;
            tab.session_id = session_id;
        }
        Task::none()
    }

    pub(in crate::ui) fn handle_mcp_list_tabs(&mut self, requesting_tab_id: usize) -> Task<Message> {
        let is_home = self.tabs.get(requesting_tab_id)
            .is_some_and(|t| t.rank == AgentRank::Home);

        let mut visible: Vec<usize> = vec![requesting_tab_id];
        if is_home {
            visible = self.tabs.iter().map(|t| t.id).collect();
        } else {
            let mut i = 0;
            while i < visible.len() {
                let parent = visible[i];
                for t in self.tabs.iter() {
                    if t.parent_id == Some(parent) && !visible.contains(&t.id) {
                        visible.push(t.id);
                    }
                }
                i += 1;
            }
        }

        let tabs_json: Vec<serde_json::Value> = self.tabs.iter()
            .filter(|t| visible.contains(&t.id))
            .map(|t| {
                let rank = match t.rank {
                    AgentRank::Home => "home",
                    AgentRank::Project => "project",
                    AgentRank::Task => "task",
                };
                let status = match t.status {
                    AgentStatus::Idle => "idle",
                    AgentStatus::Working => "working",
                    AgentStatus::Compacting => "compacting",
                    AgentStatus::Blocked => "blocked",
                    AgentStatus::NeedsReview => "needs_review",
                    AgentStatus::Error => "error",
                };
                serde_json::json!({
                    "id": t.id,
                    "parent_id": t.parent_id,
                    "title": t.title,
                    "rank": rank,
                    "status": status,
                    "is_claude": t.is_claude,
                    "project_dir": t.project_dir.as_ref().map(|p| p.display().to_string()),
                    "worktree_dir": t.worktree_dir.as_ref().map(|p| p.display().to_string()),
                    "pr": t.pr_number(),
                })
            })
            .collect();

        self.respond_to_tab(requesting_tab_id, serde_json::json!({
            "tabs": tabs_json,
        }));
        Task::none()
    }
}

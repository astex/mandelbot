use std::collections::HashSet;
use std::path::PathBuf;

use iced::Task;

use alacritty_terminal::index::{Point as GridPoint, Side};
use alacritty_terminal::selection::Selection;

use crate::router;
use crate::tab::{AgentRank, AgentStatus, TerminalTab};

use super::super::{spawn_blocking_task, terminal_size, App, Message, PendingAutoSpawn, PendingKey};

impl App {
    pub(in crate::ui) fn handle_tab_output(
        &mut self,
        tab_id: usize,
        bg_tasks: usize,
        pr_number: Option<u32>,
    ) -> Task<Message> {
        let mut tasks: Vec<Task<Message>> = Vec::new();
        let (bell, stores, loads, osc_title) = match self.tabs.get(tab_id) {
            Some(t) => (
                t.take_bell(),
                t.take_clipboard_stores(),
                t.take_clipboard_loads(),
                (!t.is_claude).then(|| t.take_osc_title()).flatten(),
            ),
            None => return Task::none(),
        };
        if let Some(mut tab) = self.tabs.snapshot(tab_id) {
            tab.background_tasks = bg_tasks;
            tab.pr_scraped = pr_number;
            if let Some(title) = osc_title {
                tab.title = Some(title);
            }
            self.tabs.write(tab);
        }
        if bell {
            return self.bell_flashes.trigger(tab_id);
        }
        for store in stores {
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
        for load in loads {
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
                if let Some(mut tab) = self.tabs.snapshot(tab_id) {
                    tab.status = AgentStatus::Error;
                    self.tabs.write(tab);
                }
                Task::none()
            }
        }
    }

    pub(in crate::ui) fn handle_set_title(&mut self, tab_id: usize, title: String) -> Task<Message> {
        if let Some(mut tab) = self.tabs.snapshot(tab_id) {
            tab.title = Some(title);
            self.tabs.write(tab);
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
                    if let Some(existing) = self.tabs.find_project_for_dir(&canonical) {
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
                let dir = self.tabs.project_dir_for(requesting_tab_id);
                (AgentRank::Task, dir, Some(requesting_tab_id))
            }
        };

        let resolved_model = model_override.clone().unwrap_or_else(|| match rank {
            AgentRank::Home => self.config.models.home.clone(),
            AgentRank::Project => self.config.models.project.clone(),
            AgentRank::Task => self.config.models.task.clone(),
        });
        let has_prompt = prompt.as_ref().is_some_and(|p| !p.trim().is_empty());

        if resolved_model == "auto" && has_prompt {
            // Defer the spawn behind a Haiku classification pass.  The
            // MCP caller stays blocked on the parent socket until the
            // `AutoRouteResolved` handler responds with the new tab id.
            let prompt_text = prompt.as_ref().cloned().unwrap_or_default();
            let request = PendingAutoSpawn {
                is_claude: true,
                rank,
                project_dir,
                parent_id,
                prompt,
                branch,
                base,
                resume_session_id: None,
                existing_worktree: None,
                insert_position: None,
                requesting_tab_id: Some(requesting_tab_id),
            };
            let request = Box::new(request);
            return spawn_blocking_task(
                move || router::classify_blocking(&prompt_text),
                move |result| Message::AutoRouteResolved { request, result },
            );
        }

        let (new_tab_id, task) = self.spawn_tab(true, rank, project_dir, parent_id, prompt, branch, model_override, base);
        self.respond_to_tab(requesting_tab_id, serde_json::json!({"tab_id": new_tab_id}));
        task
    }

    pub(in crate::ui) fn handle_set_status(&mut self, tab_id: usize, status: AgentStatus) -> Task<Message> {
        if let Some(mut tab) = self.tabs.snapshot(tab_id) {
            tab.status = status;
            self.tabs.write(tab);
        }
        Task::none()
    }

    pub(in crate::ui) fn handle_set_pr(&mut self, tab_id: usize, pr: Option<u32>) -> Task<Message> {
        if let Some(mut tab) = self.tabs.snapshot(tab_id) {
            tab.pr_override = pr;
            self.tabs.write(tab);
        }
        Task::none()
    }

    pub(in crate::ui) fn handle_wakeup_at(&mut self, tab_id: usize, epoch_ms: u64) -> Task<Message> {
        if let Some(mut tab) = self.tabs.snapshot(tab_id) {
            tab.next_wakeup_at_ms = Some(epoch_ms);
            self.tabs.write(tab);
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
        if let Some(mut tab) = self.tabs.snapshot(tab_id) {
            if tab.next_wakeup_at_ms == Some(epoch_ms) {
                tab.next_wakeup_at_ms = None;
                self.tabs.write(tab);
            }
        }
        Task::none()
    }

    pub(in crate::ui) fn handle_pty_input(&mut self, bytes: Vec<u8>) -> Task<Message> {
        let tab_id = self.tabs.active_id();
        let transition = if let Some(tab) = self.tabs.get(tab_id) {
            tab.write_input(&bytes);
            tab.is_claude && tab.status == AgentStatus::NeedsReview && bytes == b"\r"
        } else {
            false
        };
        if transition {
            if let Some(mut tab) = self.tabs.snapshot(tab_id) {
                tab.status = AgentStatus::Working;
                self.tabs.write(tab);
            }
        }
        Task::none()
    }

    pub(in crate::ui) fn handle_scroll(&mut self, delta: i32) -> Task<Message> {
        if let Some(tab) = self.tabs.active() {
            tab.scroll(delta);
        }
        Task::none()
    }

    pub(in crate::ui) fn handle_scroll_to(&mut self, offset: usize) -> Task<Message> {
        if let Some(tab) = self.tabs.active() {
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
                let parent_id = self.tabs.active()
                    .and_then(|t| if t.rank == AgentRank::Task { t.parent_id } else { Some(t.id) });
                let project_dir = self.tabs.project_dir_for(self.tabs.active_id());
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
                if let Some(dir) = self.tabs.project_dir_for(self.tabs.active_id()) {
                    let (id, task) = self.spawn_tab(true, AgentRank::Task, Some(dir), Some(self.tabs.active_id()), None, None, None, None);
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
        let home_id = self.tabs.active_id();
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        let tab = TerminalTab::new_pending(id, rows, cols, home_id);
        self.tabs.push(tab);
        self.focus_tab(id);
        Task::none()
    }

    pub(in crate::ui) fn handle_navigate_sibling(&mut self, delta: i32) -> Task<Message> {
        let order = self.tabs.display_order();
        if let Some(idx) = order.iter().position(|&id| id == self.tabs.active_id()) {
            let new_idx = (idx as i32 + delta)
                .rem_euclid(order.len() as i32) as usize;
            let target = order[new_idx];
            self.focus_tab(target);
        }
        Task::none()
    }

    pub(in crate::ui) fn handle_navigate_rank(&mut self, delta: i32) -> Task<Message> {
        if delta > 0 {
            if let Some(child) = self.tabs.first_child(self.tabs.active_id()) {
                self.focus_tab(child);
            }
        } else {
            if let Some(tab) = self.tabs.active() {
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
        let cur = order.iter().position(|&id| id == self.tabs.active_id()).unwrap_or(0);

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
        let tab_id = self.tabs.active_id();
        let Some(mut tab) = self.tabs.snapshot(tab_id) else { return Task::none() };
        let Some(input) = &mut tab.pending_input else { return Task::none() };

        match key {
            PendingKey::Char(c) => {
                input.push(c);
                self.tabs.write(tab);
                Task::none()
            }
            PendingKey::Backspace => {
                input.pop();
                self.tabs.write(tab);
                Task::none()
            }
            PendingKey::Cancel => {
                self.close_tab(tab_id)
            }
            PendingKey::Submit => {
                let home = std::env::var("HOME").unwrap_or_default();
                let path = expand_tilde(input, &home);
                self.open_project_from_path(tab_id, path)
            }
        }
    }

    /// Open a project from `path`, consuming the pending tab `tab_id`:
    /// canonicalize, focus an already-open project if one matches,
    /// otherwise replace the pending tab with a spawned project tab.
    /// No-op when `tab_id` is gone — the folder dialog is non-modal, so
    /// its result can arrive after the pending tab was closed or
    /// submitted via a typed path.
    fn open_project_from_path(&mut self, tab_id: usize, path: PathBuf) -> Task<Message> {
        let parent_id = match self.tabs.get(tab_id) {
            Some(t) => t.parent_id,
            None => return Task::none(),
        };

        let canonical = std::fs::canonicalize(&path).unwrap_or(path);

        if let Some(existing) = self.tabs.find_project_for_dir(&canonical) {
            self.focus_tab(existing);
            return self.close_tab(tab_id);
        }

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

    pub(in crate::ui) fn handle_open_project_dialog(&mut self, tab_id: usize) -> Task<Message> {
        if self.project_dialog_open || !self.tabs.contains(tab_id) {
            return Task::none();
        }
        self.project_dialog_open = true;

        // The dialog future must be built here in update() — on macOS rfd
        // dialogs have to be spawned on the main thread; only the await
        // runs on the executor.
        let home = std::env::var("HOME").unwrap_or_default();
        let dialog = rfd::AsyncFileDialog::new()
            .set_title("Open Project")
            .set_directory(home)
            .pick_folder();

        Task::perform(dialog, move |handle| Message::SpawnOrFocusProjectTab {
            tab_id,
            path: handle.map(|h| h.path().to_path_buf()),
        })
    }

    pub(in crate::ui) fn handle_spawn_or_focus_project_tab(
        &mut self,
        tab_id: usize,
        path: Option<PathBuf>,
    ) -> Task<Message> {
        self.project_dialog_open = false;
        match path {
            Some(path) => self.open_project_from_path(tab_id, path),
            None => Task::none(),
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
            to_close.extend_from_slice(self.tabs.children_of(Some(parent)));
            i += 1;
        }

        if to_close.contains(&self.tabs.active_id()) {
            let (root_idx, root_parent) = self.tabs.index_of(target_tab_id)
                .and_then(|i| self.tabs.get_by_index(i).map(|t| (i, t.parent_id)))
                .unwrap_or((0, None));
            let new_id = self.tabs
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
        if let Some(tab) = self.tabs.active() {
            tab.set_selection(sel);
        }
        Task::none()
    }

    pub(in crate::ui) fn handle_update_selection(&mut self, point: GridPoint, side: Side) -> Task<Message> {
        if let Some(tab) = self.tabs.active() {
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
            if let Some(tab) = self.tabs.get(tab_id) {
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
        if let Some(mut tab) = self.tabs.snapshot(tab_id) {
            tab.worktree_dir = worktree_dir;
            tab.session_id = session_id;
            self.tabs.write(tab);
        }
        Task::none()
    }

    pub(in crate::ui) fn handle_mcp_list_tabs(&mut self, requesting_tab_id: usize) -> Task<Message> {
        let tabs_json = build_list_tabs_json(&self.tabs, requesting_tab_id);
        self.respond_to_tab(requesting_tab_id, serde_json::json!({
            "tabs": tabs_json,
        }));
        Task::none()
    }
}

/// Expand a leading `~` to `home`. Only the first occurrence is
/// replaced, matching shell behavior for `~/...` paths.
fn expand_tilde(raw: &str, home: &str) -> PathBuf {
    if raw.starts_with('~') {
        PathBuf::from(raw.replacen('~', home, 1))
    } else {
        PathBuf::from(raw)
    }
}

fn build_list_tabs_json(
    tabs: &crate::tabs::Tabs,
    requesting_tab_id: usize,
) -> Vec<serde_json::Value> {
    let is_home = tabs.get(requesting_tab_id)
        .is_some_and(|t| t.rank == AgentRank::Home);

    let editable: HashSet<usize> = if is_home {
        tabs.iter().map(|t| t.id).collect()
    } else {
        let mut frontier: Vec<usize> = vec![requesting_tab_id];
        let mut set: HashSet<usize> = HashSet::new();
        set.insert(requesting_tab_id);
        while let Some(parent) = frontier.pop() {
            for &child in tabs.children_of(Some(parent)) {
                if set.insert(child) {
                    frontier.push(child);
                }
            }
        }
        set
    };

    tabs.iter()
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
                "is_me": t.id == requesting_tab_id,
                "is_editable": editable.contains(&t.id),
            })
        })
        .collect()
}

#[cfg(test)]
mod expand_tilde_tests {
    use std::path::PathBuf;

    use super::expand_tilde;

    #[test]
    fn expands_leading_tilde() {
        assert_eq!(
            expand_tilde("~/work", "/Users/me"),
            PathBuf::from("/Users/me/work"),
        );
    }

    #[test]
    fn bare_tilde_is_home() {
        assert_eq!(expand_tilde("~", "/Users/me"), PathBuf::from("/Users/me"));
    }

    #[test]
    fn absolute_path_untouched() {
        assert_eq!(expand_tilde("/tmp/x", "/Users/me"), PathBuf::from("/tmp/x"));
    }

    #[test]
    fn interior_tilde_untouched() {
        assert_eq!(
            expand_tilde("/tmp/~x", "/Users/me"),
            PathBuf::from("/tmp/~x"),
        );
    }
}

#[cfg(test)]
mod list_tabs_tests {
    use super::build_list_tabs_json;
    use crate::tab::{AgentRank, TerminalTab};
    use crate::tabs::Tabs;

    fn tab(id: usize, parent_id: Option<usize>, rank: AgentRank) -> TerminalTab {
        TerminalTab::new(id, 24, 80, true, rank, None, parent_id, 0, None)
    }

    // Tree:
    //   1 (home)
    //   ├─ 2 (project)
    //   │  ├─ 3 (task)
    //   │  └─ 4 (task)
    //   │     └─ 5 (task)
    //   └─ 6 (project)
    //      └─ 7 (task)
    fn sample_tree() -> Tabs {
        let mut tabs = Tabs::new();
        tabs.push(tab(1, None, AgentRank::Home));
        tabs.push(tab(2, Some(1), AgentRank::Project));
        tabs.push(tab(3, Some(2), AgentRank::Task));
        tabs.push(tab(4, Some(2), AgentRank::Task));
        tabs.push(tab(5, Some(4), AgentRank::Task));
        tabs.push(tab(6, Some(1), AgentRank::Project));
        tabs.push(tab(7, Some(6), AgentRank::Task));
        tabs
    }

    fn editable_ids(json: &[serde_json::Value]) -> Vec<u64> {
        json.iter()
            .filter(|t| t["is_editable"].as_bool().unwrap())
            .map(|t| t["id"].as_u64().unwrap())
            .collect()
    }

    #[test]
    fn returns_every_tab_regardless_of_caller() {
        let tabs = sample_tree();
        for caller in [1u64, 2, 3, 4, 5, 6, 7] {
            let json = build_list_tabs_json(&tabs, caller as usize);
            let ids: Vec<u64> =
                json.iter().map(|t| t["id"].as_u64().unwrap()).collect();
            assert_eq!(ids, vec![1, 2, 3, 4, 5, 6, 7], "caller {caller}");
        }
    }

    #[test]
    fn is_me_only_true_for_caller() {
        let tabs = sample_tree();
        let json = build_list_tabs_json(&tabs, 4);
        for entry in &json {
            let id = entry["id"].as_u64().unwrap();
            let is_me = entry["is_me"].as_bool().unwrap();
            assert_eq!(is_me, id == 4, "tab {id}");
        }
    }

    #[test]
    fn home_can_edit_everything() {
        let tabs = sample_tree();
        let json = build_list_tabs_json(&tabs, 1);
        assert_eq!(editable_ids(&json), vec![1, 2, 3, 4, 5, 6, 7]);
    }

    #[test]
    fn project_edits_self_and_descendants() {
        let tabs = sample_tree();
        let json = build_list_tabs_json(&tabs, 2);
        // project 2 owns tasks 3, 4, 5 — not sibling project 6 or its subtree
        let mut ids = editable_ids(&json);
        ids.sort();
        assert_eq!(ids, vec![2, 3, 4, 5]);
    }

    #[test]
    fn task_edits_self_and_descendants_only() {
        let tabs = sample_tree();
        let json = build_list_tabs_json(&tabs, 4);
        let mut ids = editable_ids(&json);
        ids.sort();
        // task 4 reaches its child 5 but not its parent 2 or siblings
        assert_eq!(ids, vec![4, 5]);
    }

    #[test]
    fn leaf_task_edits_only_itself() {
        let tabs = sample_tree();
        let json = build_list_tabs_json(&tabs, 3);
        assert_eq!(editable_ids(&json), vec![3]);
    }
}

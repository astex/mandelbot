use std::io::Write;
use std::path::{Path, PathBuf};

use uuid::Uuid;

use iced::Task;

use crate::tab::{AgentRank, TerminalTab};

use super::super::{terminal_size, App, Message};

impl App {
    pub(super) fn active_tab_mut(&mut self) -> Option<&mut TerminalTab> {
        self.tabs.get_mut(self.active_tab_id)
    }

    pub(super) fn focus_tab(&mut self, id: usize) {
        if let Some(pid) = self.tabs.get(id).and_then(|t| t.parent_id) {
            self.tabs.unfold_ancestors(pid);
        }
        if id != self.active_tab_id {
            self.prev_active_tab_id = Some(self.active_tab_id);
        }
        self.active_tab_id = id;
        self.tabs.set_active(id);
    }

    pub(super) fn spawn_tab(
        &mut self,
        is_claude: bool,
        rank: AgentRank,
        project_dir: Option<PathBuf>,
        parent_id: Option<usize>,
        prompt: Option<String>,
        branch: Option<String>,
        model_override: Option<String>,
        base: Option<String>,
    ) -> (usize, Task<Message>) {
        self.spawn_tab_full(
            is_claude, rank, project_dir, parent_id, prompt,
            branch, model_override, base, None, None, None,
        )
    }

    pub(super) fn spawn_tab_full(
        &mut self,
        is_claude: bool,
        rank: AgentRank,
        project_dir: Option<PathBuf>,
        parent_id: Option<usize>,
        prompt: Option<String>,
        branch: Option<String>,
        model_override: Option<String>,
        base: Option<String>,
        resume_session_id: Option<String>,
        existing_worktree: Option<PathBuf>,
        insert_position: Option<usize>,
    ) -> (usize, Task<Message>) {
        if let Some(pid) = parent_id {
            self.tabs.unfold_ancestors(pid);
        }

        let Some(size) = self.window_size else {
            return (0, Task::none());
        };
        let (rows, cols) = terminal_size(size, self.config.char_width(), self.config.char_height());
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        let parent = parent_id.and_then(|pid| self.tabs.get(pid));
        let depth = parent.map_or(0, |p| p.depth + 1);
        let project_id = match rank {
            AgentRank::Home => None,
            AgentRank::Project => Some(id),
            AgentRank::Task => parent.and_then(|p| p.project_id),
        };
        let model = model_override.unwrap_or_else(|| match rank {
            AgentRank::Home => self.config.models.home.clone(),
            AgentRank::Project => self.config.models.project.clone(),
            AgentRank::Task => self.config.models.task.clone(),
        });

        let mut tab = TerminalTab::new(
            id, rows, cols, is_claude, rank,
            project_dir.clone(), parent_id, depth, project_id,
        );

        let (event_tx, event_rx) = std::sync::mpsc::channel();
        let pty_tx = event_tx.clone();
        tab.set_event_tx(event_tx);

        let inserted_idx = match insert_position {
            Some(pos) if pos <= self.tabs.len() => {
                self.tabs.insert(pos, tab);
                pos
            }
            _ => {
                self.tabs.push(tab);
                self.tabs.len() - 1
            }
        };

        if let Some(tab) = self.tabs.get_by_index(inserted_idx) {
            tab.set_colors(
                self.terminal_theme.fg,
                self.terminal_theme.bg,
                self.terminal_theme.fg,
            );
            let cw = self.config.char_width();
            let ch = self.config.char_height();
            tab.set_window_size(alacritty_terminal::event::WindowSize {
                num_lines: rows as u16,
                num_cols: cols as u16,
                cell_width: cw as u16,
                cell_height: ch as u16,
            });
        }

        let session_id = if is_claude && resume_session_id.is_none() {
            Some(Uuid::new_v4().to_string())
        } else {
            None
        };

        let params = crate::tab::TabSpawnParams {
            id,
            rows,
            cols,
            is_claude,
            rank,
            project_dir,
            shell: self.config.shell.clone(),
            workflow: self.config.workflow.clone(),
            worktree_location: self.config.worktree_location.clone(),
            model,
            parent_socket: self.parent_socket_path.clone(),
            prompt,
            branch,
            base,
            control_prefix: self.config.control_prefix.to_string(),
            session_id,
            resume_session_id,
            existing_worktree,
        };

        let tab = self.tabs.get_by_index(inserted_idx).expect("just inserted");
        let tab_task = Task::run(
            crate::tab::tab_stream(
                params,
                event_rx,
                pty_tx,
                tab.term_arc(),
                tab.listener(),
            ),
            |msg| msg,
        );

        let fifo_path = crate::tab::runtime_dir().join(format!("{id}.fifo"));
        crate::tab::create_fifo(&fifo_path);
        let fifo_task = Task::run(
            crate::tab::fifo_stream(id, fifo_path),
            |msg| msg,
        );
        (id, Task::batch([tab_task, fifo_task]))
    }

    pub(super) fn active_rank(&self) -> Option<AgentRank> {
        self.active_tab().map(|t| t.rank)
    }

    pub(super) fn project_dir_for_tab(&self, tab_id: usize) -> Option<PathBuf> {
        let project_id = self.tabs.get(tab_id)?.project_id?;
        self.tabs.get(project_id)?.project_dir.clone()
    }

    pub(super) fn first_child(&self, tab_id: usize) -> Option<usize> {
        self.tabs
            .children_of(Some(tab_id))
            .iter()
            .copied()
            .find(|&id| self.tabs.get(id).is_some_and(|t| t.is_claude))
    }

    pub(super) fn find_project_for_dir(&self, dir: &Path) -> Option<usize> {
        self.tabs.iter()
            .find(|t| t.rank == AgentRank::Project && t.project_dir.as_deref() == Some(dir))
            .map(|t| t.id)
    }

    pub(super) fn respond_to_tab(&self, tab_id: usize, response: serde_json::Value) {
        if let Some(mut stream) = self.response_writers.lock().unwrap().remove(&tab_id) {
            let mut msg = serde_json::to_string(&response).unwrap();
            msg.push('\n');
            let _ = stream.write_all(msg.as_bytes());
            let _ = stream.flush();
        }
    }

    pub(super) fn pick_focus_after_close(
        &self,
        closing_parent_id: Option<usize>,
        anchor_idx: usize,
        closing_ids: &[usize],
    ) -> Option<usize> {
        let sibling_at =
            |pos: usize| -> Option<usize> {
                self.tabs.get_by_index(pos).and_then(|t| {
                    (t.parent_id == closing_parent_id
                        && !closing_ids.contains(&t.id))
                    .then_some(t.id)
                })
            };
        let prev = (0..anchor_idx)
            .rev()
            .find_map(sibling_at);
        let next = (anchor_idx..self.tabs.len())
            .find_map(sibling_at);
        prev.or(next).or_else(|| {
            closing_parent_id.filter(|p| !closing_ids.contains(p))
        })
    }

    pub(in crate::ui) fn close_tab(&mut self, tab_id: usize) -> Task<Message> {
        let Some(idx) = self.tabs.index_of(tab_id) else {
            return Task::none();
        };

        let (tab_uuid, closing_parent_id) = {
            let closing = self.tabs.get(tab_id).expect("just found");
            (closing.uuid.clone(), closing.parent_id)
        };
        let _ = self.ckpt_store.close_tab(&tab_uuid).persist(&self.ckpt_store);

        self.tabs.close_with_promotion(tab_id);

        if self.prev_active_tab_id == Some(tab_id) {
            self.prev_active_tab_id = None;
        }

        if self.tabs.is_empty() {
            return iced::exit();
        }

        if self.active_tab_id == tab_id {
            let new_id = self
                .pick_focus_after_close(closing_parent_id, idx, &[tab_id])
                .unwrap_or_else(|| {
                    let fallback = idx.min(self.tabs.len() - 1);
                    self.tabs.get_by_index(fallback).expect("non-empty").id
                });
            self.focus_tab(new_id);
        }

        Task::none()
    }
}

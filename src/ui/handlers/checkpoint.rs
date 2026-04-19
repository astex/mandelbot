use std::collections::HashSet;

use uuid::Uuid;

use iced::Task;

use crate::checkpoint::{
    self, run_checkpoint_blocking, run_fork_blocking, CheckpointOutcome, CheckpointPrep,
    ForkOutcome, ForkPrep,
};
use crate::tab::AgentRank;

use super::super::{
    spawn_blocking_discard, spawn_blocking_task, App, CheckpointReason, ForkAction, Message,
};


impl App {
    pub(in crate::ui) fn handle_mcp_checkpoint(&mut self, tab_id: usize) -> Task<Message> {
        self.kick_checkpoint(tab_id, CheckpointReason::Mcp)
    }

    pub(in crate::ui) fn handle_auto_checkpoint(&mut self, tab_id: usize) -> Task<Message> {
        if self.config.auto_checkpoint {
            self.kick_checkpoint(tab_id, CheckpointReason::Auto)
        } else {
            Task::none()
        }
    }

    pub(in crate::ui) fn handle_checkpoint_done(
        &mut self,
        tab_id: usize,
        reason: CheckpointReason,
        result: Result<CheckpointOutcome, String>,
    ) -> Task<Message> {
        self.finish_checkpoint(tab_id, reason, result)
    }

    pub(in crate::ui) fn handle_fork_done(
        &mut self,
        source_tab_id: usize,
        action: ForkAction,
        result: Result<ForkOutcome, String>,
    ) -> Task<Message> {
        self.finish_fork(source_tab_id, action, result)
    }

    pub(in crate::ui) fn handle_undo(&mut self, tab_id: usize) -> Task<Message> {
        let Some(tab) = self.tabs.iter().find(|t| t.id == tab_id) else {
            return Task::none();
        };
        let tab_uuid = tab.uuid.clone();
        let Some(head) = self.ckpt_store.head_of(&tab_uuid).cloned() else {
            return Task::none();
        };
        let Some(parent) = self
            .ckpt_store
            .node(&head)
            .and_then(|n| n.parent.clone())
        else {
            return Task::none();
        };
        let mut new_redo = tab.redo_path.clone();
        new_redo.push(head);
        let max = crate::checkpoint_store::REDO_PATH_MAX;
        if new_redo.len() > max {
            new_redo.drain(..new_redo.len() - max);
        }
        self.kick_fork(
            tab_id,
            parent,
            ForkAction::Replace { new_redo },
        )
    }

    pub(in crate::ui) fn handle_redo(&mut self, tab_id: usize) -> Task<Message> {
        let Some(tab) = self.tabs.iter().find(|t| t.id == tab_id) else {
            return Task::none();
        };
        let mut new_redo = tab.redo_path.clone();
        let Some(target) = new_redo.pop() else {
            return Task::none();
        };
        if !self.ckpt_store.nodes.contains_key(&target) {
            return Task::none();
        }
        self.kick_fork(
            tab_id,
            target,
            ForkAction::Replace { new_redo },
        )
    }

    pub(in crate::ui) fn handle_mcp_replace(
        &mut self,
        tab_id: usize,
        ckpt_id: String,
    ) -> Task<Message> {
        self.handle_replace(tab_id, ckpt_id)
    }

    pub(in crate::ui) fn handle_mcp_fork(
        &mut self,
        tab_id: usize,
        ckpt_id: String,
        prompt: Option<String>,
    ) -> Task<Message> {
        self.handle_fork(tab_id, ckpt_id, prompt)
    }

    pub(in crate::ui) fn handle_replace(&mut self, tab_id: usize, ckpt_id: String) -> Task<Message> {
        if let Some(t) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
            t.redo_path.clear();
        }
        self.kick_fork(
            tab_id,
            ckpt_id,
            ForkAction::Replace { new_redo: Vec::new() },
        )
    }

    pub(in crate::ui) fn handle_fork(
        &mut self,
        tab_id: usize,
        ckpt_id: String,
        prompt: Option<String>,
    ) -> Task<Message> {
        if let Some(t) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
            t.redo_path.clear();
        }
        self.kick_fork(
            tab_id,
            ckpt_id,
            ForkAction::Fork { prompt },
        )
    }

    pub(super) fn kick_checkpoint(
        &mut self,
        tab_id: usize,
        reason: CheckpointReason,
    ) -> Task<Message> {
        let prep = match self.prepare_checkpoint(tab_id) {
            Ok(p) => p,
            Err(e) => {
                if matches!(reason, CheckpointReason::Mcp) {
                    self.respond_to_tab(
                        tab_id,
                        serde_json::json!({"error": e.to_string()}),
                    );
                }
                return Task::none();
            }
        };
        spawn_blocking_task(
            move || run_checkpoint_blocking(prep),
            move |result| Message::CheckpointDone {
                tab_id,
                reason,
                result: Box::new(result),
            },
        )
    }

    pub(super) fn prepare_checkpoint(
        &self,
        tab_id: usize,
    ) -> Result<CheckpointPrep, checkpoint::TimeTravelError> {
        use checkpoint::TimeTravelError as E;
        let tab = self
            .tabs
            .iter()
            .find(|t| t.id == tab_id)
            .ok_or(E::UnknownTab(tab_id))?;
        if tab.rank == AgentRank::Home {
            return Err(E::NotSupportedForRank(tab.rank));
        }
        let wt = tab.worktree_dir.clone().ok_or(E::NoWorktree)?;
        let session_id = tab.session_id.clone().ok_or(E::NoSessionId)?;
        let tab_uuid = tab.uuid.clone();
        let title = tab.title.clone();

        let parent_id = self.ckpt_store.head_of(&tab_uuid).cloned();
        let parent_commit = parent_id
            .as_deref()
            .and_then(|pid| self.ckpt_store.node(pid))
            .map(|n| n.shadow_commit.clone());
        let parent_line_count = parent_id
            .as_deref()
            .and_then(|pid| self.ckpt_store.node(pid))
            .map(|n| n.jsonl_line_count);
        let needs_root = parent_id.is_none();

        Ok(CheckpointPrep {
            wt,
            session_id,
            title,
            parent_id,
            parent_commit,
            parent_line_count,
            needs_root,
        })
    }

    pub(super) fn finish_checkpoint(
        &mut self,
        tab_id: usize,
        reason: CheckpointReason,
        result: Result<CheckpointOutcome, String>,
    ) -> Task<Message> {
        let outcome = match result {
            Ok(o) => o,
            Err(e) => {
                if matches!(reason, CheckpointReason::Mcp) {
                    self.respond_to_tab(
                        tab_id,
                        serde_json::json!({"error": e}),
                    );
                }
                return Task::none();
            }
        };

        let tab_uuid = match self.tabs.iter().find(|t| t.id == tab_id) {
            Some(t) => t.uuid.clone(),
            None => return Task::none(),
        };

        if let Some(root) = outcome.root {
            let root_id = root.id.clone();
            self.ckpt_store.insert_node(root);
            self.ckpt_store.set_head(&tab_uuid, root_id);
        }

        let response = match outcome.new_node {
            None => serde_json::json!({
                "skipped": "duplicate_of_parent",
                "parent_id": outcome.parent_id,
                "jsonl_line_count": outcome.line_count,
            }),
            Some(node) => {
                let new_id = node.id.clone();
                let shadow_commit = node.shadow_commit.clone();
                let line_count = node.jsonl_line_count;
                self.ckpt_store.insert_node(node);
                self.ckpt_store.set_head(&tab_uuid, new_id.clone());
                if let Some(t) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                    t.redo_path.clear();
                }
                let extra_protected: HashSet<String> = self
                    .tabs
                    .iter()
                    .flat_map(|t| t.redo_path.iter().cloned())
                    .collect();
                self.ckpt_store.prune_tree(&new_id, &extra_protected);
                serde_json::json!({
                    "checkpoint_id": new_id,
                    "shadow_commit": shadow_commit,
                    "jsonl_line_count": line_count,
                })
            }
        };

        let save_task = match self.ckpt_store.head_of(&tab_uuid).cloned() {
            Some(head) => self.schedule_save_tree_at(&head),
            None => Task::none(),
        };

        if let CheckpointReason::Mcp = reason {
            self.respond_to_tab(tab_id, response);
        }
        let scroll_task = if matches!(reason, CheckpointReason::TimelineOpen)
            && let Some(tab) = self.tabs.iter().find(|t| t.id == tab_id)
        {
            crate::widget::timeline::scroll_to_cursor(
                &self.ckpt_store,
                tab,
                &self.config,
            )
        } else {
            Task::none()
        };
        Task::batch([save_task, scroll_task])
    }

    /// Snapshot the checkpoint store and persist the tree containing
    /// `any_id` off the UI thread.
    pub(super) fn schedule_save_tree_at(&self, any_id: &str) -> Task<Message> {
        let store = self.ckpt_store.clone();
        let any_id = any_id.to_string();
        spawn_blocking_discard(move || {
            let _ = crate::checkpoint_store::save_tree(&store, &any_id);
        })
    }

    pub(super) fn kick_fork(
        &mut self,
        tab_id: usize,
        ckpt_id: String,
        action: ForkAction,
    ) -> Task<Message> {
        let prep = match self.prepare_fork(tab_id, ckpt_id) {
            Ok(p) => p,
            Err(e) => {
                self.respond_to_tab(
                    tab_id,
                    serde_json::json!({"error": e.to_string()}),
                );
                return Task::none();
            }
        };
        spawn_blocking_task(
            move || run_fork_blocking(prep),
            move |result| Message::ForkDone {
                source_tab_id: tab_id,
                action,
                result: Box::new(result),
            },
        )
    }

    pub(super) fn prepare_fork(
        &self,
        tab_id: usize,
        ckpt_id: String,
    ) -> Result<ForkPrep, checkpoint::TimeTravelError> {
        use checkpoint::TimeTravelError as E;
        let tab = self
            .tabs
            .iter()
            .find(|t| t.id == tab_id)
            .ok_or(E::UnknownTab(tab_id))?;
        let ckpt = self
            .ckpt_store
            .node(&ckpt_id)
            .cloned()
            .ok_or_else(|| E::CheckpointNotFound(ckpt_id.clone()))?;
        let project_dir = tab.project_dir.clone().ok_or(E::NoProjectDir)?;

        let suffix = &Uuid::new_v4().simple().to_string()[..6];
        let new_branch = format!(
            "fork-t{tab_id}-{}-{}",
            &ckpt.shadow_commit[..8],
            suffix,
        );
        let wt_path = crate::worktree::worktree_path(
            &project_dir,
            &self.config.worktree_location,
            &new_branch,
        );

        Ok(ForkPrep {
            project_dir,
            ckpt_id,
            ckpt_title: ckpt.title.clone(),
            ckpt_shadow_commit: ckpt.shadow_commit,
            ckpt_session_id: ckpt.session_id,
            ckpt_jsonl_line_count: ckpt.jsonl_line_count,
            src_worktree: ckpt.worktree_dir,
            new_branch,
            wt_path,
        })
    }

    pub(super) fn finish_fork(
        &mut self,
        source_tab_id: usize,
        action: ForkAction,
        result: Result<ForkOutcome, String>,
    ) -> Task<Message> {
        let outcome = match result {
            Ok(o) => o,
            Err(e) => {
                self.respond_to_tab(
                    source_tab_id,
                    serde_json::json!({"error": e}),
                );
                return Task::none();
            }
        };

        let Some((idx, rank, project_dir)) = self
            .tabs
            .iter()
            .enumerate()
            .find(|(_, t)| t.id == source_tab_id)
            .and_then(|(i, t)| {
                t.project_dir.clone().map(|d| (i, t.rank, d))
            })
        else {
            return Task::none();
        };

        let ckpt_title = outcome.ckpt_title.clone();

        let (prompt, keep_source, new_redo) = match action {
            ForkAction::Fork { prompt } => (prompt, true, Vec::new()),
            ForkAction::Replace { new_redo } => (None, false, new_redo),
        };

        let (new_tab_id, spawn_task) = self.spawn_tab_full(
            true,
            rank,
            Some(project_dir),
            Some(source_tab_id),
            prompt,
            Some(outcome.new_branch.clone()),
            None,
            None,
            outcome.resume_session_id.clone(),
            Some(outcome.wt_path.clone()),
            Some(idx + 1),
        );

        if let Some(new_tab) = self.tabs.iter_mut().find(|t| t.id == new_tab_id) {
            new_tab.worktree_dir = Some(outcome.wt_path.clone());
            if let Some(title) = ckpt_title {
                new_tab.title = Some(title);
            }
            new_tab.redo_path = new_redo;
            let new_tab_uuid = new_tab.uuid.clone();
            self.ckpt_store
                .set_head(&new_tab_uuid, outcome.ckpt_id.clone());
        }

        let save_task = self.schedule_save_tree_at(&outcome.ckpt_id);

        self.focus_tab(new_tab_id);
        self.respond_to_tab(
            source_tab_id,
            serde_json::json!({
                "new_tab_id": new_tab_id,
                "worktree": outcome.wt_path.to_string_lossy(),
            }),
        );

        let close_task = if keep_source {
            Task::none()
        } else {
            self.close_tab(source_tab_id)
        };

        Task::batch([spawn_task, save_task, close_task])
    }
}

use std::collections::{HashMap, HashSet};

use crate::tab::{AgentRank, TabMeta, TerminalTab};

/// Collection of `TerminalTab`s with O(1) id lookup, indexed children, and
/// cached per-frame views (`display_order`, `number_assignments`).
///
/// `by_id` and `children` are indexes derived from `tabs`. `display_order` and
/// `number_assignments` are cached views recomputed eagerly on any structural
/// change, fold/unfold, or active-tab change.
pub struct Tabs {
    tabs: Vec<TerminalTab>,
    by_id: HashMap<usize, usize>,
    children: HashMap<Option<usize>, Vec<usize>>,
    folded: HashSet<usize>,
    active_id: usize,
    display_order: Vec<usize>,
    number_assignments: HashMap<usize, usize>,
}

impl Tabs {
    pub fn new() -> Self {
        Self {
            tabs: Vec::new(),
            by_id: HashMap::new(),
            children: HashMap::new(),
            folded: HashSet::new(),
            active_id: 0,
            display_order: Vec::new(),
            number_assignments: HashMap::new(),
        }
    }

    fn rebuild_indexes(&mut self) {
        self.by_id.clear();
        self.children.clear();
        for (idx, tab) in self.tabs.iter().enumerate() {
            self.by_id.insert(tab.id, idx);
            self.children.entry(tab.parent_id).or_default().push(tab.id);
        }
    }

    fn recompute_caches(&mut self) {
        self.display_order = self.compute_display_order();
        self.number_assignments = self.compute_number_assignments();
    }

    fn recompute_all(&mut self) {
        self.rebuild_indexes();
        self.recompute_caches();
    }

    fn compute_display_order(&self) -> Vec<usize> {
        // Home is always visible — but startup/snapshot load may recompute
        // before the first push, so tolerate emptiness.
        let Some(home_id) = self.home_id() else {
            return Vec::new();
        };
        let mut order = vec![home_id];
        // Iterative preorder DFS. Push children in reverse so the leftmost
        // pops first. Folded tabs are recorded but their subtree is skipped.
        let mut stack: Vec<usize> =
            self.children_of(Some(home_id)).iter().rev().copied().collect();
        while let Some(id) = stack.pop() {
            let Some(tab) = self.get(id) else { continue };
            if !tab.is_claude {
                continue;
            }
            order.push(tab.id);
            if !self.folded.contains(&tab.id) {
                for &c in self.children_of(Some(tab.id)).iter().rev() {
                    stack.push(c);
                }
            }
        }
        for tab in self.tabs.iter().filter(|t| !t.is_claude) {
            order.push(tab.id);
        }
        order
    }

    fn compute_number_assignments(&self) -> HashMap<usize, usize> {
        let visible: &[usize] = &self.display_order;
        let is_visible = |id: usize| visible.contains(&id);

        // Home is always visible — but guard against
        // recompute-before-first-push (see compute_display_order).
        let Some(home_id) = self.home_id() else {
            return HashMap::new();
        };
        let mut eligible: HashSet<usize> = HashSet::from([home_id]);

        if let Some(shell_id) = visible.iter().copied().find(|&id| {
            self.get(id).map(|t| !t.is_claude).unwrap_or(false)
        }) {
            eligible.insert(shell_id);
        }

        if let Some(active_tab) = self.get(self.active_id) {
            let mut cur = active_tab.parent_id;
            while let Some(pid) = cur {
                if eligible.len() >= 10 { break; }
                if is_visible(pid) {
                    eligible.insert(pid);
                }
                cur = self.get(pid).and_then(|t| t.parent_id);
            }

            if eligible.len() < 10 && is_visible(active_tab.id) {
                eligible.insert(active_tab.id);
            }
            let active_parent = active_tab.parent_id;
            let active_is_claude = active_tab.is_claude;
            for t in self.tabs.iter() {
                if eligible.len() >= 10 { break; }
                if t.id != active_tab.id
                    && t.parent_id == active_parent
                    && t.is_claude == active_is_claude
                    && is_visible(t.id)
                {
                    eligible.insert(t.id);
                }
            }
        }

        let mut claude_by_depth: Vec<(usize, usize)> = visible.iter()
            .filter_map(|&id| {
                self.get(id)
                    .filter(|t| t.is_claude)
                    .map(|t| (t.depth, id))
            })
            .collect();
        claude_by_depth.sort_by_key(|&(depth, _)| depth);
        for (_, id) in claude_by_depth {
            if eligible.len() >= 10 { break; }
            eligible.insert(id);
        }
        for &id in visible.iter() {
            if eligible.len() >= 10 { break; }
            if let Some(t) = self.get(id) {
                if !t.is_claude {
                    eligible.insert(id);
                }
            }
        }

        let mut assignments = HashMap::new();
        let mut next = 0_usize;
        for &id in visible.iter() {
            if next > 9 { break; }
            if eligible.contains(&id) {
                assignments.insert(id, next);
                next += 1;
            }
        }
        assignments
    }

    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    pub fn contains(&self, id: usize) -> bool {
        self.by_id.contains_key(&id)
    }

    pub fn get(&self, id: usize) -> Option<&TerminalTab> {
        self.by_id.get(&id).map(|&i| &self.tabs[i])
    }

    /// Id of the Home tab, which is always `tabs[0]`. `None` only during
    /// startup/snapshot load, before the first push.
    pub fn home_id(&self) -> Option<usize> {
        self.tabs.first().map(|t| t.id)
    }

    /// Snapshot of tab `id`'s metadata. Pair with [`write`] for copy→mutate→write.
    pub fn snapshot(&self, id: usize) -> Option<TabMeta> {
        self.get(id).map(|t| t.meta.clone())
    }

    /// Store `meta` back on the tab with matching id. Rebuilds indexes and
    /// cached views if `parent_id` changed. Silently no-ops if no tab has
    /// that id.
    pub fn write(&mut self, meta: TabMeta) {
        let Some(&idx) = self.by_id.get(&meta.id) else { return };
        let parent_changed = self.tabs[idx].meta.parent_id != meta.parent_id;
        self.tabs[idx].meta = meta;
        if parent_changed {
            self.recompute_all();
        }
    }

    pub fn index_of(&self, id: usize) -> Option<usize> {
        self.by_id.get(&id).copied()
    }

    pub fn get_by_index(&self, idx: usize) -> Option<&TerminalTab> {
        self.tabs.get(idx)
    }

    pub fn iter(&self) -> std::slice::Iter<'_, TerminalTab> {
        self.tabs.iter()
    }

    /// Child ids of `parent_id` in vec order. Root tabs are keyed under `None`.
    pub fn children_of(&self, parent_id: Option<usize>) -> &[usize] {
        self.children
            .get(&parent_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    pub fn display_order(&self) -> &[usize] {
        &self.display_order
    }

    pub fn number_assignments(&self) -> &HashMap<usize, usize> {
        &self.number_assignments
    }

    pub fn active_id(&self) -> usize {
        self.active_id
    }

    pub fn set_active(&mut self, id: usize) {
        if self.active_id == id {
            return;
        }
        self.active_id = id;
        self.number_assignments = self.compute_number_assignments();
    }

    pub fn is_folded(&self, id: usize) -> bool {
        self.folded.contains(&id)
    }

    pub fn fold(&mut self, id: usize) {
        if self.folded.insert(id) {
            self.recompute_caches();
        }
    }

    pub fn unfold(&mut self, id: usize) {
        if self.folded.remove(&id) {
            self.recompute_caches();
        }
    }

    pub fn push(&mut self, tab: TerminalTab) {
        self.tabs.push(tab);
        self.recompute_all();
    }

    pub fn insert(&mut self, pos: usize, tab: TerminalTab) {
        self.tabs.insert(pos, tab);
        self.recompute_all();
    }

    pub fn retain<F: FnMut(&TerminalTab) -> bool>(&mut self, f: F) {
        self.tabs.retain(f);
        let alive: HashSet<usize> = self.tabs.iter().map(|t| t.id).collect();
        self.folded.retain(|id| alive.contains(id));
        self.recompute_all();
    }

    pub fn remove(&mut self, id: usize) -> Option<TerminalTab> {
        let idx = self.by_id.get(&id).copied()?;
        let tab = self.tabs.remove(idx);
        self.folded.remove(&id);
        self.recompute_all();
        Some(tab)
    }

    /// Change `id`'s parent. Keeps `children` consistent.
    pub fn reparent(&mut self, id: usize, new_parent: Option<usize>) {
        let Some(&idx) = self.by_id.get(&id) else { return };
        if self.tabs[idx].parent_id == new_parent {
            return;
        }
        self.tabs[idx].meta.parent_id = new_parent;
        self.recompute_all();
    }

    /// Move `id` into `target`'s slot among their siblings, shifting the
    /// tabs in between by one. Agent tabs are siblings when they share a
    /// parent; shell tabs are all siblings. Home never moves. Returns
    /// whether anything moved.
    pub fn move_to_sibling(&mut self, id: usize, target: usize) -> bool {
        let (Some(from), Some(to)) = (self.index_of(id), self.index_of(target)) else {
            return false;
        };
        let (a, b) = (&self.tabs[from], &self.tabs[to]);
        let siblings = a.is_claude == b.is_claude
            && (!a.is_claude || a.parent_id == b.parent_id);
        if from == to || from == 0 || to == 0 || !siblings {
            return false;
        }
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        self.recompute_all();
        true
    }

    /// Ids of every tab below `id` in the tree, excluding `id` itself.
    fn descendants(&self, id: usize) -> Vec<usize> {
        let mut out = self.children_of(Some(id)).to_vec();
        let mut i = 0;
        while i < out.len() {
            out.extend_from_slice(self.children_of(Some(out[i])));
            i += 1;
        }
        out
    }

    pub fn has_claude_children(&self, parent_id: usize) -> bool {
        self.children_of(Some(parent_id))
            .iter()
            .any(|&id| self.get(id).is_some_and(|t| t.is_claude))
    }

    /// Unfold `id` and every ancestor up to the root.
    pub fn unfold_ancestors(&mut self, mut id: usize) {
        loop {
            self.unfold(id);
            match self.get(id).and_then(|t| t.parent_id) {
                Some(pid) => id = pid,
                None => break,
            }
        }
    }

    pub fn active(&self) -> Option<&TerminalTab> {
        self.get(self.active_id)
    }

    pub fn first_child(&self, parent_id: usize) -> Option<usize> {
        self.children_of(Some(parent_id))
            .iter()
            .copied()
            .find(|&id| self.get(id).is_some_and(|t| t.is_claude))
    }

    pub fn find_project_for_dir(&self, dir: &std::path::Path) -> Option<usize> {
        self.iter()
            .find(|t| t.rank == AgentRank::Project && t.project_dir.as_deref() == Some(dir))
            .map(|t| t.id)
    }

    /// Resolve the project directory `tab_id` works in: its own
    /// `project_dir` when set (a guest tab's differs from its owning
    /// project's), otherwise the owning Project tab's via `project_id`.
    pub fn project_dir_for(&self, tab_id: usize) -> Option<std::path::PathBuf> {
        let tab = self.get(tab_id)?;
        if let Some(dir) = &tab.project_dir {
            return Some(dir.clone());
        }
        self.get(tab.project_id?)?.project_dir.clone()
    }

    /// For a guest tab — a task nested under one project but working in
    /// another — the directory of the project it works in. `None` for
    /// tabs working in their owning project.
    pub fn guest_project_dir(&self, tab_id: usize) -> Option<&std::path::Path> {
        let tab = self.get(tab_id)?;
        if tab.rank != AgentRank::Task {
            return None;
        }
        let dir = tab.project_dir.as_deref()?;
        let owner = self.get(tab.project_id?)?.project_dir.as_deref();
        (owner != Some(dir)).then_some(dir)
    }

    /// Find a sensible new active tab after closing `closing_ids` (a contiguous
    /// removal anchored at vec position `anchor_idx` under `closing_parent_id`).
    /// Prefers the previous sibling, then the next sibling, then the parent.
    /// Pure read against the current tab vec — call after the structural close.
    pub fn pick_focus_after_close(
        &self,
        closing_parent_id: Option<usize>,
        anchor_idx: usize,
        closing_ids: &[usize],
    ) -> Option<usize> {
        let sibling_at = |pos: usize| -> Option<usize> {
            self.get_by_index(pos).and_then(|t| {
                (t.parent_id == closing_parent_id && !closing_ids.contains(&t.id))
                    .then_some(t.id)
            })
        };
        let prev = (0..anchor_idx).rev().find_map(sibling_at);
        let next = (anchor_idx..self.len()).find_map(sibling_at);
        prev.or(next).or_else(|| {
            closing_parent_id.filter(|p| !closing_ids.contains(p))
        })
    }

    /// Remove `tab_id`, promoting its first child into its slot in the parent
    /// chain (inheriting its depth) and reparenting remaining children under
    /// the promoted child. No-op if `tab_id` is unknown.
    pub fn close_with_promotion(&mut self, tab_id: usize) {
        let Some(closing) = self.get(tab_id) else { return };
        let closing_parent_id = closing.parent_id;
        let closing_depth = closing.depth;
        let closing_is_project = closing.rank == AgentRank::Project;
        let closing_project_dir = closing.project_dir.clone();

        let children: Vec<usize> = self.children_of(Some(tab_id)).to_vec();
        let first_child_id = children.first().copied();

        if let Some(promoted_id) = first_child_id {
            // The promoted tab moves up a level, and its subtree with it.
            for id in self.descendants(promoted_id) {
                if let Some(mut meta) = self.snapshot(id) {
                    meta.depth = meta.depth.saturating_sub(1);
                    self.write(meta);
                }
            }
            if let Some(mut promoted) = self.snapshot(promoted_id) {
                promoted.depth = closing_depth;
                if closing_is_project {
                    // Heading a project's tree makes it the project.
                    promoted.rank = AgentRank::Project;
                    promoted.project_id = Some(promoted_id);
                    promoted.project_dir = promoted.project_dir.or(closing_project_dir);
                }
                self.write(promoted);
            }
            if closing_is_project {
                let members: Vec<usize> = self.iter()
                    .filter(|t| t.project_id == Some(tab_id))
                    .map(|t| t.id)
                    .collect();
                for id in members {
                    if let Some(mut meta) = self.snapshot(id) {
                        meta.project_id = Some(promoted_id);
                        self.write(meta);
                    }
                }
            }
            self.reparent(promoted_id, closing_parent_id);
            for &cid in children.iter().skip(1) {
                self.reparent(cid, Some(promoted_id));
            }
            // Children needn't follow their parent in vec order, so move
            // the promoted tab into the closing tab's slot among siblings.
            self.move_to_sibling(promoted_id, tab_id);
        }

        self.remove(tab_id);
    }
}

impl Default for Tabs {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tab::{AgentRank, TerminalTab};

    fn tab(id: usize, parent_id: Option<usize>) -> TerminalTab {
        TerminalTab::new(id, 24, 80, true, AgentRank::Task, None, parent_id, 0, None)
    }

    #[test]
    fn push_builds_indexes() {
        let mut tabs = Tabs::new();
        tabs.push(tab(10, None));
        tabs.push(tab(20, Some(10)));
        tabs.push(tab(30, Some(10)));

        assert_eq!(tabs.len(), 3);
        assert_eq!(tabs.get(10).map(|t| t.id), Some(10));
        assert_eq!(tabs.get(20).unwrap().parent_id, Some(10));
        assert!(tabs.get(999).is_none());
        assert_eq!(tabs.index_of(30), Some(2));
        assert_eq!(tabs.children_of(None), &[10]);
        assert_eq!(tabs.children_of(Some(10)), &[20, 30]);
    }

    #[test]
    fn insert_preserves_order_in_children_of() {
        let mut tabs = Tabs::new();
        tabs.push(tab(1, None));
        tabs.push(tab(2, Some(1)));
        tabs.push(tab(4, Some(1)));
        // Insert child 3 between 2 and 4 in vec order.
        tabs.insert(2, tab(3, Some(1)));

        assert_eq!(tabs.children_of(Some(1)), &[2, 3, 4]);
        assert_eq!(tabs.index_of(3), Some(2));
        assert_eq!(tabs.index_of(4), Some(3));
    }

    #[test]
    fn remove_clears_from_indexes_and_shifts() {
        let mut tabs = Tabs::new();
        tabs.push(tab(1, None));
        tabs.push(tab(2, Some(1)));
        tabs.push(tab(3, Some(1)));

        let removed = tabs.remove(2).map(|t| t.id);
        assert_eq!(removed, Some(2));
        assert!(!tabs.contains(2));
        assert_eq!(tabs.index_of(3), Some(1));
        assert_eq!(tabs.children_of(Some(1)), &[3]);
    }

    #[test]
    fn retain_rebuilds_indexes() {
        let mut tabs = Tabs::new();
        tabs.push(tab(1, None));
        tabs.push(tab(2, Some(1)));
        tabs.push(tab(3, Some(1)));

        tabs.retain(|t| t.id != 2);
        assert!(!tabs.contains(2));
        assert_eq!(tabs.children_of(Some(1)), &[3]);
        assert_eq!(tabs.index_of(3), Some(1));
    }

    #[test]
    fn reparent_updates_children_map() {
        let mut tabs = Tabs::new();
        tabs.push(tab(1, None));
        tabs.push(tab(2, None));
        tabs.push(tab(3, Some(1)));

        tabs.reparent(3, Some(2));
        assert_eq!(tabs.get(3).unwrap().parent_id, Some(2));
        assert_eq!(tabs.children_of(Some(1)), &[] as &[usize]);
        assert_eq!(tabs.children_of(Some(2)), &[3]);
    }

    #[test]
    fn children_of_unknown_parent_is_empty() {
        let tabs = Tabs::new();
        assert_eq!(tabs.children_of(Some(42)), &[] as &[usize]);
        assert_eq!(tabs.children_of(None), &[] as &[usize]);
    }

    fn shell(id: usize, parent_id: Option<usize>) -> TerminalTab {
        TerminalTab::new(id, 24, 80, false, AgentRank::Task, None, parent_id, 0, None)
    }

    fn project(id: usize, dir: &str) -> TerminalTab {
        TerminalTab::new(
            id, 24, 80, true, AgentRank::Project,
            Some(std::path::PathBuf::from(dir)), None, 0, Some(id),
        )
    }

    fn task_in_project(id: usize, parent_id: Option<usize>, project_id: usize, depth: usize) -> TerminalTab {
        TerminalTab::new(id, 24, 80, true, AgentRank::Task, None, parent_id, depth, Some(project_id))
    }

    #[test]
    fn has_claude_children_distinguishes_claude_from_shell() {
        let mut tabs = Tabs::new();
        tabs.push(tab(1, None));
        tabs.push(shell(2, Some(1)));
        assert!(!tabs.has_claude_children(1));
        tabs.push(tab(3, Some(1)));
        assert!(tabs.has_claude_children(1));
        assert!(!tabs.has_claude_children(99));
    }

    #[test]
    fn unfold_ancestors_walks_chain() {
        let mut tabs = Tabs::new();
        tabs.push(tab(1, None));
        tabs.push(tab(2, Some(1)));
        tabs.push(tab(3, Some(2)));
        tabs.fold(1);
        tabs.fold(2);
        tabs.fold(3);
        // unfold ancestors of 3 (3 itself, 2, 1)
        tabs.unfold_ancestors(3);
        assert!(!tabs.is_folded(1));
        assert!(!tabs.is_folded(2));
        assert!(!tabs.is_folded(3));
    }

    #[test]
    fn unfold_ancestors_unknown_id_is_safe() {
        let mut tabs = Tabs::new();
        tabs.push(tab(1, None));
        tabs.unfold_ancestors(999); // no panic; loop exits when get returns None
    }

    #[test]
    fn first_child_skips_shells() {
        let mut tabs = Tabs::new();
        tabs.push(tab(1, None));
        tabs.push(shell(2, Some(1)));
        tabs.push(tab(3, Some(1)));
        tabs.push(tab(4, Some(1)));
        assert_eq!(tabs.first_child(1), Some(3));
        assert_eq!(tabs.first_child(99), None);
    }

    #[test]
    fn find_project_for_dir_matches_project_tabs_only() {
        let mut tabs = Tabs::new();
        tabs.push(tab(1, None));
        tabs.push(project(2, "/repo/a"));
        tabs.push(project(3, "/repo/b"));
        assert_eq!(tabs.find_project_for_dir(std::path::Path::new("/repo/b")), Some(3));
        assert_eq!(tabs.find_project_for_dir(std::path::Path::new("/repo/missing")), None);
    }

    #[test]
    fn project_dir_for_follows_project_id() {
        let mut tabs = Tabs::new();
        tabs.push(project(1, "/repo/a"));
        tabs.push(task_in_project(2, Some(1), 1, 1));
        assert_eq!(
            tabs.project_dir_for(2),
            Some(std::path::PathBuf::from("/repo/a")),
        );
        assert_eq!(tabs.project_dir_for(1), Some(std::path::PathBuf::from("/repo/a")));
        assert_eq!(tabs.project_dir_for(99), None);
    }

    fn guest_in_project(id: usize, parent_id: usize, project_id: usize, dir: &str) -> TerminalTab {
        TerminalTab::new(
            id, 24, 80, true, AgentRank::Task,
            Some(std::path::PathBuf::from(dir)), Some(parent_id), 2, Some(project_id),
        )
    }

    #[test]
    fn project_dir_for_prefers_guest_dir() {
        let mut tabs = Tabs::new();
        tabs.push(project(1, "/repo/a"));
        tabs.push(guest_in_project(2, 1, 1, "/repo/b"));
        assert_eq!(
            tabs.project_dir_for(2),
            Some(std::path::PathBuf::from("/repo/b")),
        );
    }

    #[test]
    fn guest_project_dir_only_for_foreign_tasks() {
        let mut tabs = Tabs::new();
        tabs.push(project(1, "/repo/a"));
        tabs.push(guest_in_project(2, 1, 1, "/repo/a"));
        tabs.push(guest_in_project(3, 1, 1, "/repo/b"));
        tabs.push(task_in_project(4, Some(1), 1, 1));
        assert_eq!(tabs.guest_project_dir(1), None);
        assert_eq!(tabs.guest_project_dir(2), None);
        assert_eq!(
            tabs.guest_project_dir(3),
            Some(std::path::Path::new("/repo/b")),
        );
        assert_eq!(tabs.guest_project_dir(4), None);
    }

    #[test]
    fn active_returns_active_tab() {
        let mut tabs = Tabs::new();
        tabs.push(tab(1, None));
        tabs.push(tab(2, None));
        assert!(tabs.active().is_none());
        tabs.set_active(1);
        assert_eq!(tabs.active().map(|t| t.id), Some(1));
        tabs.set_active(2);
        assert_eq!(tabs.active().map(|t| t.id), Some(2));
    }

    #[test]
    fn pick_focus_prefers_prev_sibling() {
        let mut tabs = Tabs::new();
        tabs.push(tab(1, None));      // idx 0
        tabs.push(tab(2, Some(1)));   // idx 1
        tabs.push(tab(3, Some(1)));   // idx 2 — closing this
        tabs.push(tab(4, Some(1)));   // idx 3
        let pick = tabs.pick_focus_after_close(Some(1), 2, &[3]);
        assert_eq!(pick, Some(2));
    }

    #[test]
    fn pick_focus_falls_back_to_next_sibling() {
        let mut tabs = Tabs::new();
        tabs.push(tab(1, None));
        tabs.push(tab(2, Some(1)));   // idx 1 — closing
        tabs.push(tab(3, Some(1)));   // idx 2
        let pick = tabs.pick_focus_after_close(Some(1), 1, &[2]);
        assert_eq!(pick, Some(3));
    }

    #[test]
    fn pick_focus_falls_back_to_parent_when_no_siblings() {
        let mut tabs = Tabs::new();
        tabs.push(tab(1, None));
        tabs.push(tab(2, Some(1)));   // idx 1 — closing
        let pick = tabs.pick_focus_after_close(Some(1), 1, &[2]);
        assert_eq!(pick, Some(1));
    }

    #[test]
    fn pick_focus_returns_none_when_parent_also_closing() {
        let mut tabs = Tabs::new();
        tabs.push(tab(1, None));
        tabs.push(tab(2, Some(1)));
        let pick = tabs.pick_focus_after_close(Some(1), 1, &[1, 2]);
        assert_eq!(pick, None);
    }

    #[test]
    fn close_with_promotion_promotes_first_child_and_reparents_rest() {
        let mut tabs = Tabs::new();
        tabs.push(tab(1, None));            // root
        tabs.push(task_in_project(2, Some(1), 0, 1)); // closing
        tabs.push(task_in_project(3, Some(2), 0, 2)); // promoted
        tabs.push(task_in_project(4, Some(2), 0, 2)); // reparented under 3
        tabs.push(task_in_project(5, Some(2), 0, 2)); // reparented under 3

        tabs.close_with_promotion(2);

        assert!(!tabs.contains(2));
        // 3 took 2's slot (parent_id=1, depth=1)
        let promoted = tabs.get(3).expect("3 still present");
        assert_eq!(promoted.parent_id, Some(1));
        assert_eq!(promoted.depth, 1);
        // 4 and 5 now live under 3
        assert_eq!(tabs.children_of(Some(3)), &[4, 5]);
        assert_eq!(tabs.children_of(Some(1)), &[3]);
    }

    #[test]
    fn close_with_promotion_takes_closing_slot() {
        let mut tabs = Tabs::new();
        tabs.push(tab(1, None));
        tabs.push(project(2, "/repo/a"));
        tabs.push(project(3, "/repo/b"));
        tabs.reparent(2, Some(1));
        tabs.reparent(3, Some(1));
        // Spawned later, so they sit after both projects in vec order.
        tabs.push(task_in_project(4, Some(2), 2, 2));
        tabs.push(task_in_project(5, Some(3), 3, 2));

        tabs.close_with_promotion(2);

        assert_eq!(tabs.children_of(Some(1)), &[4, 3]);
        assert_eq!(tabs.display_order(), &[1, 4, 3, 5]);
    }

    #[test]
    fn close_with_promotion_upgrades_task_to_project() {
        let mut tabs = Tabs::new();
        tabs.push(tab(1, None));
        let mut proj = project(2, "/repo/a");
        proj.meta.depth = 1;
        tabs.push(proj);
        tabs.reparent(2, Some(1));
        let mut first = task_in_project(3, Some(2), 2, 2);
        first.meta.project_dir = None;
        tabs.push(first);
        tabs.push(task_in_project(4, Some(2), 2, 2));
        tabs.push(task_in_project(5, Some(3), 2, 3));

        tabs.close_with_promotion(2);

        let promoted = tabs.get(3).unwrap();
        assert_eq!(promoted.rank, AgentRank::Project);
        assert_eq!(promoted.project_id, Some(3));
        assert_eq!(promoted.depth, 1);
        assert_eq!(tabs.get(4).unwrap().project_id, Some(3));
        assert_eq!(tabs.get(5).unwrap().project_id, Some(3));
        assert_eq!(tabs.get(4).unwrap().depth, 2);
        assert_eq!(tabs.get(5).unwrap().depth, 2);
        assert_eq!(
            tabs.find_project_for_dir(std::path::Path::new("/repo/a")),
            Some(3),
        );
    }

    #[test]
    fn close_with_promotion_keeps_task_rank_under_task() {
        let mut tabs = Tabs::new();
        tabs.push(project(1, "/repo/a"));
        tabs.push(task_in_project(2, Some(1), 1, 1));
        tabs.push(task_in_project(3, Some(2), 1, 2));

        tabs.close_with_promotion(2);

        let promoted = tabs.get(3).unwrap();
        assert_eq!(promoted.rank, AgentRank::Task);
        assert_eq!(promoted.project_id, Some(1));
    }

    #[test]
    fn close_with_promotion_no_children_just_removes() {
        let mut tabs = Tabs::new();
        tabs.push(tab(1, None));
        tabs.push(tab(2, Some(1)));
        tabs.close_with_promotion(2);
        assert!(!tabs.contains(2));
        assert_eq!(tabs.children_of(Some(1)), &[] as &[usize]);
    }

    #[test]
    fn move_to_sibling_reorders_within_parent() {
        let mut tabs = Tabs::new();
        tabs.push(tab(1, None));
        tabs.push(tab(2, Some(1)));
        tabs.push(tab(3, Some(1)));
        tabs.push(tab(4, Some(1)));

        assert!(tabs.move_to_sibling(2, 4));
        assert_eq!(tabs.children_of(Some(1)), &[3, 4, 2]);
        assert!(tabs.move_to_sibling(2, 3));
        assert_eq!(tabs.children_of(Some(1)), &[2, 3, 4]);
        assert_eq!(tabs.display_order(), &[1, 2, 3, 4]);
    }

    #[test]
    fn move_to_sibling_rejects_non_siblings_and_home() {
        let mut tabs = Tabs::new();
        tabs.push(tab(1, None));
        tabs.push(tab(2, Some(1)));
        tabs.push(tab(3, Some(2)));
        tabs.push(tab(4, None));
        tabs.push(shell(5, Some(1)));

        assert!(!tabs.move_to_sibling(3, 2));
        assert!(!tabs.move_to_sibling(4, 1));
        assert!(!tabs.move_to_sibling(5, 2));
        assert!(!tabs.move_to_sibling(2, 2));
        assert_eq!(tabs.index_of(1), Some(0));
    }

    #[test]
    fn move_to_sibling_treats_all_shells_as_siblings() {
        let mut tabs = Tabs::new();
        tabs.push(tab(1, None));
        tabs.push(shell(2, Some(1)));
        tabs.push(tab(3, Some(1)));
        tabs.push(shell(4, None));

        assert!(tabs.move_to_sibling(4, 2));
        assert_eq!(tabs.display_order(), &[1, 3, 4, 2]);
    }

    #[test]
    fn close_with_promotion_unknown_id_is_noop() {
        let mut tabs = Tabs::new();
        tabs.push(tab(1, None));
        tabs.close_with_promotion(999);
        assert_eq!(tabs.len(), 1);
    }
}

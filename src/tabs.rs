use std::collections::{HashMap, HashSet};

use crate::tab::{TabMeta, TerminalTab};

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
        // Home is always tabs[0] and always visible — but startup/snapshot
        // load may recompute before the first push, so tolerate emptiness.
        let Some(home_id) = self.tabs.first().map(|t| t.id) else {
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

        // Home is always tabs[0] and always visible — but guard against
        // recompute-before-first-push (see compute_display_order).
        let Some(home_id) = self.tabs.first().map(|t| t.id) else {
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

    /// Remove `tab_id`, promoting its first child into its slot in the parent
    /// chain (inheriting its depth) and reparenting remaining children under
    /// the promoted child. No-op if `tab_id` is unknown.
    pub fn close_with_promotion(&mut self, tab_id: usize) {
        let Some(closing) = self.get(tab_id) else { return };
        let closing_parent_id = closing.parent_id;
        let closing_depth = closing.depth;

        let children: Vec<usize> = self.children_of(Some(tab_id)).to_vec();
        let first_child_id = children.first().copied();

        if let Some(promoted_id) = first_child_id {
            if let Some(mut promoted) = self.snapshot(promoted_id) {
                promoted.depth = closing_depth;
                self.write(promoted);
            }
            self.reparent(promoted_id, closing_parent_id);
            for &cid in children.iter().skip(1) {
                self.reparent(cid, Some(promoted_id));
            }
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
}

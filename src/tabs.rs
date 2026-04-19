use std::collections::{HashMap, HashSet};

use crate::tab::{AgentRank, TerminalTab};

/// Collection of `TerminalTab`s with O(1) id lookup, indexed children, and
/// cached per-frame views (`display_order`, `number_assignments`).
///
/// The underlying `Vec` is the authoritative ordering. `by_id` and `children`
/// are derived indexes rebuilt on every mutation. `display_order` and
/// `number_assignments` are derived views recomputed eagerly on any structural
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
        let mut order = Vec::new();
        if let Some(home) = self.tabs.iter().find(|t| t.rank == AgentRank::Home) {
            order.push(home.id);
            self.collect_claude_descendants(home.id, &mut order);
        }
        for tab in self.tabs.iter().filter(|t| !t.is_claude) {
            order.push(tab.id);
        }
        order
    }

    fn collect_claude_descendants(&self, parent_id: usize, order: &mut Vec<usize>) {
        for &child_id in self.children_of(Some(parent_id)) {
            let Some(tab) = self.get(child_id) else { continue };
            if !tab.is_claude {
                continue;
            }
            order.push(tab.id);
            if !self.folded.contains(&tab.id) {
                self.collect_claude_descendants(tab.id, order);
            }
        }
    }

    fn compute_number_assignments(&self) -> HashMap<usize, usize> {
        let visible: &[usize] = &self.display_order;
        let is_visible = |id: usize| visible.contains(&id);

        let mut eligible: HashSet<usize> = HashSet::new();

        if let Some(home) = self.tabs.iter().find(|t| t.rank == AgentRank::Home) {
            if is_visible(home.id) {
                eligible.insert(home.id);
            }
        }

        if eligible.len() < 10 {
            if let Some(shell_id) = visible.iter().copied().find(|&id| {
                self.get(id).map(|t| !t.is_claude).unwrap_or(false)
            }) {
                eligible.insert(shell_id);
            }
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

    pub fn get_mut(&mut self, id: usize) -> Option<&mut TerminalTab> {
        let i = *self.by_id.get(&id)?;
        Some(&mut self.tabs[i])
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

    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, TerminalTab> {
        self.tabs.iter_mut()
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

    pub fn remove(&mut self, id: usize) -> Option<TerminalTab> {
        let idx = self.by_id.get(&id).copied()?;
        let tab = self.tabs.remove(idx);
        self.folded.remove(&id);
        self.recompute_all();
        Some(tab)
    }

    pub fn retain<F: FnMut(&TerminalTab) -> bool>(&mut self, f: F) {
        self.tabs.retain(f);
        let alive: HashSet<usize> = self.tabs.iter().map(|t| t.id).collect();
        self.folded.retain(|id| alive.contains(id));
        self.recompute_all();
    }

    /// Change `id`'s parent. Keeps `children` consistent.
    pub fn reparent(&mut self, id: usize, new_parent: Option<usize>) {
        let Some(&idx) = self.by_id.get(&id) else { return };
        if self.tabs[idx].parent_id == new_parent {
            return;
        }
        self.tabs[idx].parent_id = new_parent;
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
            if let Some(promoted) = self.get_mut(promoted_id) {
                promoted.depth = closing_depth;
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

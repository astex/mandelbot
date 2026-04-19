use std::collections::HashMap;

use crate::tab::TerminalTab;

/// Collection of `TerminalTab`s with O(1) id lookup and indexed children.
///
/// The underlying `Vec` is the authoritative ordering used by the tab bar
/// (insertion order, respected by `children_of`). `by_id` and `children`
/// are derived indexes rebuilt on every mutation.
pub struct Tabs {
    tabs: Vec<TerminalTab>,
    by_id: HashMap<usize, usize>,
    children: HashMap<Option<usize>, Vec<usize>>,
}

impl Tabs {
    pub fn new() -> Self {
        Self {
            tabs: Vec::new(),
            by_id: HashMap::new(),
            children: HashMap::new(),
        }
    }

    fn rebuild(&mut self) {
        self.by_id.clear();
        self.children.clear();
        for (idx, tab) in self.tabs.iter().enumerate() {
            self.by_id.insert(tab.id, idx);
            self.children.entry(tab.parent_id).or_default().push(tab.id);
        }
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

    /// Clone the tab with `id`. Combined with [`write`] this is the
    /// copy→mutate→write pattern: take an owned snapshot, mutate it,
    /// then `write` it back — the write unconditionally rebuilds the
    /// indexes, so `by_id` and `children` stay consistent even if the
    /// caller changed `parent_id`.
    pub fn snapshot(&self, id: usize) -> Option<TerminalTab> {
        self.get(id).cloned()
    }

    /// Replace the existing tab with the same `id`. Rebuilds indexes.
    /// Silently no-ops if no tab with that id exists.
    pub fn write(&mut self, tab: TerminalTab) {
        let Some(&idx) = self.by_id.get(&tab.id) else { return };
        self.tabs[idx] = tab;
        self.rebuild();
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

    pub fn push(&mut self, tab: TerminalTab) {
        self.tabs.push(tab);
        self.rebuild();
    }

    pub fn insert(&mut self, pos: usize, tab: TerminalTab) {
        self.tabs.insert(pos, tab);
        self.rebuild();
    }

    pub fn remove(&mut self, id: usize) -> Option<TerminalTab> {
        let idx = self.by_id.get(&id).copied()?;
        let tab = self.tabs.remove(idx);
        self.rebuild();
        Some(tab)
    }

    pub fn retain<F: FnMut(&TerminalTab) -> bool>(&mut self, f: F) {
        self.tabs.retain(f);
        self.rebuild();
    }

    /// Change `id`'s parent. Keeps `children` consistent.
    pub fn reparent(&mut self, id: usize, new_parent: Option<usize>) {
        let Some(&idx) = self.by_id.get(&id) else { return };
        if self.tabs[idx].parent_id == new_parent {
            return;
        }
        self.tabs[idx].parent_id = new_parent;
        self.rebuild();
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
    fn children_of_unknown_parent_is_empty() {
        let tabs = Tabs::new();
        assert_eq!(tabs.children_of(Some(42)), &[] as &[usize]);
        assert_eq!(tabs.children_of(None), &[] as &[usize]);
    }
}

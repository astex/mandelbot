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

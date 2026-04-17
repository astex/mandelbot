//! Tree-shaped checkpoint store, one file per component.
//!
//! Each connected tree of checkpoints serializes to
//! `~/.mandelbot/checkpoints/<root-id>.json`. The root's own UUID is the
//! component identifier — no separate id is minted. A per-turn save
//! rewrites just that component's file. When a component's last live
//! tab closes, the file is deleted and owned JSONLs + shadow refs are
//! swept.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub type CheckpointId = String;

/// Per-tree hard node cap. Pruning triggers when a component exceeds this.
pub const MAX_NODES_PER_TREE: usize = 200;
/// Nodes with the N most-recent `created_at` values are protected from eviction.
pub const KEEP_RECENT: usize = 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointNode {
    pub id: CheckpointId,
    #[serde(default)]
    pub parent: Option<CheckpointId>,
    pub session_id: String,
    pub jsonl_line_count: usize,
    pub shadow_commit: String,
    #[serde(with = "systime_serde")]
    pub created_at: SystemTime,
    #[serde(default)]
    pub title: Option<String>,
    pub worktree_dir: PathBuf,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CheckpointStore {
    #[serde(default)]
    pub nodes: HashMap<CheckpointId, CheckpointNode>,
    #[serde(default)]
    pub tab_heads: HashMap<String, CheckpointId>,
    /// parent id → child ids. Rebuilt on load; maintained incrementally
    /// on insert/remove. Not serialized.
    #[serde(skip)]
    children: HashMap<CheckpointId, Vec<CheckpointId>>,
}

pub enum CloseOutcome {
    Unchanged,
    Kept(CheckpointId),
    Dropped(CheckpointId),
}

impl CloseOutcome {
    /// Flush whatever disk action this outcome implies.
    pub fn persist(&self, store: &CheckpointStore) -> io::Result<()> {
        match self {
            Self::Unchanged => Ok(()),
            Self::Kept(root) => save_tree_at_root(store, root),
            Self::Dropped(root) => delete_tree(root),
        }
    }
}

impl CheckpointStore {
    pub fn rebuild_children_index(&mut self) {
        self.children.clear();
        for node in self.nodes.values() {
            if let Some(pid) = &node.parent {
                self.children.entry(pid.clone()).or_default().push(node.id.clone());
            }
        }
    }

    pub fn insert_node(&mut self, node: CheckpointNode) {
        if let Some(pid) = &node.parent {
            self.children.entry(pid.clone()).or_default().push(node.id.clone());
        }
        self.nodes.insert(node.id.clone(), node);
    }

    pub fn node(&self, id: &str) -> Option<&CheckpointNode> {
        self.nodes.get(id)
    }

    pub fn set_head(&mut self, tab_uuid: &str, head: CheckpointId) {
        self.tab_heads.insert(tab_uuid.to_string(), head);
    }

    pub fn head_of(&self, tab_uuid: &str) -> Option<&CheckpointId> {
        self.tab_heads.get(tab_uuid)
    }

    /// Walk parent pointers until we hit a node with `parent = None` (or
    /// whose parent is missing from the store — treated as root).
    pub fn root_of(&self, id: &str) -> Option<CheckpointId> {
        let mut cur = self.nodes.get(id)?;
        loop {
            match cur.parent.as_deref().and_then(|p| self.nodes.get(p)) {
                Some(p) => cur = p,
                None => return Some(cur.id.clone()),
            }
        }
    }

    /// Every id reachable from `root_id` following child links.
    fn component_from_root(&self, root_id: &str) -> HashSet<CheckpointId> {
        let mut out = HashSet::new();
        if !self.nodes.contains_key(root_id) {
            return out;
        }
        let mut stack = vec![root_id.to_string()];
        while let Some(cur) = stack.pop() {
            if out.insert(cur.clone()) {
                if let Some(kids) = self.children.get(&cur) {
                    stack.extend(kids.iter().cloned());
                }
            }
        }
        out
    }

    pub fn close_tab(&mut self, tab_uuid: &str) -> CloseOutcome {
        let Some(old_head) = self.tab_heads.remove(tab_uuid) else {
            return CloseOutcome::Unchanged;
        };
        let Some(old_root) = self.root_of(&old_head) else {
            return CloseOutcome::Unchanged;
        };
        let comp = self.component_from_root(&old_root);
        let still_live = self.tab_heads.values().any(|h| comp.contains(h.as_str()));
        if still_live {
            return CloseOutcome::Kept(old_root);
        }
        self.gc_component_ids(comp);
        CloseOutcome::Dropped(old_root)
    }

    /// Boot-time orphan sweep. Returns one outcome per affected
    /// component (not per tab) — when N dead tabs share a root, callers
    /// see just the final state, avoiding save-then-save-then-delete
    /// churn.
    pub fn gc_orphans(&mut self, live_tab_uuids: &HashSet<String>) -> Vec<CloseOutcome> {
        let dead: Vec<String> = self
            .tab_heads
            .keys()
            .filter(|k| !live_tab_uuids.contains(*k))
            .cloned()
            .collect();
        let mut by_root: HashMap<CheckpointId, CloseOutcome> = HashMap::new();
        for tab_uuid in dead {
            let outcome = self.close_tab(&tab_uuid);
            let key = match &outcome {
                CloseOutcome::Unchanged => continue,
                CloseOutcome::Kept(r) | CloseOutcome::Dropped(r) => r.clone(),
            };
            by_root.insert(key, outcome);
        }
        by_root.into_values().collect()
    }

    /// Enforce the per-tree node cap on the component containing `any_id`,
    /// evicting lowest-scored unprotected nodes until under cap.
    pub fn prune_tree(&mut self, any_id: &str) {
        self.prune_tree_with(any_id, MAX_NODES_PER_TREE, KEEP_RECENT);
    }

    /// Test hook: same as `prune_tree` but with explicit cap / keep-recent.
    pub fn prune_tree_with(&mut self, any_id: &str, cap: usize, keep_recent: usize) {
        let Some(root) = self.root_of(any_id) else { return; };
        let component = self.component_from_root(&root);
        if component.len() <= cap { return; }

        let protected = self.compute_protected(&component, &root, keep_recent);
        let candidates: Vec<CheckpointId> = component
            .iter()
            .filter(|id| !protected.contains(*id))
            .cloned()
            .collect();
        if candidates.is_empty() { return; }

        let fork_set: HashSet<CheckpointId> = component
            .iter()
            .filter(|id| self.children.get(*id).map(|c| c.len() > 1).unwrap_or(false))
            .cloned()
            .collect();
        let tip_set: HashSet<CheckpointId> = self
            .tab_heads
            .values()
            .filter(|h| component.contains(h.as_str()))
            .cloned()
            .collect();

        let fork_dist = self.bfs_distances(&component, &fork_set);
        let tip_dist = self.bfs_distances(&component, &tip_set);

        let (min_age, max_age) = component
            .iter()
            .filter_map(|id| self.nodes.get(id))
            .map(|n| n.created_at.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as f64)
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), t| (lo.min(t), hi.max(t)));
        let age_span = (max_age - min_age).max(1.0);

        // Score: higher = keep. Evict lowest.
        //   age_norm    ∈ [0,1]  (newer → higher)
        //   fork_prox   ∈ (0,1]  (closer to a fork → higher)
        //   tip_prox    ∈ (0,1]  (closer to a tip → higher)
        let mut scored: Vec<(f64, CheckpointId)> = candidates
            .into_iter()
            .map(|id| {
                let n = &self.nodes[&id];
                let secs = n.created_at.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as f64;
                let age_norm = (secs - min_age) / age_span;
                let df = fork_dist.get(&id).copied().unwrap_or(usize::MAX / 2);
                let dt = tip_dist.get(&id).copied().unwrap_or(usize::MAX / 2);
                let fork_prox = 1.0 / (1.0 + df as f64);
                let tip_prox = 1.0 / (1.0 + dt as f64);
                (age_norm + fork_prox + tip_prox, id)
            })
            .collect();
        scored.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

        let to_evict = component.len().saturating_sub(cap).min(scored.len());
        for (_, victim) in scored.into_iter().take(to_evict) {
            self.evict_node(&victim);
        }
    }

    fn compute_protected(
        &self,
        component: &HashSet<CheckpointId>,
        root: &str,
        keep_recent: usize,
    ) -> HashSet<CheckpointId> {
        let mut protected = HashSet::new();
        protected.insert(root.to_string());
        // Fork points: any node with >1 child.
        for id in component {
            if self.children.get(id).map(|c| c.len() > 1).unwrap_or(false) {
                protected.insert(id.clone());
            }
        }
        // Tip spines: every live tab_head and its ancestors.
        for head in self.tab_heads.values() {
            if !component.contains(head.as_str()) { continue; }
            let mut cur = Some(head.clone());
            while let Some(id) = cur {
                if !protected.insert(id.clone()) {
                    // Already walked above this point via another tip.
                    break;
                }
                cur = self.nodes.get(&id).and_then(|n| n.parent.clone());
            }
        }
        // N most recent by created_at.
        let mut by_recent: Vec<&CheckpointNode> = component
            .iter()
            .filter_map(|id| self.nodes.get(id))
            .collect();
        by_recent.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        for n in by_recent.into_iter().take(keep_recent) {
            protected.insert(n.id.clone());
        }
        protected
    }

    fn bfs_distances(
        &self,
        component: &HashSet<CheckpointId>,
        sources: &HashSet<CheckpointId>,
    ) -> HashMap<CheckpointId, usize> {
        let mut dist: HashMap<CheckpointId, usize> = HashMap::new();
        let mut queue: VecDeque<CheckpointId> = VecDeque::new();
        for s in sources {
            if component.contains(s.as_str()) {
                dist.insert(s.clone(), 0);
                queue.push_back(s.clone());
            }
        }
        while let Some(cur) = queue.pop_front() {
            let d = dist[&cur];
            let mut neighbors: Vec<CheckpointId> = Vec::new();
            if let Some(node) = self.nodes.get(&cur) {
                if let Some(p) = &node.parent {
                    if component.contains(p.as_str()) { neighbors.push(p.clone()); }
                }
            }
            if let Some(kids) = self.children.get(&cur) {
                neighbors.extend(kids.iter().cloned());
            }
            for n in neighbors {
                if !dist.contains_key(&n) && component.contains(n.as_str()) {
                    dist.insert(n.clone(), d + 1);
                    queue.push_back(n);
                }
            }
        }
        dist
    }

    fn evict_node(&mut self, victim: &str) {
        let Some(node) = self.nodes.get(victim) else { return; };
        let victim_parent = node.parent.clone();
        let worktree = node.worktree_dir.clone();
        let shadow = shadow_ref_for(victim);
        let victim_children: Vec<CheckpointId> =
            self.children.get(victim).cloned().unwrap_or_default();

        // Reparent the victim's children to the victim's parent.
        for child_id in &victim_children {
            if let Some(child) = self.nodes.get_mut(child_id) {
                child.parent = victim_parent.clone();
            }
        }
        // Fix children index: drop victim from its parent's child list,
        // add victim's children in its place.
        if let Some(pid) = &victim_parent {
            let entry = self.children.entry(pid.clone()).or_default();
            entry.retain(|c| c != victim);
            entry.extend(victim_children.iter().cloned());
        }
        self.children.remove(victim);
        self.nodes.remove(victim);
        delete_shadow_ref(&worktree, &shadow);
    }

    fn gc_component_ids(&mut self, ids: HashSet<CheckpointId>) {
        if ids.is_empty() {
            return;
        }
        let mut jsonl_jobs: HashSet<(PathBuf, String)> = HashSet::new();
        let mut ref_jobs: Vec<(PathBuf, String)> = Vec::new();
        for id in &ids {
            if let Some(node) = self.nodes.get(id) {
                jsonl_jobs.insert((node.worktree_dir.clone(), node.session_id.clone()));
                ref_jobs.push((node.worktree_dir.clone(), shadow_ref_for(id)));
            }
        }
        for (wt, sid) in &jsonl_jobs {
            delete_jsonl(wt, sid);
        }
        for (wt, rname) in &ref_jobs {
            delete_shadow_ref(wt, rname);
        }
        for id in &ids {
            self.nodes.remove(id);
            self.children.remove(id);
        }
    }
}

pub fn shadow_ref_for(checkpoint_id: &str) -> String {
    format!("refs/heads/mandelbot-checkpoints/ckpt-{checkpoint_id}")
}

fn store_dir() -> PathBuf {
    if let Ok(override_dir) = std::env::var("MANDELBOT_CHECKPOINT_DIR") {
        return PathBuf::from(override_dir);
    }
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join(".mandelbot").join("checkpoints")
}

fn tree_file(root_id: &str) -> PathBuf {
    store_dir().join(format!("{root_id}.json"))
}

/// Save the tree containing `any_id` to its file. Walks to root
/// internally so callers don't have to.
pub fn save_tree(store: &CheckpointStore, any_id: &str) -> io::Result<()> {
    let Some(root) = store.root_of(any_id) else { return Ok(()); };
    save_tree_at_root(store, &root)
}

fn save_tree_at_root(store: &CheckpointStore, root_id: &str) -> io::Result<()> {
    let ids = store.component_from_root(root_id);
    if ids.is_empty() {
        return Ok(());
    }
    let nodes: HashMap<CheckpointId, CheckpointNode> = ids
        .iter()
        .filter_map(|id| store.nodes.get(id).map(|n| (id.clone(), n.clone())))
        .collect();
    let tab_heads: HashMap<String, CheckpointId> = store
        .tab_heads
        .iter()
        .filter(|(_, h)| ids.contains(h.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let partial = CheckpointStore {
        nodes,
        tab_heads,
        children: HashMap::new(),
    };
    let path = tree_file(root_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(&partial)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

pub fn delete_tree(root_id: &str) -> io::Result<()> {
    match std::fs::remove_file(tree_file(root_id)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

pub fn load_all() -> io::Result<CheckpointStore> {
    let dir = store_dir();
    let mut store = CheckpointStore::default();
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(store),
        Err(e) => return Err(e),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let Ok(partial) = serde_json::from_slice::<CheckpointStore>(&bytes) else {
            continue;
        };
        store.nodes.extend(partial.nodes);
        store.tab_heads.extend(partial.tab_heads);
    }
    store.rebuild_children_index();
    Ok(store)
}

fn delete_jsonl(project_path: &Path, session_id: &str) {
    let path = crate::checkpoint::jsonl_path_for(project_path, session_id);
    let _ = std::fs::remove_file(path);
}

fn delete_shadow_ref(worktree_path: &Path, shadow_ref: &str) {
    let _ = crate::checkpoint::git(worktree_path, &["update-ref", "-d", shadow_ref]);
}

mod systime_serde {
    use super::{Deserialize, Deserializer, Duration, Serializer, SystemTime, UNIX_EPOCH};

    pub fn serialize<S: Serializer>(t: &SystemTime, s: S) -> Result<S::Ok, S::Error> {
        let secs = t.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        s.serialize_u64(secs)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<SystemTime, D::Error> {
        let secs = u64::deserialize(d)?;
        Ok(UNIX_EPOCH + Duration::from_secs(secs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn with_temp_dir<F: FnOnce(&Path)>(f: F) {
        use std::sync::Mutex;
        static ENV_LOCK: Mutex<()> = Mutex::new(());
        let _guard = ENV_LOCK.lock().unwrap();

        let tmp = std::env::temp_dir().join(format!(
            "mandelbot-store-test-{}-{}",
            std::process::id(),
            Uuid::new_v4(),
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let prev = std::env::var("MANDELBOT_CHECKPOINT_DIR").ok();
        unsafe { std::env::set_var("MANDELBOT_CHECKPOINT_DIR", &tmp); }
        f(&tmp);
        match prev {
            Some(p) => unsafe { std::env::set_var("MANDELBOT_CHECKPOINT_DIR", p); },
            None => unsafe { std::env::remove_var("MANDELBOT_CHECKPOINT_DIR"); },
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    fn mk_node(id: &str, parent: Option<&str>, session: &str) -> CheckpointNode {
        CheckpointNode {
            id: id.into(),
            parent: parent.map(|s| s.into()),
            session_id: session.into(),
            jsonl_line_count: 0,
            shadow_commit: "deadbeef".into(),
            created_at: UNIX_EPOCH,
            title: None,
            worktree_dir: PathBuf::from("/tmp/does-not-exist"),
        }
    }

    #[test]
    fn save_tree_load_all_roundtrip() {
        with_temp_dir(|_d| {
            let mut store = CheckpointStore::default();
            store.insert_node(mk_node("a", None, "s1"));
            store.insert_node(mk_node("b", Some("a"), "s1"));
            store.set_head("tab-1", "b".into());
            // Pass the leaf — save_tree walks to root.
            save_tree(&store, "b").unwrap();
            let back = load_all().unwrap();
            assert_eq!(back.nodes.len(), 2);
            assert_eq!(back.tab_heads.get("tab-1").map(String::as_str), Some("b"));
        });
    }

    #[test]
    fn save_tree_isolates_components() {
        with_temp_dir(|_d| {
            let mut store = CheckpointStore::default();
            store.insert_node(mk_node("r1", None, "s1"));
            store.insert_node(mk_node("r2", None, "s2"));
            store.set_head("t1", "r1".into());
            store.set_head("t2", "r2".into());
            save_tree(&store, "r1").unwrap();
            save_tree(&store, "r2").unwrap();

            let back = load_all().unwrap();
            assert_eq!(back.nodes.len(), 2);
            assert!(back.tab_heads.contains_key("t1"));
            assert!(back.tab_heads.contains_key("t2"));
        });
    }

    #[test]
    fn delete_tree_removes_file() {
        with_temp_dir(|_d| {
            let mut store = CheckpointStore::default();
            store.insert_node(mk_node("r", None, "s"));
            store.set_head("t", "r".into());
            save_tree(&store, "r").unwrap();

            delete_tree("r").unwrap();
            let back = load_all().unwrap();
            assert!(back.nodes.is_empty());
            assert!(back.tab_heads.is_empty());
        });
    }

    #[test]
    fn load_missing_returns_empty() {
        with_temp_dir(|_d| {
            let store = load_all().unwrap();
            assert!(store.nodes.is_empty());
            assert!(store.tab_heads.is_empty());
        });
    }

    #[test]
    fn root_walks_parents() {
        let mut store = CheckpointStore::default();
        store.insert_node(mk_node("root", None, "s"));
        store.insert_node(mk_node("mid", Some("root"), "s"));
        store.insert_node(mk_node("leaf", Some("mid"), "s"));
        assert_eq!(store.root_of("leaf").as_deref(), Some("root"));
        assert_eq!(store.root_of("root").as_deref(), Some("root"));
    }

    #[test]
    fn close_tab_with_shared_component_reports_kept() {
        let mut store = CheckpointStore::default();
        store.insert_node(mk_node("root", None, "s-parent"));
        store.insert_node(mk_node("child-a", Some("root"), "s-parent"));
        store.insert_node(mk_node("child-b", Some("root"), "s-fork"));
        store.set_head("tab-parent", "child-a".into());
        store.set_head("tab-fork", "child-b".into());

        let outcome = store.close_tab("tab-parent");
        assert!(matches!(outcome, CloseOutcome::Kept(ref r) if r == "root"));
        assert!(store.head_of("tab-parent").is_none());
        assert!(store.nodes.contains_key("root"));
        assert!(store.nodes.contains_key("child-b"));
        assert!(store.nodes.contains_key("child-a"));
    }

    #[test]
    fn close_last_tab_reports_dropped_and_drops_component() {
        let mut store = CheckpointStore::default();
        store.insert_node(mk_node("root", None, "s"));
        store.insert_node(mk_node("mid", Some("root"), "s"));
        store.insert_node(mk_node("leaf", Some("mid"), "s"));
        store.set_head("tab", "leaf".into());

        let outcome = store.close_tab("tab");
        assert!(matches!(outcome, CloseOutcome::Dropped(ref r) if r == "root"));
        assert!(store.nodes.is_empty());
        assert!(store.tab_heads.is_empty());
    }

    #[test]
    fn gc_orphans_dedupes_per_component() {
        // Two dead tabs in the same component produce exactly one
        // Dropped outcome, not one per tab.
        let mut store = CheckpointStore::default();
        store.insert_node(mk_node("root", None, "s"));
        store.insert_node(mk_node("a", Some("root"), "sa"));
        store.insert_node(mk_node("b", Some("root"), "sb"));
        store.set_head("dead-1", "a".into());
        store.set_head("dead-2", "b".into());

        let outcomes = store.gc_orphans(&HashSet::new());
        assert_eq!(outcomes.len(), 1);
        assert!(matches!(&outcomes[0], CloseOutcome::Dropped(r) if r == "root"));
    }

    #[test]
    fn gc_orphans_reports_dropped_components() {
        let mut store = CheckpointStore::default();
        store.insert_node(mk_node("r1", None, "s1"));
        store.insert_node(mk_node("r2", None, "s2"));
        store.set_head("alive", "r1".into());
        store.set_head("dead", "r2".into());

        let mut live = HashSet::new();
        live.insert("alive".into());
        let outcomes = store.gc_orphans(&live);

        assert_eq!(outcomes.len(), 1);
        assert!(matches!(&outcomes[0], CloseOutcome::Dropped(r) if r == "r2"));
        assert!(store.nodes.contains_key("r1"));
        assert!(!store.nodes.contains_key("r2"));
    }

    fn mk_node_at(id: &str, parent: Option<&str>, secs: u64) -> CheckpointNode {
        let mut n = mk_node(id, parent, "s");
        n.created_at = UNIX_EPOCH + Duration::from_secs(secs);
        n
    }

    fn build_chain(store: &mut CheckpointStore, n: usize) {
        // root=n0, n1 -> n0, n2 -> n1, ...; n_i.created_at = i.
        for i in 0..n {
            let id = format!("n{i}");
            let parent = if i == 0 { None } else { Some(format!("n{}", i - 1)) };
            store.insert_node(mk_node_at(&id, parent.as_deref(), i as u64));
        }
    }

    #[test]
    fn prune_is_noop_under_cap() {
        let mut store = CheckpointStore::default();
        build_chain(&mut store, 5);
        store.set_head("tab", "n4".into());
        store.prune_tree_with("n4", 10, 2);
        assert_eq!(store.nodes.len(), 5);
    }

    #[test]
    fn prune_respects_all_four_protected_classes() {
        // Chain of 20 + a fork branch. Cap=6, keep_recent=2.
        // Must preserve: root (n0), fork point, tip spines, 2 most recent.
        let mut store = CheckpointStore::default();
        build_chain(&mut store, 20);
        // Add fork branch off n5.
        store.insert_node(mk_node_at("fork-a", Some("n5"), 100));
        store.insert_node(mk_node_at("fork-b", Some("n5"), 101));
        // Two live tabs: main tip and fork tip.
        store.set_head("tab-main", "n19".into());
        store.set_head("tab-fork", "fork-b".into());

        store.prune_tree_with("n0", 8, 2);

        // Root survives.
        assert!(store.nodes.contains_key("n0"), "root must survive");
        // Fork point survives.
        assert!(store.nodes.contains_key("n5"), "fork point must survive");
        // Tip spines survive.
        assert!(store.nodes.contains_key("n19"), "main tip must survive");
        assert!(store.nodes.contains_key("fork-b"), "fork tip must survive");
        // 2 most recent survive (fork-b @101, fork-a @100 are newest).
        assert!(store.nodes.contains_key("fork-a"), "2nd most recent must survive");
        // All of n0, n1..n5 (ancestors of fork point on main spine), n5..n19 on main spine
        // are protected. So the only unprotected candidates were... none on the spine.
        // Cap=8, but we have 22 nodes all protected -> should not fall below protected set.
        // Guarantee connectivity.
        assert_connected(&store);
    }

    #[test]
    fn prune_under_fork_reparents_correctly() {
        // Linear chain with a fork at middle. Evict a non-fork, non-tip node
        // and verify its child's parent pointer was rewritten.
        let mut store = CheckpointStore::default();
        // n0 -> n1 -> n2 -> n3 -> n4. Fork at n2: also n2 -> x.
        for i in 0..5 {
            let id = format!("n{i}");
            let parent = if i == 0 { None } else { Some(format!("n{}", i - 1)) };
            store.insert_node(mk_node_at(&id, parent.as_deref(), i as u64));
        }
        store.insert_node(mk_node_at("x", Some("n2"), 50));
        store.set_head("tab-main", "n4".into());
        store.set_head("tab-fork", "x".into());
        // Add extra filler nodes to force eviction pressure, all older than tips.
        // Cap=6 with 6 existing -> no-op. Add more unprotected nodes between n0 and n1? Can't
        // insert mid-chain without restructure. Instead just cap=5: forces eviction of one
        // unprotected (non-tip, non-fork, non-recent, non-root) node.
        // Protected: n0 (root), n2 (fork), n4 (tip), x (tip), n1, n3 (spines to tips via parents).
        // Wait: n1 is ancestor of n4 (via n2), yes spine. n3 is spine to n4. So all 6 protected.
        // To actually exercise reparenting, lower keep_recent doesn't help — spines cover all.
        // Rebuild: make a branch where eviction bites. Instead use a new store.
        let mut s2 = CheckpointStore::default();
        // Two-pronged tree: root=r, r->a->b->c (main chain), r->f (separate branch tip).
        // If the only tip is `c`, then `a`,`b`,`c`,`r` all protected. `f` is leaf but not a tip.
        // With cap=3, keep_recent=0: we must evict one of (f, a, b). f is not a spine,
        // not a fork (r *is* a fork, protected). f has no children -> evictable.
        s2.insert_node(mk_node_at("r", None, 0));
        s2.insert_node(mk_node_at("a", Some("r"), 1));
        s2.insert_node(mk_node_at("b", Some("a"), 2));
        s2.insert_node(mk_node_at("c", Some("b"), 3));
        s2.insert_node(mk_node_at("f", Some("r"), 4));
        s2.set_head("tab", "c".into());
        // r is now a fork (>1 child), protected. c is tip, protected. a,b on spine, protected.
        // f is leaf, unprotected. Cap=4 keep_recent=0 -> evict f.
        s2.prune_tree_with("r", 4, 0);
        assert!(!s2.nodes.contains_key("f"), "f should be evicted");
        // r should no longer be a fork — single remaining child `a`.
        assert_eq!(s2.children.get("r").map(|c| c.len()).unwrap_or(0), 1);

        // Now test actual reparent: chain r->a->b->c with cap=3. Only c is tip.
        // Protected: r (root), c (tip), a,b (spine). All protected. No eviction.
        // Need a scenario with a legitimate mid-chain non-protected node. Protection
        // by spines is total in a chain, so we need a branch whose tip is NOT a
        // registered tab_head.
        let mut s3 = CheckpointStore::default();
        // r -> a -> b -> tipL (tab=tip).  r -> x -> y (no tab — y is leaf but unprotected).
        s3.insert_node(mk_node_at("r", None, 0));
        s3.insert_node(mk_node_at("a", Some("r"), 1));
        s3.insert_node(mk_node_at("b", Some("a"), 2));
        s3.insert_node(mk_node_at("tipL", Some("b"), 3));
        s3.insert_node(mk_node_at("x", Some("r"), 4));
        s3.insert_node(mk_node_at("y", Some("x"), 5));
        s3.set_head("tab", "tipL".into());
        // Protected: r (root, also fork), tipL (tip), b, a (spine), y (2 most recent w/ keep=2).
        // Cap=4, keep_recent=1 (only y most-recent): then x is unprotected (not on spine,
        // not fork, not recent top-1). But x has child y (protected). Evicting x reparents y to r.
        s3.prune_tree_with("r", 5, 1);
        assert!(!s3.nodes.contains_key("x"), "x should be evicted");
        assert!(s3.nodes.contains_key("y"), "y (protected by recent) must survive");
        assert_eq!(
            s3.nodes.get("y").unwrap().parent.as_deref(),
            Some("r"),
            "y should be reparented to r"
        );
        assert!(
            s3.children.get("r").unwrap().iter().any(|c| c == "y"),
            "children index must show y under r"
        );
        assert_connected(&s3);
    }

    #[test]
    fn prune_is_idempotent_when_all_protected() {
        let mut store = CheckpointStore::default();
        build_chain(&mut store, 5);
        store.set_head("tab", "n4".into());
        // Chain of 5 with tip at n4 — entire spine is protected; root protected.
        // Cap=1 can't force eviction since everything is protected.
        let before = store.nodes.len();
        store.prune_tree_with("n4", 1, 0);
        assert_eq!(store.nodes.len(), before, "fully-protected tree is immutable under prune");
        store.prune_tree_with("n4", 1, 0);
        assert_eq!(store.nodes.len(), before, "prune is idempotent");
    }

    #[test]
    fn post_prune_tree_stays_connected() {
        let mut store = CheckpointStore::default();
        // Long chain with a couple of branches; aggressive cap to force many evictions.
        for i in 0..30 {
            let id = format!("n{i}");
            let parent = if i == 0 { None } else { Some(format!("n{}", i - 1)) };
            store.insert_node(mk_node_at(&id, parent.as_deref(), i as u64));
        }
        // Dead branch off n10.
        store.insert_node(mk_node_at("d1", Some("n10"), 100));
        store.insert_node(mk_node_at("d2", Some("d1"), 101));
        store.set_head("tab", "n29".into());

        store.prune_tree_with("n0", 8, 2);

        assert!(store.nodes.contains_key("n0"));
        assert!(store.nodes.contains_key("n29"));
        assert_connected(&store);
    }

    fn assert_connected(store: &CheckpointStore) {
        // Every node's parent (if Some) must exist in the store.
        for node in store.nodes.values() {
            if let Some(p) = &node.parent {
                assert!(
                    store.nodes.contains_key(p),
                    "node {} references missing parent {}",
                    node.id,
                    p
                );
            }
        }
        // Every node must be reachable from some root via parent walks.
        for node in store.nodes.values() {
            let mut cur = Some(node.id.clone());
            let mut steps = 0;
            while let Some(id) = cur {
                let n = store.nodes.get(&id).expect("dangling reference");
                cur = n.parent.clone();
                steps += 1;
                assert!(steps < 10_000, "cycle in parent chain");
            }
        }
    }

    #[test]
    fn close_tab_on_unknown_is_unchanged() {
        let mut store = CheckpointStore::default();
        let outcome = store.close_tab("never-existed");
        assert!(matches!(outcome, CloseOutcome::Unchanged));
    }
}

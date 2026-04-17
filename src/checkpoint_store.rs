//! Tree-shaped checkpoint store, one file per component.
//!
//! Each connected tree of checkpoints serializes to
//! `~/.mandelbot/checkpoints/<root-id>.json`. The root's own UUID is the
//! component identifier — no separate id is minted. A per-turn save
//! rewrites just that component's file. When a component's last live
//! tab closes, the file is deleted and owned JSONLs + shadow refs are
//! swept.

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub type CheckpointId = String;

/// Hard cap on nodes per component tree. Pruning runs after each new
/// checkpoint is inserted to keep the tree under this.
pub const MAX_NODES_PER_TREE: usize = 200;

/// Nodes this recent (by `created_at`, per component) are protected from
/// eviction regardless of other policy.
pub const MIN_RECENT_PROTECTED: usize = 20;

/// Max entries in a tab's in-memory redo stack. Bounding this also
/// bounds how many interior nodes a single tab can pin against pruning.
pub const REDO_PATH_MAX: usize = 20;

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

    /// Chain-collapse prune for the tree containing `any_id`. No-op if
    /// the component is under `MAX_NODES_PER_TREE`.
    ///
    /// Protected from eviction: root, fork points (>1 child), every
    /// live tip's spine back to root, the `MIN_RECENT_PROTECTED`
    /// most-recently-created nodes in the component, and any ids in
    /// `extra_protected` (e.g. live tabs' redo stacks).
    ///
    /// Among the rest, we identify maximal linear chains (runs of
    /// unprotected nodes where each has exactly one child) and evict
    /// the midpoint of the longest chain. Picking the midpoint keeps
    /// the chain's endpoints — which abut protected context (a fork,
    /// a tip, a recent node) — until the chain has been split down to
    /// length 1, so visual continuity near the protected boundary
    /// survives the longest.
    pub fn prune_tree(&mut self, any_id: &str, extra_protected: &HashSet<CheckpointId>) {
        self.prune_tree_with(
            any_id,
            MAX_NODES_PER_TREE,
            MIN_RECENT_PROTECTED,
            extra_protected,
        );
    }

    pub fn prune_tree_with(
        &mut self,
        any_id: &str,
        max_nodes: usize,
        min_recent: usize,
        extra_protected: &HashSet<CheckpointId>,
    ) {
        let Some(root_id) = self.root_of(any_id) else { return; };
        let ids = self.component_from_root(&root_id);
        if ids.len() <= max_nodes {
            return;
        }

        let mut protected = self.protected_set(&root_id, &ids, min_recent);
        for id in extra_protected {
            if ids.contains(id) {
                protected.insert(id.clone());
            }
        }

        // Steady state: cap is exceeded by 1, we evict 1. The loop
        // handles bulk catch-up (e.g. boot-time oversize tree or a
        // lowered cap) by re-finding the longest chain each round,
        // since eviction may merge two chains into a longer one.
        let mut live_ids = ids;
        while live_ids.len() > max_nodes {
            let chains = self.find_chains(&live_ids, &protected);
            let Some(longest) = chains.into_iter().max_by_key(|c| c.len()) else {
                break; // nothing unprotected left to evict
            };
            let victim = longest[longest.len() / 2].clone();
            self.evict_node(&victim);
            live_ids.remove(&victim);
        }
    }

    fn protected_set(
        &self,
        root_id: &str,
        ids: &HashSet<CheckpointId>,
        min_recent: usize,
    ) -> HashSet<CheckpointId> {
        let mut protected: HashSet<CheckpointId> = HashSet::new();
        protected.insert(root_id.to_string());

        // Every live tip's spine back to root.
        for head in self.tab_heads.values() {
            if !ids.contains(head.as_str()) {
                continue;
            }
            let mut cur = head.clone();
            loop {
                if !protected.insert(cur.clone()) {
                    break;
                }
                match self.nodes.get(&cur).and_then(|n| n.parent.clone()) {
                    Some(par) if ids.contains(&par) => cur = par,
                    _ => break,
                }
            }
        }

        // Fork points.
        for id in ids {
            if self.children.get(id).map(|c| c.len()).unwrap_or(0) > 1 {
                protected.insert(id.clone());
            }
        }

        // N most recent in this component.
        let mut by_time: Vec<&CheckpointNode> = ids
            .iter()
            .filter_map(|i| self.nodes.get(i))
            .collect();
        by_time.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        for n in by_time.iter().take(min_recent) {
            protected.insert(n.id.clone());
        }

        protected
    }

    fn find_chains(
        &self,
        ids: &HashSet<CheckpointId>,
        protected: &HashSet<CheckpointId>,
    ) -> Vec<Vec<CheckpointId>> {
        let eligible = |id: &str| -> bool {
            !protected.contains(id)
                && ids.contains(id)
                && self.children.get(id).map(|c| c.len()).unwrap_or(0) == 1
        };

        let mut visited: HashSet<CheckpointId> = HashSet::new();
        let mut chains: Vec<Vec<CheckpointId>> = Vec::new();
        for id in ids {
            if visited.contains(id) || !eligible(id) {
                continue;
            }
            let mut start = id.clone();
            loop {
                let parent = self.nodes.get(&start).and_then(|n| n.parent.clone());
                match parent {
                    Some(p) if eligible(&p) => start = p,
                    _ => break,
                }
            }
            let mut chain: Vec<CheckpointId> = Vec::new();
            let mut cur = start;
            loop {
                visited.insert(cur.clone());
                chain.push(cur.clone());
                let next = self
                    .children
                    .get(&cur)
                    .and_then(|c| c.first().cloned());
                match next {
                    Some(n) if eligible(&n) => cur = n,
                    _ => break,
                }
            }
            chains.push(chain);
        }
        chains
    }

    /// JSONL files are not swept here — surviving sibling nodes may
    /// share the evicted node's `session_id`.
    fn evict_node(&mut self, id: &str) {
        let Some(node) = self.nodes.get(id) else { return; };
        let parent = node.parent.clone();
        let worktree = node.worktree_dir.clone();
        let kids = self.children.get(id).cloned().unwrap_or_default();

        for kid in &kids {
            if let Some(knode) = self.nodes.get_mut(kid) {
                knode.parent = parent.clone();
            }
        }
        if let Some(pid) = &parent {
            if let Some(siblings) = self.children.get_mut(pid) {
                siblings.retain(|x| x != id);
                siblings.extend(kids.iter().cloned());
            }
        }
        self.children.remove(id);
        delete_shadow_ref(&worktree, &shadow_ref_for(id));
        self.nodes.remove(id);
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

    /// root -> n1 -> n2 -> ... -> nN linear.
    fn mk_linear(n: usize) -> CheckpointStore {
        let mut s = CheckpointStore::default();
        s.insert_node(mk_node_at("root", None, 0));
        for i in 1..=n {
            let id = format!("n{i}");
            let parent = if i == 1 { "root".to_string() } else { format!("n{}", i - 1) };
            s.insert_node(mk_node_at(&id, Some(&parent), i as u64));
        }
        s.set_head("tab", format!("n{n}"));
        s
    }

    fn assert_connected(store: &CheckpointStore) {
        for node in store.nodes.values() {
            if let Some(p) = &node.parent {
                assert!(
                    store.nodes.contains_key(p),
                    "node {} has missing parent {}",
                    node.id,
                    p
                );
            }
        }
        for (pid, kids) in &store.children {
            if !store.nodes.contains_key(pid) { continue; }
            for k in kids {
                assert!(store.nodes.contains_key(k));
                assert_eq!(store.nodes[k].parent.as_deref(), Some(pid.as_str()));
            }
        }
    }

    #[test]
    fn prune_under_cap_is_noop() {
        let mut s = mk_linear(10);
        let before = s.nodes.len();
        s.prune_tree_with("n10", 50, 3, &HashSet::new());
        assert_eq!(s.nodes.len(), before);
    }

    #[test]
    fn prune_respects_protected_classes() {
        // Tree: root -> a -> b -> c -> d -> e -> f -> g, plus b -> x
        // fork. Tabs at g and c. Recent=2 → {x, g}.
        let mut s = CheckpointStore::default();
        s.insert_node(mk_node_at("root", None, 0));
        for (id, par, t) in [
            ("a", "root", 1),
            ("b", "a", 2),
            ("c", "b", 3),
            ("d", "c", 4),
            ("e", "d", 5),
            ("f", "e", 6),
            ("g", "f", 7),
            ("x", "b", 8),
        ] {
            s.insert_node(mk_node_at(id, Some(par), t));
        }
        s.set_head("tip-main", "g".into());
        s.set_head("tip-branch", "c".into());

        // Both tip spines cover the trunk; everything is protected.
        s.prune_tree_with("g", 5, 2, &HashSet::new());
        assert_eq!(s.nodes.len(), 9);

        s.tab_heads.remove("tip-branch");
        s.prune_tree_with("g", 5, 2, &HashSet::new());
        assert_eq!(s.nodes.len(), 9);

        // No tabs, no spine protection. Eligible chain [c,d,e,f] gets
        // drained; root/b(fork)/x(recent)/g(recent)/a survive.
        s.tab_heads.clear();
        s.prune_tree_with("g", 5, 2, &HashSet::new());
        assert!(s.nodes.contains_key("root"));
        assert!(s.nodes.contains_key("b"));
        assert!(s.nodes.contains_key("x"));
        assert!(s.nodes.contains_key("g"));
        assert_eq!(s.nodes.len(), 5);
        assert_connected(&s);
    }

    #[test]
    fn prune_reparents_under_fork() {
        // root -> a -> b -> c (main) and a -> d (fork sibling).
        // After evicting b, c should reparent to a.
        let mut s = CheckpointStore::default();
        s.insert_node(mk_node_at("root", None, 0));
        s.insert_node(mk_node_at("a", Some("root"), 1));
        s.insert_node(mk_node_at("b", Some("a"), 2));
        s.insert_node(mk_node_at("c", Some("b"), 3));
        s.insert_node(mk_node_at("d", Some("a"), 4));
        // No tabs so no spine protection; recent=1 so only d is
        // recent-protected. a is a fork (2 kids). root, a, d protected.
        s.prune_tree_with("c", 3, 1, &HashSet::new());
        assert!(!s.nodes.contains_key("b"));
        assert_eq!(s.nodes.get("c").unwrap().parent.as_deref(), Some("a"));
        let a_kids = s.children.get("a").cloned().unwrap_or_default();
        assert!(a_kids.contains(&"c".to_string()));
        assert!(a_kids.contains(&"d".to_string()));
        assert!(!a_kids.contains(&"b".to_string()));
        assert_connected(&s);
    }

    #[test]
    fn prune_idempotent_when_all_protected() {
        let mut s = mk_linear(5);
        s.prune_tree_with("n5", 3, 10, &HashSet::new());
        assert_eq!(s.nodes.len(), 6);
        s.prune_tree_with("n5", 3, 10, &HashSet::new());
        assert_eq!(s.nodes.len(), 6);
        assert_connected(&s);
    }

    #[test]
    fn prune_post_tree_connected_linear() {
        let mut s = mk_linear(30);
        s.tab_heads.clear();
        s.prune_tree_with("n30", 10, 2, &HashSet::new());
        assert!(s.nodes.len() <= 10);
        assert!(s.nodes.contains_key("root"));
        assert!(s.nodes.contains_key("n30"));
        assert!(s.nodes.contains_key("n29"));
        assert_connected(&s);
        // Midpoint-only eviction never picks chain endpoints until the
        // chain is length 1, so n1 and n28 outlive the interior.
        assert!(s.nodes.contains_key("n1"));
        assert!(s.nodes.contains_key("n28"));
    }

    #[test]
    fn prune_respects_extra_protected() {
        let mut s = mk_linear(30);
        s.tab_heads.clear();
        let mut redo: HashSet<String> = HashSet::new();
        redo.insert("n10".into());
        redo.insert("n15".into());
        redo.insert("not-in-tree".into());
        s.prune_tree_with("n30", 10, 2, &redo);
        assert!(s.nodes.len() <= 10);
        assert!(s.nodes.contains_key("n10"));
        assert!(s.nodes.contains_key("n15"));
        assert!(s.nodes.contains_key("root"));
        assert!(s.nodes.contains_key("n30"));
        assert_connected(&s);
    }

    #[test]
    fn close_tab_on_unknown_is_unchanged() {
        let mut store = CheckpointStore::default();
        let outcome = store.close_tab("never-existed");
        assert!(matches!(outcome, CloseOutcome::Unchanged));
    }
}

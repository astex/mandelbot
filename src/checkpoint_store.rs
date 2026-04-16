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

    #[test]
    fn close_tab_on_unknown_is_unchanged() {
        let mut store = CheckpointStore::default();
        let outcome = store.close_tab("never-existed");
        assert!(matches!(outcome, CloseOutcome::Unchanged));
    }
}

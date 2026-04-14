//! Per-tab checkpoint persistence.
//!
//! Each tab has a stable UUID (see `TerminalTab.uuid`) and an on-disk
//! file at `~/.mandelbot/checkpoints/<tab-uuid>.json` containing its
//! checkpoints plus the small amount of metadata needed to prune owned
//! resources on close.
//!
//! Durability model: write-on-mutation. Every code path that mutates
//! `tab.checkpoints` or `tab.owned_session_ids` must call `save` for
//! that tab so a later restart (or GC sweep) sees consistent state.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::checkpoint::Checkpoint;

/// Serialized form of a tab's durable state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TabRecord {
    /// Stable UUID — matches the on-disk filename stem.
    pub tab_uuid: String,
    /// First session UUID assigned to the tab. Never a "copy we own" —
    /// pruning must never delete this JSONL.
    pub canonical_session_id: Option<String>,
    /// UUIDs the tab created via replace/fork; safe to delete on prune.
    pub owned_session_ids: Vec<String>,
    /// Worktree path at save time. Used by startup GC to decide whether
    /// a record's worktree still exists.
    pub worktree_dir: Option<PathBuf>,
    pub checkpoints: Vec<Checkpoint>,
}

fn checkpoints_dir() -> PathBuf {
    if let Ok(override_dir) = std::env::var("MANDELBOT_CHECKPOINT_DIR") {
        return PathBuf::from(override_dir);
    }
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join(".mandelbot").join("checkpoints")
}

fn record_path(tab_uuid: &str) -> PathBuf {
    checkpoints_dir().join(format!("{tab_uuid}.json"))
}

pub fn save(record: &TabRecord) -> io::Result<()> {
    let dir = checkpoints_dir();
    std::fs::create_dir_all(&dir)?;
    let path = record_path(&record.tab_uuid);
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(record)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// Read one tab's durable state. Exposed for future rehydration paths
/// (see PR-5 "Timeline UI" — when a timeline view re-links a tab to its
/// prior record by UUID).
#[allow(dead_code)]
pub fn load(tab_uuid: &str) -> io::Result<TabRecord> {
    let path = record_path(tab_uuid);
    let bytes = std::fs::read(path)?;
    serde_json::from_slice(&bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

pub fn remove(tab_uuid: &str) -> io::Result<()> {
    let path = record_path(tab_uuid);
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// List every `TabRecord` currently on disk. Used by startup GC.
pub fn load_all() -> io::Result<Vec<TabRecord>> {
    let dir = checkpoints_dir();
    let read = match std::fs::read_dir(&dir) {
        Ok(r) => r,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut out = Vec::new();
    for entry in read.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        if let Ok(bytes) = std::fs::read(&path) {
            if let Ok(record) = serde_json::from_slice::<TabRecord>(&bytes) {
                out.push(record);
            }
        }
    }
    Ok(out)
}

/// Delete a single owned JSONL copy under `~/.claude/projects/<slug>/`.
/// No-op if the file is gone.
pub fn delete_owned_jsonl(project_path: &Path, session_id: &str) {
    let path = crate::checkpoint::jsonl_path_for(project_path, session_id);
    let _ = std::fs::remove_file(path);
}

/// Advance the shadow ref to the oldest still-retained checkpoint so
/// dropped commits become unreachable (git gc reclaims later).
pub fn advance_shadow_ref(
    worktree_path: &Path,
    shadow_ref: &str,
    new_tip: &str,
) -> io::Result<()> {
    let status = std::process::Command::new("git")
        .args(["update-ref", shadow_ref, new_tip])
        .current_dir(worktree_path)
        .status()?;
    if !status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("git update-ref {shadow_ref} {new_tip}: exit {status}"),
        ));
    }
    Ok(())
}

/// Delete the shadow ref entirely — called on tab close.
pub fn delete_shadow_ref(worktree_path: &Path, shadow_ref: &str) {
    let _ = std::process::Command::new("git")
        .args(["update-ref", "-d", shadow_ref])
        .current_dir(worktree_path)
        .status();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checkpoint::uuid_v4;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Per-test checkpoint dir — tests run in parallel, so each one gets
    /// its own directory via the `MANDELBOT_CHECKPOINT_DIR` env var.
    /// Serialize access to the env so one test's dir doesn't leak into
    /// another.
    fn with_temp_checkpoint_dir<F: FnOnce(&Path)>(f: F) {
        use std::sync::Mutex;
        static ENV_LOCK: Mutex<()> = Mutex::new(());
        let _guard = ENV_LOCK.lock().unwrap();

        let tmp = std::env::temp_dir().join(format!(
            "mandelbot-store-test-{}-{}",
            std::process::id(),
            uuid_v4(),
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let prev = std::env::var("MANDELBOT_CHECKPOINT_DIR").ok();
        // SAFETY: serialized by ENV_LOCK above.
        unsafe { std::env::set_var("MANDELBOT_CHECKPOINT_DIR", &tmp); }
        f(&tmp);
        match prev {
            Some(p) => unsafe { std::env::set_var("MANDELBOT_CHECKPOINT_DIR", p); },
            None => unsafe { std::env::remove_var("MANDELBOT_CHECKPOINT_DIR"); },
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn save_load_remove_roundtrip() {
        with_temp_checkpoint_dir(|_dir| {
            let tab_uuid = uuid_v4();
            let record = TabRecord {
                tab_uuid: tab_uuid.clone(),
                canonical_session_id: Some("canon-1".into()),
                owned_session_ids: vec!["own-a".into(), "own-b".into()],
                worktree_dir: Some(PathBuf::from("/tmp/wt")),
                checkpoints: vec![Checkpoint {
                    id: 0,
                    session_id: "canon-1".into(),
                    jsonl_line_count: 17,
                    shadow_commit: "deadbeef".into(),
                    created_at: UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000),
                }],
            };
            save(&record).unwrap();
            let back = load(&tab_uuid).unwrap();
            assert_eq!(back.canonical_session_id.as_deref(), Some("canon-1"));
            assert_eq!(back.owned_session_ids.len(), 2);
            assert_eq!(back.checkpoints.len(), 1);
            assert_eq!(back.checkpoints[0].shadow_commit, "deadbeef");

            let all = load_all().unwrap();
            assert_eq!(all.len(), 1);

            remove(&tab_uuid).unwrap();
            assert!(load(&tab_uuid).is_err());
            // Second remove is a no-op.
            remove(&tab_uuid).unwrap();
        });
    }

    #[test]
    fn load_missing_is_error_but_remove_is_not() {
        with_temp_checkpoint_dir(|_dir| {
            let tab_uuid = uuid_v4();
            assert!(load(&tab_uuid).is_err());
            remove(&tab_uuid).unwrap();
        });
    }

    #[test]
    fn save_survives_systemtime_roundtrip() {
        with_temp_checkpoint_dir(|_dir| {
            let tab_uuid = uuid_v4();
            let t = SystemTime::now();
            let record = TabRecord {
                tab_uuid: tab_uuid.clone(),
                canonical_session_id: None,
                owned_session_ids: vec![],
                worktree_dir: None,
                checkpoints: vec![Checkpoint {
                    id: 0,
                    session_id: "s".into(),
                    jsonl_line_count: 0,
                    shadow_commit: "c".into(),
                    created_at: t,
                }],
            };
            save(&record).unwrap();
            let back = load(&tab_uuid).unwrap();
            // Roundtrip is second-granular; compare at that resolution.
            let orig_secs = t.duration_since(UNIX_EPOCH).unwrap().as_secs();
            let back_secs = back.checkpoints[0]
                .created_at
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            assert_eq!(orig_secs, back_secs);
        });
    }
}

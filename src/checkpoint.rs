use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::tab::AgentRank;

/// Per-tab checkpoint metadata.
///
/// PR-4 will serialize these to disk; the derives are in place now so that
/// the shape is locked from PR-1 onward.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub id: usize,
    pub session_id: String,
    pub jsonl_line_count: usize,
    pub shadow_commit: String,
    #[serde(with = "systime_serde")]
    pub created_at: SystemTime,
    /// Tab title at the moment the checkpoint was taken, so a `fork`
    /// (or future `replace`) can restore the label a reader will expect.
    #[serde(default)]
    pub title: Option<String>,
}

/// Typed errors for the time-travel handlers.
///
/// These surface through the MCP response body as a readable message —
/// callers rely on `Display`, not `Debug`.
#[derive(Debug)]
pub enum TimeTravelError {
    UnknownTab(usize),
    NoWorktree,
    NoSessionId,
    NoProjectDir,
    CheckpointNotFound(usize),
    GitFailed(String),
    JsonlCopyFailed(String),
    Io(std::io::Error),
    NotSupportedForRank(AgentRank),
}

impl fmt::Display for TimeTravelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownTab(id) => write!(f, "unknown tab: {id}"),
            Self::NoWorktree => write!(f, "tab has no worktree"),
            Self::NoSessionId => write!(f, "tab has no claude session id"),
            Self::NoProjectDir => write!(f, "tab has no project directory"),
            Self::CheckpointNotFound(id) => write!(f, "checkpoint {id} not found on this tab"),
            Self::GitFailed(s) => write!(f, "git: {s}"),
            Self::JsonlCopyFailed(s) => write!(f, "jsonl copy: {s}"),
            Self::Io(e) => write!(f, "io: {e}"),
            Self::NotSupportedForRank(r) => {
                write!(f, "time-travel is not supported for {r:?} tabs")
            }
        }
    }
}

impl std::error::Error for TimeTravelError {}

impl From<std::io::Error> for TimeTravelError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// Mint a new session UUID for use by `replace` / `fork` flows. Threading
/// through this helper (rather than raw `Uuid::new_v4()`) keeps the
/// "canonical JSONL never truncated" invariant legible: every replace/fork
/// writes to a fresh UUID, leaving the source file intact.
pub fn fresh_session_id_for(_tab_id: usize) -> String {
    Uuid::new_v4().to_string()
}

pub fn shadow_ref(tab_id: usize) -> String {
    format!("refs/heads/mandelbot-checkpoints/tab-{tab_id}")
}

/// Translate a filesystem path into Claude Code's project slug.
/// Both `/` and `.` map to `-`, matching Claude Code's own slug logic,
/// so e.g. `/home/x/.mandelbot/p` -> `-home-x--mandelbot-p`.
pub fn project_slug(path: &Path) -> String {
    path.to_string_lossy()
        .chars()
        .map(|c| if c == '/' || c == '.' { '-' } else { c })
        .collect()
}

pub fn jsonl_path_for(project_path: &Path, session_id: &str) -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    let slug = project_slug(project_path);
    PathBuf::from(home)
        .join(".claude")
        .join("projects")
        .join(slug)
        .join(format!("{session_id}.jsonl"))
}

fn git(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("git {args:?}: {e}"))?;
    if !out.status.success() {
        return Err(format!("git {args:?}: exit {}", out.status));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Create a shadow-branch commit of the full worktree state (tracked
/// + untracked + modified) without touching HEAD or the real index.
/// Returns the commit hash.
pub fn snapshot_worktree(
    worktree_path: &Path,
    shadow_ref: &str,
    message: &str,
) -> Result<String, String> {
    // Use a temp index so we don't disturb the live one.
    let tmp_index = std::env::temp_dir().join(format!(
        "mandelbot-ckpt-idx-{}-{}",
        std::process::id(),
        Uuid::new_v4(),
    ));
    let idx_str = tmp_index.to_string_lossy().to_string();

    let run = |args: &[&str]| -> Result<String, String> {
        let out = Command::new("git")
            .args(args)
            .current_dir(worktree_path)
            .env("GIT_INDEX_FILE", &idx_str)
            .stderr(Stdio::inherit())
            .output()
            .map_err(|e| format!("git {args:?}: {e}"))?;
        if !out.status.success() {
            return Err(format!("git {args:?}: exit {}", out.status));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    };

    // Seed the temp index from HEAD.
    run(&["read-tree", "HEAD"])?;
    // Stage adds + modifies.
    run(&["add", "-A"])?;
    // Stage deletions.
    run(&["add", "-u"])?;
    let tree = run(&["write-tree"])?;

    // Parent: either the current shadow tip, or HEAD for the first commit.
    let parent = git(worktree_path, &["rev-parse", "--verify", shadow_ref])
        .or_else(|_| git(worktree_path, &["rev-parse", "HEAD"]))?;

    let commit = git(
        worktree_path,
        &["commit-tree", &tree, "-p", &parent, "-m", message],
    )?;
    git(worktree_path, &["update-ref", shadow_ref, &commit])?;

    let _ = std::fs::remove_file(&tmp_index);
    Ok(commit)
}

/// Restore the worktree's file state to a given shadow commit, without
/// moving HEAD. Destroys any untracked/uncommitted changes.
///
/// Kept available for PR-3, which wires real in-place `replace`.
#[allow(dead_code)]
pub fn rewind_worktree(worktree_path: &Path, commit: &str) -> Result<(), String> {
    // Nuke untracked + tracked changes, then read the target tree.
    git(worktree_path, &["clean", "-fdx"])?;
    git(worktree_path, &["read-tree", "-u", "--reset", commit])?;
    Ok(())
}

/// Create a fresh worktree at a branch pointing to the checkpoint commit.
pub fn fork_worktree(
    project_dir: &Path,
    new_worktree_path: &Path,
    new_branch: &str,
    base_commit: &str,
) -> Result<(), String> {
    if let Some(parent) = new_worktree_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create_dir_all {parent:?}: {e}"))?;
    }
    let path_str = new_worktree_path.to_string_lossy();
    git(
        project_dir,
        &[
            "worktree",
            "add",
            "-b",
            new_branch,
            &path_str,
            base_commit,
        ],
    )?;
    Ok(())
}

/// Copy the JSONL to `dst`, truncated to `line_count` lines. We leave
/// the inner `sessionId` / `uuid` references as-is; Claude Code treats
/// the file as source of truth when resuming.
///
/// This is the **only** supported way to produce a truncated transcript
/// for `replace` / `fork`. The canonical file at
/// `~/.claude/projects/<slug>/<session-uuid>.jsonl` must never be
/// truncated in place — see the design invariant in
/// `plans/time-travel-harden.md`.
pub fn copy_truncated_jsonl(
    src: &Path,
    dst: &Path,
    line_count: usize,
) -> Result<(), String> {
    use std::io::{BufRead, BufReader, Write};
    let parent = dst.parent().ok_or("dst has no parent")?;
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("mkdir {parent:?}: {e}"))?;
    let infile = std::fs::File::open(src)
        .map_err(|e| format!("open {src:?}: {e}"))?;
    let reader = BufReader::new(infile);
    let mut outfile = std::fs::File::create(dst)
        .map_err(|e| format!("create {dst:?}: {e}"))?;
    for (i, line) in reader.lines().enumerate() {
        if i >= line_count {
            break;
        }
        let line = line.map_err(|e| format!("read: {e}"))?;
        writeln!(outfile, "{line}").map_err(|e| format!("write: {e}"))?;
    }
    Ok(())
}

pub fn count_jsonl_lines(path: &Path) -> usize {
    let Ok(f) = std::fs::File::open(path) else { return 0 };
    use std::io::BufRead;
    std::io::BufReader::new(f).lines().count()
}

mod systime_serde {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    pub fn serialize<S: Serializer>(t: &SystemTime, s: S) -> Result<S::Ok, S::Error> {
        let secs = t
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        s.serialize_u64(secs)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<SystemTime, D::Error> {
        let secs = u64::deserialize(d)?;
        Ok(UNIX_EPOCH + Duration::from_secs(secs))
    }
}

/// Convenience for the handlers: current UTC time as `SystemTime`.
pub fn now() -> SystemTime {
    SystemTime::now()
}

/// Convenience: format a `SystemTime` as unix seconds for log/display.
#[allow(dead_code)]
pub fn to_unix_secs(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(cwd: &Path, args: &[&str]) {
        let out = std::process::Command::new(args[0])
            .args(&args[1..])
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(out.status.success(), "cmd {args:?} failed: {:?}", out);
    }

    #[test]
    fn checkpoint_and_rewind_roundtrip() {
        let dir = std::env::temp_dir().join(format!(
            "mandelbot-ckpt-test-{}-{}",
            std::process::id(),
            Uuid::new_v4(),
        ));
        std::fs::create_dir_all(&dir).unwrap();
        run(&dir, &["git", "init", "-q"]);
        run(&dir, &["git", "config", "user.email", "t@t.com"]);
        run(&dir, &["git", "config", "user.name", "t"]);
        std::fs::write(dir.join("a.txt"), "v1").unwrap();
        run(&dir, &["git", "add", "."]);
        run(&dir, &["git", "commit", "-q", "-m", "init"]);

        // Checkpoint 1: modify a.txt, add untracked u1.
        std::fs::write(dir.join("a.txt"), "v2").unwrap();
        std::fs::write(dir.join("u1.txt"), "untracked").unwrap();
        let shadow = "refs/heads/mandelbot-checkpoints/tab-99";
        let c1 = snapshot_worktree(&dir, shadow, "c1").unwrap();

        // Checkpoint 2: further modify.
        std::fs::write(dir.join("a.txt"), "v3").unwrap();
        std::fs::remove_file(dir.join("u1.txt")).unwrap();
        let c2 = snapshot_worktree(&dir, shadow, "c2").unwrap();
        assert_ne!(c1, c2);

        // Dirty change after c2 (should be nuked by rewind).
        std::fs::write(dir.join("dirty.txt"), "x").unwrap();

        // Rewind to c1.
        rewind_worktree(&dir, &c1).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "v2");
        assert_eq!(std::fs::read_to_string(dir.join("u1.txt")).unwrap(), "untracked");
        assert!(!dir.join("dirty.txt").exists());

        // Fork c2 into a new worktree.
        let fork_path = dir.parent().unwrap().join(format!(
            "mandelbot-fork-{}-{}",
            std::process::id(),
            Uuid::new_v4(),
        ));
        fork_worktree(&dir, &fork_path, "fork-test", &c2).unwrap();
        assert_eq!(std::fs::read_to_string(fork_path.join("a.txt")).unwrap(), "v3");
        assert!(!fork_path.join("u1.txt").exists());

        // JSONL truncate round-trip.
        let src = dir.join("transcript.jsonl");
        std::fs::write(&src, "l1\nl2\nl3\nl4\n").unwrap();
        let dst = dir.join("transcript-out.jsonl");
        copy_truncated_jsonl(&src, &dst, 2).unwrap();
        assert_eq!(std::fs::read_to_string(&dst).unwrap(), "l1\nl2\n");
        assert_eq!(count_jsonl_lines(&src), 4);

        // Cleanup.
        let _ = std::process::Command::new("git")
            .args(["worktree", "remove", "--force", &fork_path.to_string_lossy()])
            .current_dir(&dir)
            .output();
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&fork_path);
    }

    #[test]
    fn checkpoint_roundtrip_serde() {
        let c = Checkpoint {
            id: 7,
            session_id: "abc-1".into(),
            jsonl_line_count: 42,
            shadow_commit: "deadbeef".into(),
            created_at: UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000),
            title: Some("my tab".into()),
        };
        let s = serde_json::to_string(&c).unwrap();
        let back: Checkpoint = serde_json::from_str(&s).unwrap();
        assert_eq!(back.id, c.id);
        assert_eq!(back.session_id, c.session_id);
        assert_eq!(back.jsonl_line_count, c.jsonl_line_count);
        assert_eq!(back.shadow_commit, c.shadow_commit);
        assert_eq!(back.created_at, c.created_at);
        assert_eq!(back.title, c.title);

        // Records from before this field existed still deserialize.
        let older = r#"{"id":1,"session_id":"s","jsonl_line_count":0,"shadow_commit":"x","created_at":0}"#;
        let parsed: Checkpoint = serde_json::from_str(older).unwrap();
        assert_eq!(parsed.title, None);
    }
}

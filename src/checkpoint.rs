use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use uuid::Uuid;

use crate::tab::AgentRank;

/// Errors raised by the synchronous prep stage of a time-travel
/// operation — the part that runs on the UI thread before any disk
/// work is dispatched. Disk/git failures are reported as plain
/// `String` by the blocking tasks.
#[derive(Debug)]
pub enum TimeTravelError {
    UnknownTab(usize),
    NoWorktree,
    NoSessionId,
    NoProjectDir,
    CheckpointNotFound(String),
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
            Self::NotSupportedForRank(r) => {
                write!(f, "time-travel is not supported for {r:?} tabs")
            }
        }
    }
}

impl std::error::Error for TimeTravelError {}

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

pub(crate) fn git(cwd: &Path, args: &[&str]) -> Result<String, String> {
    git_envs(cwd, &[], args)
}

fn git_envs(cwd: &Path, envs: &[(&str, &str)], args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new("git");
    cmd.args(args).current_dir(cwd).stderr(Stdio::inherit());
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let out = cmd
        .output()
        .map_err(|e| format!("git {args:?}: {e}"))?;
    if !out.status.success() {
        return Err(format!("git {args:?}: exit {}", out.status));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Create a shadow-branch commit of the full worktree state (tracked
/// + untracked + modified) without touching HEAD or the real index.
/// The new commit is parented on `parent_commit` (falling back to HEAD
/// when `None`) and the new ref `new_ref` is pointed at it so the commit
/// stays reachable for git gc. Returns the new commit's hash.
pub fn snapshot_worktree(
    worktree_path: &Path,
    parent_commit: Option<&str>,
    new_ref: &str,
    message: &str,
) -> Result<String, String> {
    // Use a temp index so we don't disturb the live one.
    let tmp_index = std::env::temp_dir().join(format!(
        "mandelbot-ckpt-idx-{}-{}",
        std::process::id(),
        Uuid::new_v4(),
    ));
    let idx_str = tmp_index.to_string_lossy().to_string();
    let idx_env = [("GIT_INDEX_FILE", idx_str.as_str())];

    git_envs(worktree_path, &idx_env, &["read-tree", "HEAD"])?;
    // `add -A` covers adds, modifications, and deletions in one pass.
    git_envs(worktree_path, &idx_env, &["add", "-A"])?;
    let tree = git_envs(worktree_path, &idx_env, &["write-tree"])?;

    let parent = match parent_commit {
        Some(p) => p.to_string(),
        None => git(worktree_path, &["rev-parse", "HEAD"])?,
    };

    let commit = git(
        worktree_path,
        &["commit-tree", &tree, "-p", &parent, "-m", message],
    )?;
    git(worktree_path, &["update-ref", new_ref, &commit])?;

    let _ = std::fs::remove_file(&tmp_index);
    Ok(commit)
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
    let mut reader = BufReader::new(infile);
    let mut outfile = std::fs::File::create(dst)
        .map_err(|e| format!("create {dst:?}: {e}"))?;
    let mut buf = Vec::with_capacity(8 * 1024);
    for _ in 0..line_count {
        buf.clear();
        let n = reader
            .read_until(b'\n', &mut buf)
            .map_err(|e| format!("read: {e}"))?;
        if n == 0 {
            break;
        }
        outfile.write_all(&buf).map_err(|e| format!("write: {e}"))?;
    }
    Ok(())
}

pub fn count_jsonl_lines(path: &Path) -> std::io::Result<usize> {
    use std::io::BufRead;
    let f = std::fs::File::open(path)?;
    let mut reader = std::io::BufReader::new(f);
    let mut buf = Vec::with_capacity(8 * 1024);
    let mut n = 0;
    loop {
        buf.clear();
        if reader.read_until(b'\n', &mut buf)? == 0 {
            break;
        }
        n += 1;
    }
    Ok(n)
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
        let ref1 = "refs/heads/mandelbot-checkpoints/ckpt-c1";
        let c1 = snapshot_worktree(&dir, None, ref1, "c1").unwrap();

        // Checkpoint 2: further modify, parented on c1.
        std::fs::write(dir.join("a.txt"), "v3").unwrap();
        std::fs::remove_file(dir.join("u1.txt")).unwrap();
        let ref2 = "refs/heads/mandelbot-checkpoints/ckpt-c2";
        let c2 = snapshot_worktree(&dir, Some(&c1), ref2, "c2").unwrap();
        assert_ne!(c1, c2);

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
        assert_eq!(count_jsonl_lines(&src).unwrap(), 4);

        // Cleanup.
        let _ = std::process::Command::new("git")
            .args(["worktree", "remove", "--force", &fork_path.to_string_lossy()])
            .current_dir(&dir)
            .output();
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&fork_path);
    }

}

#[derive(Debug, Clone)]
pub struct CheckpointOutcome {
    pub root: Option<crate::checkpoint_store::CheckpointNode>,
    /// `None` means dup-skipped: parent line count already matched.
    pub new_node: Option<crate::checkpoint_store::CheckpointNode>,
    pub parent_id: Option<String>,
    pub line_count: usize,
}

#[derive(Debug, Clone)]
pub struct ForkOutcome {
    pub ckpt_id: String,
    pub ckpt_title: Option<String>,
    pub new_branch: String,
    pub wt_path: PathBuf,
    pub resume_session_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CheckpointPrep {
    pub wt: PathBuf,
    pub session_id: String,
    pub title: Option<String>,
    pub parent_id: Option<String>,
    pub parent_commit: Option<String>,
    pub parent_line_count: Option<usize>,
    pub needs_root: bool,
}

#[derive(Debug, Clone)]
pub struct ForkPrep {
    pub project_dir: PathBuf,
    pub ckpt_id: String,
    pub ckpt_title: Option<String>,
    pub ckpt_shadow_commit: String,
    pub ckpt_session_id: String,
    pub ckpt_jsonl_line_count: usize,
    pub src_worktree: PathBuf,
    pub new_branch: String,
    pub wt_path: PathBuf,
}

/// Returns `new_node: None` when the jsonl line count hasn't advanced
/// past the parent's, signalling the dup-skip path to `finish_checkpoint`.
pub fn run_checkpoint_blocking(
    prep: CheckpointPrep,
) -> Result<CheckpointOutcome, String> {
    let now = std::time::SystemTime::now;

    let root = if prep.needs_root {
        let root_id = Uuid::new_v4().to_string();
        let root_ref = crate::checkpoint_store::shadow_ref_for(&root_id);
        let root_msg = format!("checkpoint-{root_id}");
        let root_commit =
            snapshot_worktree(&prep.wt, None, &root_ref, &root_msg)
                .map_err(|e| e.to_string())?;
        Some(crate::checkpoint_store::CheckpointNode {
            id: root_id,
            parent: None,
            session_id: prep.session_id.clone(),
            shadow_commit: root_commit,
            jsonl_line_count: 0,
            title: None,
            worktree_dir: prep.wt.clone(),
            created_at: now(),
        })
    } else {
        None
    };

    // The effective parent for the new node: the root we just
    // created, or the preexisting head. When we synthesized a root
    // this turn, the jsonl line count is 0 by construction.
    let (parent_id, parent_commit, parent_line_count) = match &root {
        Some(r) => (
            Some(r.id.clone()),
            Some(r.shadow_commit.clone()),
            Some(0_usize),
        ),
        None => (
            prep.parent_id.clone(),
            prep.parent_commit.clone(),
            prep.parent_line_count,
        ),
    };

    let jsonl = jsonl_path_for(&prep.wt, &prep.session_id);
    let line_count =
        count_jsonl_lines(&jsonl).map_err(|e| e.to_string())?;

    // Skip only when there's a real parent — a freshly synthesized root
    // always needs a following node or the head stays at line_count=0.
    if root.is_none()
        && let Some(plc) = parent_line_count
        && plc == line_count
    {
        return Ok(CheckpointOutcome {
            root,
            new_node: None,
            parent_id,
            line_count,
        });
    }

    let new_id = Uuid::new_v4().to_string();
    let new_ref = crate::checkpoint_store::shadow_ref_for(&new_id);
    let message = format!("checkpoint-{new_id}");
    let shadow_commit = snapshot_worktree(
        &prep.wt,
        parent_commit.as_deref(),
        &new_ref,
        &message,
    )
    .map_err(|e| e.to_string())?;

    Ok(CheckpointOutcome {
        root,
        new_node: Some(crate::checkpoint_store::CheckpointNode {
            id: new_id,
            parent: parent_id.clone(),
            session_id: prep.session_id,
            shadow_commit,
            jsonl_line_count: line_count,
            title: prep.title,
            worktree_dir: prep.wt,
            created_at: now(),
        }),
        parent_id,
        line_count,
    })
}

pub fn run_fork_blocking(prep: ForkPrep) -> Result<ForkOutcome, String> {
    fork_worktree(
        &prep.project_dir,
        &prep.wt_path,
        &prep.new_branch,
        &prep.ckpt_shadow_commit,
    )?;

    let resume_session_id = if prep.ckpt_jsonl_line_count == 0 {
        None
    } else {
        let new_session = Uuid::new_v4().to_string();
        let src_jsonl =
            jsonl_path_for(&prep.src_worktree, &prep.ckpt_session_id);
        let dst_jsonl = jsonl_path_for(&prep.wt_path, &new_session);
        copy_truncated_jsonl(
            &src_jsonl,
            &dst_jsonl,
            prep.ckpt_jsonl_line_count,
        )?;
        Some(new_session)
    };

    Ok(ForkOutcome {
        ckpt_id: prep.ckpt_id,
        ckpt_title: prep.ckpt_title,
        new_branch: prep.new_branch,
        wt_path: prep.wt_path,
        resume_session_id,
    })
}

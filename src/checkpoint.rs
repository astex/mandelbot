// SPIKE: time-travel / timeline-forking — cut corners freely.
// TODO(harden): no error recovery, no cross-process locking,
// no GC, no handling of detached/bare repos, hardcodes many paths.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Clone)]
pub struct Checkpoint {
    pub id: usize,
    pub commit: String,
    pub jsonl_line_count: usize,
    pub session_id: String,
}

pub fn uuid_v4() -> String {
    let mut bytes = [0u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .expect("failed to read /dev/urandom");
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5],
        bytes[6], bytes[7],
        bytes[8], bytes[9],
        bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
    )
}

pub fn shadow_ref(tab_id: usize) -> String {
    format!("refs/heads/mandelbot-checkpoints/tab-{tab_id}")
}

/// Translate a filesystem path into Claude Code's project slug.
/// e.g. `/home/x/project` -> `-home-x-project`.
pub fn project_slug(path: &Path) -> String {
    path.to_string_lossy().replace('/', "-")
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
        uuid_v4(),
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
/// SPIKE: today we only use `fork_worktree` (new worktree at the ckpt
/// commit). This in-place rewind lives here so hardening can wire
/// same-tab rewind once process lifecycle is handled.
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

#[allow(dead_code)]
pub fn truncate_jsonl_in_place(path: &Path, line_count: usize) -> Result<(), String> {
    let tmp = path.with_extension("jsonl.trunc-tmp");
    copy_truncated_jsonl(path, &tmp, line_count)?;
    std::fs::rename(&tmp, path).map_err(|e| format!("rename: {e}"))?;
    Ok(())
}

pub fn count_jsonl_lines(path: &Path) -> usize {
    let Ok(f) = std::fs::File::open(path) else { return 0 };
    use std::io::BufRead;
    std::io::BufReader::new(f).lines().count()
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
            uuid_v4(),
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
            uuid_v4(),
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
}

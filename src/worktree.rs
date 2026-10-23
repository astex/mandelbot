use std::path::{Path, PathBuf};

const ADJECTIVES: &[&str] = &[
    "bold", "bright", "calm", "cool", "dark", "deep", "fair", "fast",
    "fine", "free", "glad", "gold", "keen", "kind", "late", "lean",
    "long", "loud", "mild", "neat", "pure", "rare", "rich", "safe",
    "slim", "soft", "tall", "true", "vast", "warm", "wide", "wild",
];

const NOUNS: &[&str] = &[
    "birch", "brook", "cliff", "cloud", "coral", "crane", "creek",
    "dawn", "dune", "ember", "fern", "fjord", "flame", "flint",
    "forge", "frost", "glade", "grove", "haven", "heath", "heron",
    "larch", "lark", "marsh", "oak", "pearl", "pine", "pond",
    "ridge", "river", "sage", "shore", "slate", "spark", "stone",
    "swift", "thorn", "tide", "vale", "wren",
];

fn generate_name() -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::time::SystemTime;

    let mut hasher = DefaultHasher::new();
    SystemTime::now().hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    let hash = hasher.finish();

    let adj = ADJECTIVES[(hash as usize) % ADJECTIVES.len()];
    let noun = NOUNS[((hash >> 16) as usize) % NOUNS.len()];
    let suffix = (hash >> 32) % 1000;
    format!("{adj}-{noun}-{suffix}")
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Return the worktree directory name: the branch name when provided,
/// otherwise a randomly generated name.
pub fn worktree_name(branch: Option<&str>) -> String {
    match branch {
        Some(b) if !b.is_empty() => b.to_string(),
        _ => generate_name(),
    }
}

/// Compute the absolute path for a worktree given the project directory,
/// the configured worktree location, and the worktree name.
pub fn worktree_path(project_dir: &Path, worktree_location: &str, name: &str) -> PathBuf {
    let base = PathBuf::from(worktree_location);
    let base = if base.is_absolute() { base } else { project_dir.join(base) };
    base.join(name)
}

/// Shell snippet that copies `<project>/.claude/settings.local.json` into
/// `<worktree>/.claude/settings.local.json`, if the source exists. Used by
/// both the fresh-worktree flow (`setup_script`) and the time-travel flow
/// (`replace` / `fork` spawn into a pre-existing worktree) so both inherit
/// the project's local Claude settings.
pub fn copy_settings_snippet(project_dir: &Path, worktree_path: &Path) -> String {
    let dir_str = project_dir.to_string_lossy();
    let wt_str = worktree_path.to_string_lossy();
    format!(
        "if [ -f {src} ]; then mkdir -p {dst_dir} && cp {src} {dst}; fi",
        src = shell_quote(&format!("{dir_str}/.claude/settings.local.json")),
        dst_dir = shell_quote(&format!("{wt_str}/.claude")),
        dst = shell_quote(&format!("{wt_str}/.claude/settings.local.json")),
    )
}

/// Build a shell script that creates a git worktree, copies
/// `.claude/settings.local.json`, and `cd`s into it. Returns the script
/// and the worktree path.
pub fn setup_script(
    project_dir: &Path,
    worktree_location: &str,
    branch: Option<&str>,
    base: Option<&str>,
) -> (String, PathBuf) {
    let name = worktree_name(branch);
    let wt_path = self::worktree_path(project_dir, worktree_location, &name);
    let wt_str = wt_path.to_string_lossy();

    let git_add = if branch.is_some_and(|b| !b.is_empty()) {
        let name_q = shell_quote(&name);
        let wt_q = shell_quote(&wt_str);
        let mut new_branch_cmd =
            format!("git worktree add -b {name_q} {wt_q}");
        if let Some(b) = base {
            new_branch_cmd.push(' ');
            new_branch_cmd.push_str(&shell_quote(b));
        }
        let existing_cmd = format!("git worktree add {wt_q} {name_q}");
        format!(
            "if git show-ref --verify --quiet refs/heads/{name_q}; \
             then {existing_cmd}; else {new_branch_cmd}; fi"
        )
    } else {
        format!("git worktree add -d {}", shell_quote(&wt_str))
    };

    let copy_settings = copy_settings_snippet(project_dir, &wt_path);

    let script = format!(
        "{git_add} && {copy_settings} && cd {}",
        shell_quote(&wt_str),
    );
    (script, wt_path)
}

/// Directory that holds Bazel output bases: `_bazel_$USER` under the platform cache directory.
fn bazel_cache_root() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    let user = std::env::var_os("USER")?;
    let cache = if cfg!(target_os = "macos") {
        home.join("Library/Caches/bazel")
    } else {
        home.join(".cache/bazel")
    };
    Some(cache.join(format!("_bazel_{}", user.to_string_lossy())))
}

/// Find the Bazel output base of a worktree. Bazel names the base after the md5 of the workspace
/// path, and writes that path to `DO_NOT_BUILD_HERE`. Return the base only if the file confirms it.
fn bazel_output_base(cache_root: &Path, worktree_path: &Path) -> Option<PathBuf> {
    let base = cache_root.join(format!("{:x}", md5::compute(worktree_path.to_string_lossy().as_bytes())));
    let owner = std::fs::read_to_string(base.join("DO_NOT_BUILD_HERE")).ok()?;
    (Path::new(owner.trim()) == worktree_path).then_some(base)
}

/// Run a command, and kill it if it runs longer than `timeout`. Return whether it exited with success.
fn run_with_timeout(cmd: &mut std::process::Command, timeout: std::time::Duration) -> bool {
    let Ok(mut child) = cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    else {
        return false;
    };
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

/// Delete a Bazel output base. Bazel makes its external files read-only, so make them writable first.
fn remove_output_base(base: &Path) -> std::io::Result<()> {
    run_with_timeout(
        std::process::Command::new("chmod").args(["-R", "u+w"]).arg(base),
        std::time::Duration::from_secs(60),
    );
    std::fs::remove_dir_all(base)
}

/// Stop the Bazel server and delete the output base of a worktree, if it has one. Best effort:
/// failures are logged and never propagate.
fn teardown_bazel_output_base(cache_root: &Path, worktree_path: &Path) {
    let canonical = worktree_path.canonicalize().ok();
    let base = bazel_output_base(cache_root, worktree_path)
        .or_else(|| canonical.and_then(|c| bazel_output_base(cache_root, &c)));
    let Some(base) = base else { return };
    let stopped = run_with_timeout(
        std::process::Command::new("bazel")
            .arg(format!("--output_base={}", base.display()))
            .arg("shutdown"),
        std::time::Duration::from_secs(10),
    );
    if !stopped {
        eprintln!("bazel shutdown did not finish for {}", base.display());
    }
    if let Err(e) = remove_output_base(&base) {
        eprintln!("failed to remove bazel output base {}: {e}", base.display());
    }
}

/// Remove a git worktree previously created by [`create`], and its Bazel output base.
pub fn remove(project_dir: &Path, worktree_path: &Path) {
    if let Some(cache_root) = bazel_cache_root() {
        teardown_bazel_output_base(&cache_root, worktree_path);
    }
    let _ = std::process::Command::new("git")
        .args(["worktree", "remove", "--force", &worktree_path.to_string_lossy()])
        .current_dir(project_dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("mandelbot-wt-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn make_base(root: &Path, wt: &Path, owner: &str) -> PathBuf {
        let base = root.join(format!("{:x}", md5::compute(wt.to_string_lossy().as_bytes())));
        std::fs::create_dir_all(base.join("external/dep")).unwrap();
        std::fs::write(base.join("DO_NOT_BUILD_HERE"), owner).unwrap();
        std::fs::write(base.join("external/dep/file"), "x").unwrap();
        std::fs::set_permissions(base.join("external/dep/file"), std::fs::Permissions::from_mode(0o444)).unwrap();
        std::fs::set_permissions(base.join("external/dep"), std::fs::Permissions::from_mode(0o555)).unwrap();
        base
    }

    #[test]
    fn md5_matches_bazel_naming() {
        // Derived from a real base: `md5(DO_NOT_BUILD_HERE contents)` is the directory name.
        let wt = "/Users/phil/.mandelbot/worktrees/watch-pr-always-exit-conflict";
        assert_eq!(format!("{:x}", md5::compute(wt.as_bytes())), "168854541fdd969c8491c42ad8b39824");
    }

    #[test]
    fn removes_read_only_base_and_keeps_cache_and_install() {
        let root = temp_root("remove");
        let wt = PathBuf::from("/nonexistent/wt-a");
        let base = make_base(&root, &wt, "/nonexistent/wt-a");
        std::fs::create_dir_all(root.join("cache")).unwrap();
        std::fs::create_dir_all(root.join("install")).unwrap();
        teardown_bazel_output_base(&root, &wt);
        assert!(!base.exists());
        assert!(root.join("cache").exists());
        assert!(root.join("install").exists());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn keeps_base_owned_by_another_path() {
        let root = temp_root("mismatch");
        let wt = PathBuf::from("/nonexistent/wt-b");
        let base = make_base(&root, &wt, "/nonexistent/other");
        teardown_bazel_output_base(&root, &wt);
        assert!(base.exists());
        std::fs::set_permissions(base.join("external/dep"), std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn missing_base_is_a_no_op() {
        let root = temp_root("missing");
        teardown_bazel_output_base(&root, Path::new("/nonexistent/wt-c"));
        assert!(root.exists());
        std::fs::remove_dir_all(&root).unwrap();
    }
}

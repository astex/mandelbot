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

/// Create a git worktree under `worktree_location/` (relative to
/// `project_dir` if not absolute) and return the absolute path to the new
/// worktree directory. When `branch` is provided it is used as both the
/// directory name and the new branch name; otherwise a random name is
/// generated with a detached HEAD.
pub fn create(
    project_dir: &Path,
    worktree_location: &str,
    branch: Option<&str>,
) -> Option<PathBuf> {
    let name = worktree_name(branch);
    let worktree_path = self::worktree_path(project_dir, worktree_location, &name);
    let path_str = worktree_path.to_string_lossy().into_owned();

    let status = if branch.is_some_and(|b| !b.is_empty()) {
        // Create a new branch with the given name.
        std::process::Command::new("git")
            .args(["worktree", "add", "-b", &name, &path_str])
            .current_dir(project_dir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .status()
    } else {
        // Detached HEAD.
        std::process::Command::new("git")
            .args(["worktree", "add", "-d", &path_str])
            .current_dir(project_dir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .status()
    };

    match status {
        Ok(s) if s.success() => {
            // Copy the project's .claude/settings.local.json into the
            // worktree so that MCP server trust carries over.
            let src = project_dir.join(".claude").join("settings.local.json");
            if src.exists() {
                let dst_dir = worktree_path.join(".claude");
                let _ = std::fs::create_dir_all(&dst_dir);
                let _ = std::fs::copy(&src, dst_dir.join("settings.local.json"));
            }
            Some(worktree_path)
        }
        _ => None,
    }
}

/// Remove a git worktree previously created by [`create`].
pub fn remove(project_dir: &Path, worktree_path: &Path) {
    let _ = std::process::Command::new("git")
        .args(["worktree", "remove", "--force", &worktree_path.to_string_lossy()])
        .current_dir(project_dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

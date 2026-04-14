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
    let dir_str = project_dir.to_string_lossy();

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

    let copy_settings = format!(
        "if [ -f {src} ]; then mkdir -p {dst_dir} && cp {src} {dst}; fi",
        src = shell_quote(&format!("{dir_str}/.claude/settings.local.json")),
        dst_dir = shell_quote(&format!("{wt_str}/.claude")),
        dst = shell_quote(&format!("{wt_str}/.claude/settings.local.json")),
    );

    let script = format!(
        "{git_add} && {copy_settings} && cd {}",
        shell_quote(&wt_str),
    );
    (script, wt_path)
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

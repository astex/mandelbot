use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use fontdb::Database;
use iced::{Font, keyboard};
use serde::Deserialize;

use crate::theme::{self, TerminalTheme};

fn default_theme() -> String {
    "dark".to_string()
}

fn default_font() -> String {
    "monospace".to_string()
}

fn default_font_size() -> f32 {
    14.0
}

fn default_control_prefix() -> String {
    "ctrl+shift".to_string()
}

fn default_movement_prefix() -> String {
    "alt+shift".to_string()
}

fn default_line_height() -> f32 {
    1.3
}

pub fn default_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string())
}

/// Shells known to accept `-l -i -c <command>`, which is how mandelbot
/// launches claude inside the configured shell.
const POSIX_SHELLS: &[&str] = &[
    "sh", "bash", "zsh", "dash", "ash", "ksh", "ksh88", "ksh93", "mksh",
    "pdksh", "yash", "busybox",
];

/// Real shells that do *not* honor `-l -i -c <command>` the same way.
const NON_POSIX_SHELLS: &[&str] = &[
    "fish", "csh", "tcsh", "rc", "nu", "xonsh", "elvish", "pwsh",
    "powershell",
];

/// Extensions that mark the configured `shell` as a script rather than
/// a shell binary.
const SCRIPT_EXTENSIONS: &[&str] = &[
    "sh", "bash", "zsh", "fish", "py", "rb", "pl", "js", "ts", "exp",
];

/// Every warning ends with this so the user knows what to edit without
/// reading mandelbot's source.
const CONFIG_HINT: &str = "Set \"shell\" in ~/.mandelbot/config.json to \
     a POSIX shell such as /bin/zsh.";

/// Why a non-shell `shell` breaks claude tabs specifically.
const SCRIPT_EFFECT: &str = "Scripts ignore the `-l -i -c <command>` \
     arguments mandelbot starts claude with, so claude tabs run the \
     script's own commands and open in the wrong directory.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellVerdict {
    /// Looks like a shell mandelbot can drive.
    Ok,
    /// Nothing to run at all; the caller falls back to `default_shell`.
    Empty,
    /// Usable as a command, but probably not as mandelbot's shell.
    Suspect(String),
}

/// Classify a configured `shell` value.
///
/// `shebang` is the first line of the shell's file when it could be
/// read, so the check stays a pure function of its inputs. A shebang is
/// decisive: a file with one is a script, and a script silently drops
/// the `-l -i -c <command>` arguments mandelbot passes it — which is
/// exactly how a wrapper script ends up running claude in its own
/// hardcoded directory instead of the tab's.
pub fn check_shell(shell: &str, shebang: Option<&str>) -> ShellVerdict {
    let Some(command) = shell.split_whitespace().next() else {
        return ShellVerdict::Empty;
    };
    let name = command.rsplit('/').next().unwrap_or(command);

    if let Some(line) = shebang
        && let Some(interpreter) = shebang_interpreter(line)
    {
        return ShellVerdict::Suspect(format!(
            "shell \"{command}\" is a script (its shebang runs \
             {interpreter}), not a shell binary. {SCRIPT_EFFECT} \
             {CONFIG_HINT}"
        ));
    }

    if POSIX_SHELLS.contains(&name) {
        return ShellVerdict::Ok;
    }

    if NON_POSIX_SHELLS.contains(&name) {
        return ShellVerdict::Suspect(format!(
            "shell \"{command}\" does not accept the \
             `-l -i -c <command>` arguments mandelbot starts claude \
             with, so claude tabs may open in the wrong directory. \
             {CONFIG_HINT}"
        ));
    }

    if let Some((_, extension)) = name.rsplit_once('.')
        && SCRIPT_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
    {
        return ShellVerdict::Suspect(format!(
            "shell \"{command}\" looks like a script, not a shell \
             binary. {SCRIPT_EFFECT} {CONFIG_HINT}"
        ));
    }

    ShellVerdict::Suspect(format!(
        "shell \"{command}\" is not a recognized POSIX shell. If it is \
         a wrapper script, claude tabs will open in the wrong \
         directory. {CONFIG_HINT}"
    ))
}

/// Extract the interpreter from a shebang line, if the line is one.
fn shebang_interpreter(line: &str) -> Option<String> {
    let rest = line.strip_prefix("#!")?.trim();
    if rest.is_empty() {
        return None;
    }
    let mut words = rest.split_whitespace();
    let first = words.next()?;
    // `#!/usr/bin/env bash` names the real interpreter in word two.
    let interpreter =
        if first.rsplit('/').next() == Some("env") {
            words.next().unwrap_or(first)
        } else {
            first
        };
    Some(interpreter.to_string())
}

/// Read the first line of `path`, for shebang detection. Returns `None`
/// for binaries, unreadable files, and bare names resolved via `PATH`.
fn read_shebang(command: &str) -> Option<String> {
    use std::io::Read;

    if !command.contains('/') {
        return None;
    }
    let mut buf = [0u8; 128];
    let mut file = fs::File::open(command).ok()?;
    let read = file.read(&mut buf).ok()?;
    let head = std::str::from_utf8(&buf[..read]).ok()?;
    Some(head.lines().next().unwrap_or("").to_string())
}

/// Classify the configured `shell`, reading its shebang when possible.
pub fn validate_shell(shell: &str) -> ShellVerdict {
    let shebang = shell
        .split_whitespace()
        .next()
        .and_then(read_shebang);
    check_shell(shell, shebang.as_deref())
}

fn default_workflow() -> String {
    "detect".to_string()
}

fn default_worktree_location() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join(".mandelbot")
        .join("worktrees")
        .to_string_lossy()
        .into_owned()
}

fn default_home_model() -> String {
    "haiku".to_string()
}

fn default_project_model() -> String {
    "sonnet".to_string()
}

fn default_task_model() -> String {
    "opus".to_string()
}

fn default_auto_checkpoint() -> bool {
    true
}

#[derive(Deserialize)]
pub struct Models {
    #[serde(default = "default_home_model")]
    pub home: String,

    #[serde(default = "default_project_model")]
    pub project: String,

    #[serde(default = "default_task_model")]
    pub task: String,
}

impl Default for Models {
    fn default() -> Self {
        Self {
            home: default_home_model(),
            project: default_project_model(),
            task: default_task_model(),
        }
    }
}

#[derive(Deserialize)]
pub struct Config {
    #[serde(default = "default_theme")]
    pub theme: String,

    #[serde(default = "default_font")]
    pub font: String,

    #[serde(default = "default_font_size")]
    pub font_size: f32,

    #[serde(default = "default_line_height")]
    pub line_height: f32,

    #[serde(default = "default_control_prefix")]
    pub control_prefix: String,

    #[serde(default = "default_movement_prefix")]
    pub movement_prefix: String,

    #[serde(default = "default_shell")]
    pub shell: String,

    #[serde(default = "default_workflow")]
    pub workflow: String,

    #[serde(default = "default_worktree_location")]
    pub worktree_location: String,

    #[serde(default)]
    pub models: Models,

    #[serde(default = "default_auto_checkpoint")]
    pub auto_checkpoint: bool,

    /// Set by `load` when `shell` looks like something mandelbot cannot
    /// drive. Surfaced to the user as a toast at startup.
    #[serde(skip)]
    pub shell_warning: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: default_theme(),
            font: default_font(),
            font_size: default_font_size(),
            line_height: default_line_height(),
            control_prefix: default_control_prefix(),
            movement_prefix: default_movement_prefix(),
            shell: default_shell(),
            workflow: default_workflow(),
            worktree_location: default_worktree_location(),
            models: Models::default(),
            auto_checkpoint: default_auto_checkpoint(),
            shell_warning: None,
        }
    }
}

fn parse_modifiers(prefix: &str) -> keyboard::Modifiers {
    let mut mods = keyboard::Modifiers::empty();
    for part in prefix.split('+') {
        match part.trim().to_lowercase().as_str() {
            "ctrl" | "control" => mods |= keyboard::Modifiers::CTRL,
            "shift" => mods |= keyboard::Modifiers::SHIFT,
            "alt" => mods |= keyboard::Modifiers::ALT,
            "super" | "logo" | "cmd" | "meta" => mods |= keyboard::Modifiers::LOGO,
            _ => {}
        }
    }
    mods
}

static CHAR_WIDTH: OnceLock<f32> = OnceLock::new();

/// Query the advance width of '0' from the system font via fontdb.
fn query_char_width(font_name: &str, font_size: f32) -> f32 {
    let mut db = Database::new();
    db.load_system_fonts();

    let query = fontdb::Query {
        families: &[fontdb::Family::Name(font_name)],
        ..fontdb::Query::default()
    };

    db.query(&query)
        .and_then(|id| {
            db.with_face_data(id, |data, face_index| {
                let face = ttf_parser::Face::parse(data, face_index).ok()?;
                let scale = font_size / face.units_per_em() as f32;
                let glyph = face.glyph_index('0')?;
                let advance = face.glyph_hor_advance(glyph)? as f32;
                Some(advance * scale)
            })
        })
        .flatten()
        .unwrap_or(font_size * 0.6)
}

impl Config {
    pub fn char_width(&self) -> f32 {
        *CHAR_WIDTH.get_or_init(|| {
            // Round to the nearest even integer so half-cell splits in block
            // characters land on whole pixels.
            let raw = query_char_width(&self.font, self.font_size);
            (raw / 2.0).round() * 2.0
        })
    }

    pub fn font(&self) -> Font {
        static FONT_NAME: OnceLock<String> = OnceLock::new();
        let name = FONT_NAME.get_or_init(|| self.font.clone());
        if name == "monospace" {
            Font::MONOSPACE
        } else {
            Font {
                family: iced::font::Family::Name(name.as_str()),
                ..Font::MONOSPACE
            }
        }
    }

    pub fn char_height(&self) -> f32 {
        // Round to the nearest even integer so half-cell splits in block
        // characters land on whole pixels.
        let raw = self.font_size * self.line_height;
        (raw / 2.0).round() * 2.0
    }

    pub fn terminal_theme(&self) -> TerminalTheme {
        match self.theme.as_str() {
            "light" => theme::solarized_light(),
            _ => theme::solarized_dark(),
        }
    }

    pub fn control_modifiers(&self) -> keyboard::Modifiers {
        parse_modifiers(&self.control_prefix)
    }

    pub fn movement_modifiers(&self) -> keyboard::Modifiers {
        parse_modifiers(&self.movement_prefix)
    }

    pub fn matches_control(&self, modifiers: keyboard::Modifiers) -> bool {
        let expected = self.control_modifiers();
        modifiers & expected == expected
    }

    pub fn matches_movement(&self, modifiers: keyboard::Modifiers) -> bool {
        let expected = self.movement_modifiers();
        modifiers & expected == expected
    }

    pub fn load() -> Self {
        let mut config = Self::read();
        config.check_shell();
        config
    }

    fn read() -> Self {
        if let Ok(json) = std::env::var("MANDELBOT_CONFIG") {
            return serde_json::from_str(&json)
                .expect("MANDELBOT_CONFIG contains invalid JSON");
        }

        let path = config_path();
        match fs::read_to_string(&path) {
            Ok(contents) => serde_json::from_str(&contents)
                .unwrap_or_else(|e| panic!("{}: invalid JSON: {e}", path.display())),
            Err(_) => Self::default(),
        }
    }

    /// Record a warning when `shell` is not something mandelbot can
    /// drive, and fall back to the default shell when it is empty.
    /// Never refuses to start: an unusual-but-working shell should
    /// still boot, just loudly.
    fn check_shell(&mut self) {
        let warning = match validate_shell(&self.shell) {
            ShellVerdict::Ok => return,
            ShellVerdict::Empty => {
                let fallback = default_shell();
                self.shell = fallback.clone();
                format!(
                    "config \"shell\" is empty; falling back to \
                     {fallback}."
                )
            }
            ShellVerdict::Suspect(reason) => reason,
        };
        self.shell_warning = Some(warning);
    }
}

fn config_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".mandelbot").join("config.json")
}

/// Find font bytes for all style variants (regular, italic, bold, bold-italic)
/// of a family name using fontdb.
pub fn find_font_variants(name: &str) -> Vec<Vec<u8>> {
    let mut db = Database::new();
    db.load_system_fonts();

    let monospace_family;
    let families: &[fontdb::Family<'_>] = if name == "monospace" {
        // Resolve the system monospace font so we can load all its variants.
        let query = fontdb::Query {
            families: &[fontdb::Family::Monospace],
            ..fontdb::Query::default()
        };
        match db.query(&query) {
            Some(id) => match db.face(id) {
                Some(info) => {
                    for family in &info.families {
                        if !family.0.is_empty() {
                            monospace_family = family.0.clone();
                            return find_font_variants_inner(&db, &monospace_family);
                        }
                    }
                    return Vec::new();
                }
                None => return Vec::new(),
            },
            None => return Vec::new(),
        }
    } else {
        &[fontdb::Family::Name(name)]
    };

    find_font_variants_from_families(&db, families)
}

fn find_font_variants_inner(db: &Database, name: &str) -> Vec<Vec<u8>> {
    find_font_variants_from_families(db, &[fontdb::Family::Name(name)])
}

fn find_font_variants_from_families(db: &Database, families: &[fontdb::Family<'_>]) -> Vec<Vec<u8>> {
    let styles = [
        (fontdb::Weight::NORMAL, fontdb::Style::Normal),
        (fontdb::Weight::NORMAL, fontdb::Style::Italic),
        (fontdb::Weight::BOLD, fontdb::Style::Normal),
        (fontdb::Weight::BOLD, fontdb::Style::Italic),
    ];

    let mut results = Vec::new();
    let mut seen_ids = Vec::new();
    for (weight, style) in styles {
        let query = fontdb::Query {
            families,
            weight,
            style,
            ..fontdb::Query::default()
        };
        if let Some(id) = db.query(&query) {
            if !seen_ids.contains(&id) {
                seen_ids.push(id);
                if let Some(data) = db.with_face_data(id, |data, _| data.to_vec()) {
                    results.push(data);
                }
            }
        }
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    fn suspect_reason(verdict: ShellVerdict) -> String {
        match verdict {
            ShellVerdict::Suspect(reason) => reason,
            other => panic!("expected Suspect, got {other:?}"),
        }
    }

    #[test]
    fn known_shells_are_ok() {
        for shell in
            ["/bin/bash", "/bin/zsh", "/bin/sh", "/usr/bin/dash", "zsh"]
        {
            assert_eq!(
                check_shell(shell, None),
                ShellVerdict::Ok,
                "{shell}",
            );
        }
    }

    #[test]
    fn unusual_path_to_a_known_shell_is_ok() {
        assert_eq!(
            check_shell("/opt/homebrew/bin/bash", None),
            ShellVerdict::Ok,
        );
        assert_eq!(
            check_shell("/nix/store/abc123-bash-5.2/bin/bash", None),
            ShellVerdict::Ok,
        );
    }

    #[test]
    fn arguments_after_the_shell_are_ignored() {
        assert_eq!(
            check_shell("/bin/bash --norc", None),
            ShellVerdict::Ok,
        );
    }

    #[test]
    fn empty_shell_is_empty() {
        assert_eq!(check_shell("", None), ShellVerdict::Empty);
        assert_eq!(check_shell("   \t ", None), ShellVerdict::Empty);
    }

    #[test]
    fn shebang_marks_a_wrapper_script() {
        let reason = suspect_reason(check_shell(
            "/Users/someone/bin/wrapper",
            Some("#!/bin/bash"),
        ));
        assert!(reason.contains("is a script"), "{reason}");
        assert!(reason.contains("-l -i -c"), "{reason}");
        assert!(reason.contains("wrapper"), "{reason}");
    }

    #[test]
    fn shebang_wins_over_a_shell_like_name() {
        // A file named `bash` that is really a script still drops the
        // arguments mandelbot passes it.
        assert!(matches!(
            check_shell("/Users/someone/bin/bash", Some("#!/bin/sh")),
            ShellVerdict::Suspect(_),
        ));
    }

    #[test]
    fn a_binary_first_line_is_not_a_shebang() {
        assert_eq!(
            check_shell("/bin/bash", Some("\u{7f}ELF\u{2}\u{1}\u{1}")),
            ShellVerdict::Ok,
        );
    }

    #[test]
    fn script_extension_is_suspect_without_a_shebang() {
        let reason = suspect_reason(check_shell(
            "/Users/someone/bin/claude-shell.sh",
            None,
        ));
        assert!(reason.contains("looks like a script"), "{reason}");
        assert!(reason.contains("claude-shell.sh"), "{reason}");
    }

    #[test]
    fn script_extension_matches_case_insensitively() {
        assert!(matches!(
            check_shell("/Users/someone/bin/Wrapper.SH", None),
            ShellVerdict::Suspect(_),
        ));
    }

    #[test]
    fn fish_is_flagged_as_a_real_but_incompatible_shell() {
        let reason =
            suspect_reason(check_shell("/opt/homebrew/bin/fish", None));
        assert!(reason.contains("-l -i -c"), "{reason}");
        assert!(!reason.contains("script"), "{reason}");
    }

    #[test]
    fn unrecognized_names_are_flagged_but_not_called_scripts() {
        let reason = suspect_reason(check_shell("/usr/bin/mystery", None));
        assert!(
            reason.contains("not a recognized POSIX shell"),
            "{reason}",
        );
    }

    #[test]
    fn every_warning_names_the_config_key() {
        for shell in [
            "/usr/bin/fish",
            "/Users/someone/bin/wrapper.sh",
            "/usr/bin/mystery",
        ] {
            let reason = suspect_reason(check_shell(shell, None));
            assert!(reason.contains("~/.mandelbot/config.json"), "{shell}");
        }
    }

    #[test]
    fn env_shebang_reports_the_real_interpreter() {
        assert_eq!(
            shebang_interpreter("#!/usr/bin/env bash"),
            Some("bash".to_string()),
        );
        assert_eq!(
            shebang_interpreter("#!/bin/zsh -f"),
            Some("/bin/zsh".to_string()),
        );
        assert_eq!(shebang_interpreter("#!"), None);
        assert_eq!(shebang_interpreter("not a shebang"), None);
    }
}

use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use fontdb::Database;
use iced::keyboard;
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

fn default_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string())
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

fn default_checkpoint_retention() -> usize {
    100
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

    #[serde(default = "default_checkpoint_retention")]
    pub checkpoint_retention: usize,
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
            checkpoint_retention: default_checkpoint_retention(),
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

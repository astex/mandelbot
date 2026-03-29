use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

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

/// Query the advance width of '0' from the font file via ttf-parser.
fn query_char_width(font_name: &str, font_size: f32) -> f32 {
    resolve_font_path(font_name)
        .and_then(|path| {
            let data = fs::read(&path).ok()?;
            let face = ttf_parser::Face::parse(&data, 0).ok()?;
            let scale = font_size / face.units_per_em() as f32;
            let glyph = face.glyph_index('0')?;
            let advance = face.glyph_hor_advance(glyph)? as f32;
            Some(advance * scale)
        })
        .unwrap_or(font_size * 0.6)
}

/// Use fontconfig (Linux) or font directories (macOS) to find the font file.
fn resolve_font_path(font_name: &str) -> Option<PathBuf> {
    if let Ok(output) = std::process::Command::new("fc-match")
        .arg(font_name)
        .arg("-f")
        .arg("%{file}")
        .output()
    {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Some(PathBuf::from(path));
            }
        }
    }

    find_font_bytes(font_name).and_then(|_| {
        let home = std::env::var("HOME").unwrap_or_default();
        let dirs = [
            format!("{home}/Library/Fonts"),
            "/Library/Fonts".into(),
            "/System/Library/Fonts".into(),
            "/System/Library/Fonts/Supplemental".into(),
            "/usr/share/fonts".into(),
        ];
        let lower = font_name.to_lowercase();
        for dir in &dirs {
            if let Ok(entries) = fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                    if stem.to_lowercase() == lower {
                        return Some(path);
                    }
                }
            }
        }
        None
    })
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

/// Search macOS font directories for a font file matching the given name.
pub fn find_font_bytes(name: &str) -> Option<Vec<u8>> {
    if name == "monospace" {
        return None;
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let dirs = [
        format!("{home}/Library/Fonts"),
        "/Library/Fonts".into(),
        "/System/Library/Fonts".into(),
        "/System/Library/Fonts/Supplemental".into(),
    ];
    let lower = name.to_lowercase();
    for dir in &dirs {
        let Ok(entries) = fs::read_dir(dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            if stem.to_lowercase() == lower {
                if let Ok(bytes) = fs::read(&path) {
                    return Some(bytes);
                }
            }
        }
    }
    None
}

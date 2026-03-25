use std::fs;
use std::path::PathBuf;

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
    "alt".to_string()
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

const LINE_HEIGHT: f32 = 1.3;

impl Config {
    pub fn char_width(&self) -> f32 {
        self.font_size * 0.6
    }

    pub fn char_height(&self) -> f32 {
        self.font_size * LINE_HEIGHT
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

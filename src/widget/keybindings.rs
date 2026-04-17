use iced::keyboard;

use crate::config::Config;
use crate::ui::Message;

/// Context passed to keybinding resolution so bindings can reference
/// the active tab (e.g. for `CloseTab`).
pub struct KeyContext {
    pub active_tab_id: usize,
}

/// Try to map a key press to a global message (control-prefix and
/// movement-prefix bindings). Returns `Some(msg)` if the key matched.
pub fn keybinding_message(
    config: &Config,
    key: &keyboard::Key,
    modifiers: keyboard::Modifiers,
    ctx: &KeyContext,
) -> Option<Message> {
    if config.matches_control(modifiers) {
        if let Some(msg) = control_key_message(key, ctx) {
            return Some(msg);
        }
    }

    if config.matches_movement(modifiers) {
        if let Some(msg) = movement_key_message(key) {
            return Some(msg);
        }
    }

    None
}

fn control_key_message(key: &keyboard::Key, ctx: &KeyContext) -> Option<Message> {
    use keyboard::key::Named;
    match key {
        keyboard::Key::Character(c) if c.as_ref() == "t" => Some(Message::NewTab),
        keyboard::Key::Named(Named::Space) => Some(Message::SpawnAgent),
        keyboard::Key::Named(Named::ArrowDown) => Some(Message::SpawnAgent),
        keyboard::Key::Named(Named::ArrowRight) => Some(Message::SpawnChild),
        keyboard::Key::Character(c) if c.as_ref() == "w" => Some(Message::CloseTab(ctx.active_tab_id)),
        keyboard::Key::Character(c) if c.as_ref() == "f" => Some(Message::ToggleFoldTab(ctx.active_tab_id)),
        keyboard::Key::Character(c) if c.as_ref() == "h" => Some(Message::ToggleTimeline(ctx.active_tab_id)),
        _ => None,
    }
}

fn movement_key_message(key: &keyboard::Key) -> Option<Message> {
    use keyboard::key::Named;
    match key {
        keyboard::Key::Named(Named::Space) => Some(Message::NextIdle),
        keyboard::Key::Named(Named::ArrowDown) => Some(Message::NavigateSibling(1)),
        keyboard::Key::Named(Named::ArrowUp) => Some(Message::NavigateSibling(-1)),
        keyboard::Key::Named(Named::ArrowRight) => Some(Message::NavigateRank(1)),
        keyboard::Key::Named(Named::ArrowLeft) => Some(Message::NavigateRank(-1)),
        keyboard::Key::Character(c) if c.as_ref() == "-" => Some(Message::FocusPreviousTab),
        keyboard::Key::Character(c) => {
            c.as_ref().parse::<usize>().ok()
                .filter(|&d| (0..=9).contains(&d))
                .map(Message::SelectTabByIndex)
        }
        _ => None,
    }
}

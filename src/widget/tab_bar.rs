//! Left-hand tab bar: agent tree (Home → Projects → Tasks), shell tabs,
//! and any toasts anchored to the bottom.

use std::collections::{HashMap, HashSet};

use iced::widget::{button, column, container, mouse_area, row, text, Space};
use iced::{Alignment, Border, Color, Element, Fill, Font, Theme};

use crate::animation::FlashState;
use crate::config::Config;
use crate::tab::{AgentRank, AgentStatus, TerminalTab};
use crate::theme::TerminalTheme;
use crate::ui::{Message, PADDING, TAB_BAR_WIDTH, TAB_GROUP_GAP};

pub fn view<'a>(
    tabs: &'a [TerminalTab],
    active_tab_id: usize,
    display_order: &[usize],
    number_assignments: &HashMap<usize, usize>,
    bell_flashes: &'a FlashState,
    folded_tabs: &'a HashSet<usize>,
    terminal_theme: &'a TerminalTheme,
    config: &'a Config,
    toast_elements: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    let active_bg = terminal_theme.bg;
    let inactive_bg = terminal_theme.black;
    let fg = terminal_theme.fg;

    let has_claude_children = |parent_id: usize| -> bool {
        tabs.iter().any(|t| t.parent_id == Some(parent_id) && t.is_claude)
    };

    let tab_button = |tab: &TerminalTab, display_number: Option<usize>, indent: f32| {
        let is_active = tab.id == active_tab_id;
        let tab_id = tab.id;
        let is_foldable = tab.is_claude && tab.rank != AgentRank::Home;
        let has_children = is_foldable && has_claude_children(tab_id);
        let is_folded = folded_tabs.contains(&tab_id);

        let base_bg = if is_active { active_bg } else { inactive_bg };
        let bg = bell_flashes.blend(tab_id, base_bg, terminal_theme.yellow);

        let cw = config.char_width();
        let avail = TAB_BAR_WIDTH - indent - PADDING * 2.0 - cw * 3.0;
        let max_label_chars = (avail / cw) as usize;

        let label_text: String = if tab.is_pending() {
            "new project...".into()
        } else if let Some(title) = &tab.title {
            if !tab.is_claude {
                format_shell_title(title, max_label_chars)
            } else {
                title.clone()
            }
        } else if tab.rank == AgentRank::Project {
            if let Some(dir) = &tab.project_dir {
                dir.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| dir.to_string_lossy().into_owned())
            } else {
                String::new()
            }
        } else if !tab.is_claude {
            "shell".into()
        } else {
            String::new()
        };

        let number_text = match display_number {
            Some(n) => format!("{n}"),
            None => " ".into(),
        };

        let label_len = label_text.len();
        let label = text(label_text)
            .size(config.font_size)
            .font(Font::MONOSPACE)
            .color(fg);
        let number = text(number_text)
            .size(config.font_size)
            .font(Font::MONOSPACE)
            .color(fg);

        let label = container(label).width(Fill).clip(true);

        let toggle_slot_width = config.char_width() + 8.0;
        let toggle: Element<'_, Message> = if has_children {
            let icon = if is_folded { "+" } else { "-" };
            let icon_text = text(icon)
                .size(config.font_size)
                .font(Font::MONOSPACE)
                .color(fg);
            let icon_container = container(icon_text)
                .width(toggle_slot_width)
                .align_x(Alignment::Center);
            mouse_area(icon_container)
                .on_press(Message::ToggleFoldTab(tab_id))
                .into()
        } else if is_foldable {
            Space::new().width(toggle_slot_width).into()
        } else {
            Space::new().width(0).into()
        };

        const SUFFIX_SPACING: f32 = 6.0;
        let mut suffix = row![]
            .align_y(Alignment::Center)
            .spacing(SUFFIX_SPACING);
        if label_len + 2 >= max_label_chars {
            let muted_fg = Color { a: 0.4, ..fg };
            suffix = suffix.push(
                text("|")
                    .size(config.font_size)
                    .font(Font::MONOSPACE)
                    .color(muted_fg),
            );
        }

        if tab.is_claude && tab.pr_number().is_some() {
            let muted_fg = Color { a: 0.7, ..fg };
            let pr_icon = text("⎇")
                .size(config.font_size)
                .font(Font::MONOSPACE)
                .color(muted_fg);
            let pr_btn = button(pr_icon)
                .on_press(Message::OpenPr(tab_id))
                .padding(0)
                .style(move |_theme, _status| button::Style {
                    background: Some(bg.into()),
                    border: Border::default(),
                    ..Default::default()
                });
            suffix = suffix.push(pr_btn);
        }
        if tab.is_claude && tab.background_tasks > 0 {
            let bg_label = format!("+{}", tab.background_tasks);
            suffix = suffix.push(
                text(bg_label)
                    .size(config.font_size * 0.75)
                    .font(Font::MONOSPACE)
                    .color(terminal_theme.cyan),
            );
        }
        if tab.is_claude {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            let has_pending_wakeup = matches!(
                tab.next_wakeup_at_ms,
                Some(t) if t > now_ms,
            );
            if has_pending_wakeup {
                suffix = suffix.push(
                    text("⏱")
                        .size(config.font_size * 0.75)
                        .color(terminal_theme.cyan),
                );
            }
        }
        {
            let dot_size = config.font_size * 0.6;
            let dot_char = if tab.status == AgentStatus::Idle { "○" } else { "●" };
            let dot_color = status_dot_color(tab.status, fg);
            suffix = suffix.push(text(dot_char).size(dot_size).color(dot_color));
        }
        let suffix = suffix.push(number);

        let toggle_gap = if is_foldable { PADDING } else { 0.0 };
        let content = row![
            Space::new().width(PADDING),
            toggle,
            Space::new().width(toggle_gap),
            label,
            suffix,
            Space::new().width(PADDING),
        ]
            .align_y(Alignment::Center);

        let styled = container(content)
            .width(TAB_BAR_WIDTH - indent)
            .padding([5, 10])
            .style(move |_theme: &Theme| container::Style {
                background: Some(bg.into()),
                border: Border::default(),
                ..Default::default()
            });

        let tab_elem: Element<'_, Message> = mouse_area(styled)
            .on_press(Message::SelectTab(tab_id))
            .into();

        if indent > 0.0 {
            row![Space::new().width(indent), tab_elem].width(TAB_BAR_WIDTH).into()
        } else {
            tab_elem
        }
    };

    let has_agents = tabs.iter().any(|t| t.is_claude);
    let show_separators = tabs.len() > 1;

    let separator = || -> Element<'_, Message> {
        let muted = Color { a: 0.25, ..fg };
        container(Space::new())
            .width(TAB_BAR_WIDTH)
            .height(1)
            .style(move |_theme: &Theme| container::Style {
                background: Some(muted.into()),
                ..Default::default()
            })
            .into()
    };

    let indent_step = 20.0_f32;

    let mut tab_col = column![];
    tab_col = tab_col.push(Space::new().width(TAB_BAR_WIDTH).height(TAB_GROUP_GAP / 2.0));

    for &tab_id in display_order.iter() {
        let Some(tab) = tabs.iter().find(|t| t.id == tab_id) else { continue };
        if !tab.is_claude { continue; }
        if show_separators && tab.rank == AgentRank::Project {
            tab_col = tab_col.push(Space::new().width(TAB_BAR_WIDTH).height(TAB_GROUP_GAP / 2.0));
            tab_col = tab_col.push(separator());
            tab_col = tab_col.push(Space::new().width(TAB_BAR_WIDTH).height(TAB_GROUP_GAP / 2.0));
        }
        let indent = tab.depth as f32 * indent_step;
        let num = number_assignments.get(&tab.id).copied();
        tab_col = tab_col.push(tab_button(tab, num, indent));
    }

    let mut first_shell = true;
    for &tab_id in display_order.iter() {
        let Some(tab) = tabs.iter().find(|t| t.id == tab_id) else { continue };
        if tab.is_claude { continue; }
        if first_shell {
            first_shell = false;
            if show_separators {
                tab_col = tab_col.push(Space::new().width(TAB_BAR_WIDTH).height(TAB_GROUP_GAP / 2.0));
                tab_col = tab_col.push(separator());
                tab_col = tab_col.push(Space::new().width(TAB_BAR_WIDTH).height(TAB_GROUP_GAP / 2.0));
            } else if has_agents {
                tab_col = tab_col.push(Space::new().width(TAB_BAR_WIDTH).height(TAB_GROUP_GAP));
            }
        }
        let num = number_assignments.get(&tab.id).copied();
        tab_col = tab_col.push(tab_button(tab, num, 0.0));
    }

    if !toast_elements.is_empty() {
        tab_col = tab_col.push(Space::new().width(TAB_BAR_WIDTH).height(Fill));
        let mut toast_col = column![].spacing(PADDING);
        for t in toast_elements {
            toast_col = toast_col.push(t);
        }
        tab_col = tab_col.push(
            container(toast_col)
                .width(TAB_BAR_WIDTH)
                .padding(PADDING),
        );
    }

    let tab_col = tab_col.height(Fill);

    container(tab_col)
        .width(TAB_BAR_WIDTH)
        .height(Fill)
        .style(move |_theme| container::Style {
            background: Some(inactive_bg.into()),
            ..Default::default()
        })
        .into()
}

fn status_dot_color(status: AgentStatus, fg: Color) -> Color {
    match status {
        AgentStatus::Idle => fg,
        AgentStatus::Working => Color::from_rgb8(0x50, 0xc8, 0x50),
        AgentStatus::Compacting => Color::from_rgb8(0xb0, 0x80, 0xe0),
        AgentStatus::Blocked => Color::from_rgb8(0xe8, 0xb8, 0x30),
        AgentStatus::NeedsReview => Color::from_rgb8(0x40, 0xa0, 0xe0),
        AgentStatus::Error => Color::from_rgb8(0xe0, 0x40, 0x40),
    }
}

/// Format a shell tab title from a `"<cwd>\t<prompt>\t<command>"` OSC string.
///
/// Walks up the directory chain from longest to shortest until the title fits
/// within `max_chars`, stopping at `~` or `/`. The prompt character varies by
/// shell (`%` for zsh, `$` for bash, `#` for root).
fn format_shell_title(raw: &str, max_chars: usize) -> String {
    if !raw.contains('\u{a0}') {
        return raw.to_string();
    }
    let mut fields = raw.splitn(3, '\u{a0}');
    let cwd = fields.next().unwrap();
    let prompt = fields.next().unwrap_or("$");
    let cmd = fields.next().unwrap_or("");

    let slash_positions: Vec<usize> = cwd
        .char_indices()
        .filter_map(|(i, c)| if c == '/' { Some(i) } else { None })
        .collect();

    let nbsp = '\u{a0}';
    let suffix = if cmd.is_empty() {
        format!("{nbsp}{prompt}{nbsp}")
    } else {
        format!("{nbsp}{prompt}{nbsp}{cmd}")
    };

    let candidate = format!("{cwd}{suffix}");
    if candidate.len() <= max_chars {
        return candidate.trim_end().to_string();
    }

    for &pos in &slash_positions {
        let dir = &cwd[pos + 1..];
        let candidate = format!("{dir}{suffix}");
        if candidate.len() <= max_chars {
            return candidate.trim_end().to_string();
        }
    }

    if cmd.is_empty() {
        prompt.to_string()
    } else {
        format!("{prompt}{nbsp}{cmd}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_shell_title_idle() {
        assert_eq!(
            format_shell_title("~/src/mandelbot\u{a0}%\u{a0}", 40),
            "~/src/mandelbot\u{a0}%",
        );
        assert_eq!(
            format_shell_title("~/src/mandelbot\u{a0}%\u{a0}", 18),
            "src/mandelbot\u{a0}%",
        );
        assert_eq!(
            format_shell_title("~/src/mandelbot\u{a0}%\u{a0}", 14),
            "mandelbot\u{a0}%",
        );
        assert_eq!(
            format_shell_title("~\u{a0}%\u{a0}", 40),
            "~\u{a0}%",
        );
    }

    #[test]
    fn format_shell_title_with_command_zsh() {
        assert_eq!(
            format_shell_title("~/src/mandelbot\u{a0}%\u{a0}vim", 40),
            "~/src/mandelbot\u{a0}%\u{a0}vim",
        );
        assert_eq!(
            format_shell_title("~/src/mandelbot\u{a0}%\u{a0}vim", 22),
            "src/mandelbot\u{a0}%\u{a0}vim",
        );
        assert_eq!(
            format_shell_title("~/src/mandelbot\u{a0}%\u{a0}vim", 18),
            "mandelbot\u{a0}%\u{a0}vim",
        );
        assert_eq!(
            format_shell_title("~/src/mandelbot\u{a0}%\u{a0}vim", 10),
            "%\u{a0}vim",
        );
    }

    #[test]
    fn format_shell_title_with_command_bash() {
        assert_eq!(
            format_shell_title("~/src/mandelbot\u{a0}$\u{a0}vim", 40),
            "~/src/mandelbot\u{a0}$\u{a0}vim",
        );
    }

    #[test]
    fn format_shell_title_root() {
        assert_eq!(
            format_shell_title("/etc/nginx\u{a0}#\u{a0}nginx -t", 40),
            "/etc/nginx\u{a0}#\u{a0}nginx -t",
        );
    }

    #[test]
    fn format_shell_title_home() {
        assert_eq!(
            format_shell_title("~\u{a0}$\u{a0}ls", 40),
            "~\u{a0}$\u{a0}ls",
        );
    }

    #[test]
    fn format_shell_title_no_tab_passthrough() {
        assert_eq!(format_shell_title("zsh", 40), "zsh");
    }
}

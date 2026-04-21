//! Left-hand tab bar: agent tree (Home → Projects → Tasks), shell tabs,
//! and any toasts anchored to the bottom.

use iced::widget::{button, column, container, mouse_area, row, text, Space};
use iced::{Alignment, Border, Color, Element, Fill, Font, Theme};

use crate::animation::FlashState;
use crate::config::Config;
use crate::tab::{AgentRank, AgentStatus, TerminalTab};
use crate::tabs::Tabs;
use crate::theme::TerminalTheme;
use crate::ui::{Message, PADDING, TAB_BAR_WIDTH, TAB_GROUP_GAP};

const INDENT_STEP: f32 = 20.0;
const SUFFIX_SPACING: f32 = 6.0;

/// View-model for the tab bar: bundles the `App` refs the view reads from
/// and hangs the render helpers off a single `self`.
pub struct TabBar<'a> {
    pub tabs: &'a Tabs,
    pub bell_flashes: &'a FlashState,
    pub terminal_theme: &'a TerminalTheme,
    pub config: &'a Config,
}

impl<'a> TabBar<'a> {
    pub fn view(self, toast_elements: Vec<Element<'a, Message>>) -> Element<'a, Message> {
        let inactive_bg = self.terminal_theme.black;
        let has_agents = self.tabs.iter().any(|t| t.is_claude);
        let show_separators = self.tabs.len() > 1;

        let mut tab_col = column![];
        tab_col = tab_col.push(vspace(TAB_GROUP_GAP / 2.0));

        // Agent tree: Home → Projects → Tasks.
        let assignments = self.tabs.number_assignments();
        for &tab_id in self.tabs.display_order().iter() {
            let Some(tab) = self.tab_by_id(tab_id) else { continue };
            if !tab.is_claude {
                continue;
            }
            if show_separators && tab.rank == AgentRank::Project {
                tab_col = tab_col.push(self.separator());
            }
            let indent = tab.depth as f32 * INDENT_STEP;
            let num = assignments.get(&tab.id).copied();
            tab_col = tab_col.push(self.tab_button(tab, num, indent));
        }

        // Shell tabs, flat.
        let mut first_shell = true;
        for &tab_id in self.tabs.display_order().iter() {
            let Some(tab) = self.tab_by_id(tab_id) else { continue };
            if tab.is_claude {
                continue;
            }
            if first_shell {
                first_shell = false;
                if show_separators {
                    tab_col = tab_col.push(self.separator());
                } else if has_agents {
                    tab_col = tab_col.push(vspace(TAB_GROUP_GAP));
                }
            }
            let num = assignments.get(&tab.id).copied();
            tab_col = tab_col.push(self.tab_button(tab, num, 0.0));
        }

        // Toasts anchored to the bottom.
        if !toast_elements.is_empty() {
            tab_col = tab_col.push(Space::new().width(TAB_BAR_WIDTH).height(Fill));
            let mut toast_col = column![].spacing(PADDING);
            for t in toast_elements {
                toast_col = toast_col.push(t);
            }
            tab_col = tab_col.push(container(toast_col).width(TAB_BAR_WIDTH).padding(PADDING));
        }

        container(tab_col.height(Fill))
            .width(TAB_BAR_WIDTH)
            .height(Fill)
            .style(move |_theme| container::Style {
                background: Some(inactive_bg.into()),
                ..Default::default()
            })
            .into()
    }

    fn tab_by_id(&self, id: usize) -> Option<&'a TerminalTab> {
        self.tabs.get(id)
    }

    /// Group separator: a 1px line with half-gap padding above and below,
    /// so callers can insert it between tab groups without extra spacers.
    fn separator(&self) -> Element<'a, Message> {
        let muted = Color { a: 0.25, ..self.terminal_theme.fg };
        let line: Element<'a, Message> = container(Space::new())
            .width(TAB_BAR_WIDTH)
            .height(1)
            .style(move |_theme: &Theme| container::Style {
                background: Some(muted.into()),
                ..Default::default()
            })
            .into();
        column![vspace(TAB_GROUP_GAP / 2.0), line, vspace(TAB_GROUP_GAP / 2.0)].into()
    }

    /// A single row in the tab bar. The row composes (from left to right):
    /// indent spacer, fold toggle slot, label, suffix chips, trailing pad.
    fn tab_button(
        &self,
        tab: &'a TerminalTab,
        display_number: Option<usize>,
        indent: f32,
    ) -> Element<'a, Message> {
        let fg = self.terminal_theme.fg;
        let is_active = tab.id == self.tabs.active_id();
        let is_foldable = tab.is_claude && tab.rank != AgentRank::Home;
        let has_children = is_foldable && self.tabs.has_claude_children(tab.id);
        let is_folded = self.tabs.is_folded(tab.id);

        let base_bg = if is_active { self.terminal_theme.bg } else { self.terminal_theme.black };
        let bg = self.bell_flashes.blend(tab.id, base_bg, self.terminal_theme.yellow);

        let max_label_chars = self.max_label_chars(indent);
        let label_str = self.label_text(tab, max_label_chars);
        let label_len = label_str.len();
        let label = container(
            text(label_str)
                .size(self.config.font_size)
                .font(Font::MONOSPACE)
                .color(fg),
        )
        .width(Fill)
        .clip(true);

        let toggle = self.toggle_element(tab.id, is_foldable, has_children, is_folded);
        let suffix = self.suffix_row(tab, display_number, bg, label_len >= max_label_chars.saturating_sub(2));

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

        let tab_elem: Element<'a, Message> = mouse_area(styled)
            .on_press(Message::SelectTab(tab.id))
            .into();

        if indent > 0.0 {
            row![Space::new().width(indent), tab_elem]
                .width(TAB_BAR_WIDTH)
                .into()
        } else {
            tab_elem
        }
    }

    fn max_label_chars(&self, indent: f32) -> usize {
        let cw = self.config.char_width();
        let avail = TAB_BAR_WIDTH - indent - PADDING * 2.0 - cw * 3.0;
        (avail / cw) as usize
    }

    fn label_text(&self, tab: &TerminalTab, max_chars: usize) -> String {
        if tab.is_pending() {
            return "new project...".into();
        }
        if let Some(title) = &tab.title {
            return if tab.is_claude {
                title.clone()
            } else {
                format_shell_title(title, max_chars)
            };
        }
        if tab.rank == AgentRank::Project {
            if let Some(dir) = &tab.project_dir {
                return dir
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| dir.to_string_lossy().into_owned());
            }
            return String::new();
        }
        if !tab.is_claude {
            return "shell".into();
        }
        String::new()
    }

    fn toggle_element(
        &self,
        tab_id: usize,
        is_foldable: bool,
        has_children: bool,
        is_folded: bool,
    ) -> Element<'a, Message> {
        let fg = self.terminal_theme.fg;
        let slot_w = self.config.char_width() + 8.0;
        if has_children {
            let icon = if is_folded { "+" } else { "-" };
            let icon_text = text(icon)
                .size(self.config.font_size)
                .font(Font::MONOSPACE)
                .color(fg);
            let icon_container = container(icon_text).width(slot_w).align_x(Alignment::Center);
            mouse_area(icon_container)
                .on_press(Message::ToggleFoldTab(tab_id))
                .into()
        } else if is_foldable {
            Space::new().width(slot_w).into()
        } else {
            Space::new().width(0).into()
        }
    }

    /// Right-aligned chips: overflow bar, PR icon, background-task count,
    /// pending-wakeup indicator, status dot, digit shortcut.
    fn suffix_row(
        &self,
        tab: &'a TerminalTab,
        display_number: Option<usize>,
        bg: Color,
        label_overflows: bool,
    ) -> iced::widget::Row<'a, Message> {
        let fg = self.terminal_theme.fg;
        let size = self.config.font_size;

        let mut suffix = row![].align_y(Alignment::Center).spacing(SUFFIX_SPACING);

        if label_overflows {
            let muted = Color { a: 0.4, ..fg };
            suffix = suffix.push(text("|").size(size).font(Font::MONOSPACE).color(muted));
        }

        if tab.is_claude && tab.pr_number().is_some() {
            let muted = Color { a: 0.7, ..fg };
            let pr_icon = text("⎇").size(size).font(Font::MONOSPACE).color(muted);
            let pr_btn = button(pr_icon)
                .on_press(Message::OpenPr(tab.id))
                .padding(0)
                .style(move |_theme, _status| button::Style {
                    background: Some(bg.into()),
                    border: Border::default(),
                    ..Default::default()
                });
            suffix = suffix.push(pr_btn);
        }

        if tab.is_claude && tab.background_tasks > 0 {
            suffix = suffix.push(
                text(format!("+{}", tab.background_tasks))
                    .size(size * 0.75)
                    .font(Font::MONOSPACE)
                    .color(self.terminal_theme.cyan),
            );
        }

        if tab.is_claude && has_pending_wakeup(tab) {
            suffix = suffix.push(text("⏱").size(size * 0.75).color(self.terminal_theme.cyan));
        }

        let dot_char = if tab.status == AgentStatus::Idle { "○" } else { "●" };
        suffix = suffix.push(
            text(dot_char)
                .size(size * 0.6)
                .color(status_dot_color(tab.status, fg)),
        );

        let number_text = match display_number {
            Some(n) => format!("{n}"),
            None => " ".into(),
        };
        suffix = suffix.push(
            text(number_text)
                .size(size)
                .font(Font::MONOSPACE)
                .color(fg),
        );

        suffix
    }
}

fn vspace(h: f32) -> Element<'static, Message> {
    Space::new().width(TAB_BAR_WIDTH).height(h).into()
}

fn has_pending_wakeup(tab: &TerminalTab) -> bool {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    matches!(tab.next_wakeup_at_ms, Some(t) if t > now_ms)
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

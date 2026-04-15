use std::time::{SystemTime, UNIX_EPOCH};

use iced::widget::{container, row, text, Space};
use iced::{Alignment, Border, Color, Element, Font, Length, Theme};

use crate::config::Config;
use crate::tab::{AgentRank, TerminalTab};
use crate::theme::TerminalTheme;
use crate::ui::{Message, TimelineMode};

const MARKER_HEIGHT: f32 = 26.0;
const MARKER_SPACING: f32 = 6.0;
const MARKER_PADDING_X: f32 = 8.0;
const STRIP_HEIGHT: f32 = MARKER_HEIGHT + 12.0;

pub fn view<'a>(
    tab: &'a TerminalTab,
    config: &'a Config,
    theme: &'a TerminalTheme,
) -> Element<'a, Message> {
    let fg = theme.fg;
    let muted = Color { a: 0.55, ..fg };

    if tab.checkpoints.is_empty() {
        let message = match tab.rank {
            AgentRank::Task => {
                "no checkpoints yet — the Stop hook creates one after each turn"
            }
            AgentRank::Home | AgentRank::Project => {
                "checkpoints are only created in task tabs (claude running in a worktree)"
            }
        };
        let label = text(message)
            .size(config.font_size * 0.85)
            .font(Font::MONOSPACE)
            .color(muted);
        let strip = container(label)
            .padding([6, 10])
            .width(Length::Fill)
            .height(STRIP_HEIGHT)
            .align_y(Alignment::Center)
            .style(move |_: &Theme| container::Style {
                background: Some(theme.black.into()),
                border: Border { width: 0.0, ..Border::default() },
                ..Default::default()
            });
        return strip.into();
    }

    let focused = tab.timeline_cursor.min(tab.checkpoints.len() - 1);
    let selected_bg = theme.blue;
    let unselected_bg = Color { a: 0.25, ..fg };

    let mut markers = row![]
        .spacing(MARKER_SPACING)
        .align_y(Alignment::Center);

    for (i, ckpt) in tab.checkpoints.iter().enumerate() {
        let is_focused = i == focused;
        let bg = if is_focused { selected_bg } else { unselected_bg };
        let label_color = if is_focused { theme.bg } else { fg };

        let label_text = format!("{} #{}", format_hhmm(ckpt.created_at), ckpt.id);
        let label = text(label_text)
            .size(config.font_size * 0.85)
            .font(Font::MONOSPACE)
            .color(label_color);

        let marker = container(label)
            .padding([4, MARKER_PADDING_X as u16])
            .height(MARKER_HEIGHT)
            .align_y(Alignment::Center)
            .style(move |_: &Theme| container::Style {
                background: Some(bg.into()),
                border: Border {
                    radius: 4.0.into(),
                    width: if is_focused { 1.5 } else { 0.0 },
                    color: fg,
                },
                ..Default::default()
            });

        markers = markers.push(marker);
    }

    let hint_text = format!(
        "←/→ scrub   enter: {}   shift+enter: {}   ({} / {})",
        describe(TimelineMode::Replace),
        describe(TimelineMode::Fork),
        focused + 1,
        tab.checkpoints.len(),
    );
    let hint = text(hint_text)
        .size(config.font_size * 0.75)
        .font(Font::MONOSPACE)
        .color(muted);

    let content = row![markers, Space::new().width(Length::Fill), hint]
        .align_y(Alignment::Center)
        .spacing(12);

    container(content)
        .padding([6, 10])
        .width(Length::Fill)
        .height(STRIP_HEIGHT)
        .style(move |_: &Theme| container::Style {
            background: Some(theme.black.into()),
            ..Default::default()
        })
        .into()
}

fn describe(mode: TimelineMode) -> &'static str {
    match mode {
        TimelineMode::Replace => "replace",
        TimelineMode::Fork => "fork",
    }
}

fn format_hhmm(t: SystemTime) -> String {
    let secs = t.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let total_minutes = secs / 60;
    let hh = (total_minutes / 60) % 24;
    let mm = total_minutes % 60;
    format!("{hh:02}:{mm:02}")
}

//! Toast notification widget.

use iced::widget::{button, column, container, row, text};
use iced::{Alignment, Border, Color, Element, Fill};

use crate::config::Config;
use crate::theme::TerminalTheme;
use crate::toast::Toast;
use crate::ui::Message;

const PADDING: f32 = 4.0;

pub fn view<'a>(
    toast: &'a Toast,
    terminal_theme: &TerminalTheme,
    config: &Config,
) -> Element<'a, Message> {
    let fg = terminal_theme.fg;
    let muted_fg = Color { a: 0.6, ..fg };
    let toast_bg = terminal_theme.bg;
    let ui_font = config.font();

    let message_text = text(toast.message.clone())
        .size(config.font_size)
        .font(ui_font)
        .color(fg);

    let close_btn = button(
        text("×")
            .size(config.font_size)
            .font(ui_font)
            .color(muted_fg),
    )
        .on_press(Message::DismissToast(toast.id))
        .padding([0, 4])
        .style(move |_theme, _status| button::Style {
            background: None,
            border: Border::default(),
            ..Default::default()
        });

    let header = row![
        container(message_text).width(Fill).clip(true),
        close_btn,
    ]
        .align_y(Alignment::Start)
        .spacing(PADDING);

    let mut col = column![header].spacing(PADDING);

    let action: Option<(&'static str, Message)> = if toast.target_tab_id.is_some() {
        Some(("Go to", Message::FocusFromToast(toast.id)))
    } else if toast.prompt.is_some() {
        Some(("Open", Message::SpawnFromToast(toast.id)))
    } else {
        None
    };

    if let Some((label, on_press)) = action {
        let open_label = text(label)
            .size(config.font_size)
            .font(ui_font)
            .color(fg);
        let open_btn = button(open_label)
            .on_press(on_press)
            .padding([2, 8])
            .style(move |_theme, _status| button::Style {
                background: Some(Color { a: 0.15, ..fg }.into()),
                text_color: fg,
                border: Border {
                    color: Color { a: 0.3, ..fg },
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..Default::default()
            });
        col = col.push(row![open_btn]);
    }

    container(col)
        .padding(8)
        .width(Fill)
        .style(move |_theme| container::Style {
            background: Some(toast_bg.into()),
            border: Border {
                color: Color { a: 0.3, ..fg },
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        })
        .into()
}

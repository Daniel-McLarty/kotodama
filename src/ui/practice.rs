// SPDX-License-Identifier: MPL-2.0

use iced::widget::{button, column, container, progress_bar, row, text};
use iced::{Alignment, Element, Length};

use crate::models::{AppSettings, DailyStreak, PracticeMode, ResolvedTheme};
use crate::ui::theme::{self, ThemeTokens};
use crate::ui::Message;

/// Renders the pronunciation practice mode selector, session status, and keyboard legend.
pub fn view<'a>(
    active_mode: PracticeMode,
    settings: &'a AppSettings,
    streak: &'a DailyStreak,
    has_deck: bool,
    resolved_theme: ResolvedTheme,
) -> Element<'a, Message> {
    let colors = theme::tokens(resolved_theme);

    let mode_pills = PracticeMode::ALL.iter().fold(
        row![].spacing(10).align_y(Alignment::Center),
        |acc, &mode| {
            let selected = mode == active_mode;
            acc.push(
                button(text(mode.label()).size(14))
                    .padding([10, 16])
                    .style(theme::pill_button_style(selected))
                    .on_press(Message::SelectPracticeMode(mode)),
            )
        },
    );

    let deck_badge = if has_deck {
        container(text("Deck Ready").size(12).color(colors.sage))
            .padding([4, 10])
            .style(theme::badge_style(colors.sage_soft, colors.sage))
    } else {
        container(text("No Deck Connected Yet").size(12).color(colors.amber))
            .padding([4, 10])
            .style(theme::badge_style(colors.amber_soft, colors.amber))
    };

    let blind_label = if settings.blind_mode {
        "Blind Mode: On"
    } else {
        "Blind Mode: Off"
    };

    let status_badges = row![
        deck_badge,
        container(
            text(settings.playback_speed.label())
                .size(12)
                .color(colors.text_primary),
        )
        .padding([4, 10])
        .style(theme::badge_style(colors.card_muted, colors.border)),
        container(text(blind_label).size(12).color(colors.text_primary))
            .padding([4, 10])
            .style(theme::badge_style(colors.card_muted, colors.border)),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let mode_card = container(
        column![
            row![
                text(active_mode.label()).size(20).color(colors.text_primary),
                status_badges,
            ]
            .spacing(12)
            .align_y(Alignment::Center),
            text(active_mode.short_description())
                .size(14)
                .color(colors.text_muted),
            container(
                column![
                    row![
                        text("Daily Goal Progress").size(13).color(colors.text_primary),
                        text(streak.daily_goal.label())
                            .size(13)
                            .color(colors.text_muted),
                    ]
                    .spacing(12),
                    progress_bar(0.0..=1.0, streak.goal_progress_ratio()),
                ]
                .spacing(8),
            )
            .width(Length::Fill)
            .padding(14)
            .style(theme::muted_card_style),
            button(text("Start Practice Session").size(14))
                .padding([10, 18])
                .style(theme::primary_button_style)
                .on_press(Message::TriggerRecoverableAction),
        ]
        .spacing(14),
    )
    .width(Length::Fill)
    .padding(20)
    .style(theme::card_style);

    let shortcuts_card = container(
        column![
            text("Keyboard-First Controls")
                .size(17)
                .color(colors.text_primary),
            row![
                shortcut_chip("Space", "Play reference", colors),
                shortcut_chip("R", "Start / stop mic", colors),
                shortcut_chip("T", "Play your take", colors),
                shortcut_chip("1 – 4", "FSRS override", colors),
            ]
            .spacing(10),
        ]
        .spacing(12),
    )
    .width(Length::Fill)
    .padding(18)
    .style(theme::card_style);

    column![mode_pills, mode_card, shortcuts_card]
        .spacing(18)
        .width(Length::Fill)
        .into()
}

fn shortcut_chip<'a>(
    key: &'a str,
    action: &'a str,
    colors: &'static ThemeTokens,
) -> Element<'a, Message> {
    container(
        column![
            text(key).size(14).color(colors.coral_text),
            text(action).size(12).color(colors.text_muted),
        ]
        .spacing(4),
    )
    .width(Length::Fill)
    .padding(12)
    .style(theme::muted_card_style)
    .into()
}

// SPDX-License-Identifier: MPL-2.0

use iced::widget::{button, column, container, progress_bar, row, text};
use iced::{Alignment, Element, Length};

use crate::models::{AppSettings, DailyStreak, PracticeMode};
use crate::ui::{Message, theme};

/// Renders the pronunciation practice mode selector, session status, and keyboard legend.
pub fn view<'a>(
    active_mode: PracticeMode,
    settings: &'a AppSettings,
    streak: &'a DailyStreak,
    has_deck: bool,
) -> Element<'a, Message> {
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
        container(text("Deck Ready").size(12).color(theme::SAGE))
            .padding([4, 10])
            .style(theme::badge_style(theme::SAGE_SOFT, theme::SAGE))
    } else {
        container(text("No Deck Connected Yet").size(12).color(theme::AMBER))
            .padding([4, 10])
            .style(theme::badge_style(theme::AMBER_SOFT, theme::AMBER))
    };

    let blind_label = if settings.blind_mode {
        "Blind Mode: On"
    } else {
        "Blind Mode: Off"
    };

    let status_badges = row![
        deck_badge,
        container(text(settings.playback_speed.label()).size(12).color(theme::WARM_SLATE))
            .padding([4, 10])
            .style(theme::badge_style(theme::CARD_MUTED, theme::BORDER_WARM)),
        container(text(blind_label).size(12).color(theme::WARM_SLATE))
            .padding([4, 10])
            .style(theme::badge_style(theme::CARD_MUTED, theme::BORDER_WARM)),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let mode_card = container(
        column![
            row![
                text(active_mode.label()).size(20).color(theme::WARM_SLATE),
                status_badges,
            ]
            .spacing(12)
            .align_y(Alignment::Center),
            text(active_mode.short_description())
                .size(14)
                .color(theme::MUTED_SLATE),
            container(
                column![
                    row![
                        text("Daily Goal Progress").size(13).color(theme::WARM_SLATE),
                        text(streak.daily_goal.label())
                            .size(13)
                            .color(theme::MUTED_SLATE),
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
            text("Keyboard-First Controls").size(17).color(theme::WARM_SLATE),
            row![
                shortcut_chip("Space", "Play reference"),
                shortcut_chip("R", "Start / stop mic"),
                shortcut_chip("T", "Play your take"),
                shortcut_chip("1 – 4", "FSRS override"),
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

fn shortcut_chip<'a>(key: &'a str, action: &'a str) -> Element<'a, Message> {
    container(
        column![
            text(key).size(14).color(theme::CORAL_HOVER),
            text(action).size(12).color(theme::MUTED_SLATE),
        ]
        .spacing(4),
    )
    .width(Length::Fill)
    .padding(12)
    .style(theme::muted_card_style)
    .into()
}

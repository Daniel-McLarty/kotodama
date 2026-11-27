// SPDX-License-Identifier: MPL-2.0

use chrono::Utc;
use iced::widget::{button, column, container, progress_bar, row, text};
use iced::{Alignment, Element, Length};

use crate::models::{DailyStreak, KanaStats, MoraAttempt, ResolvedTheme};
use crate::ui::theme::{self, ThemeTokens};
use crate::ui::Message;

/// Renders the analytics dashboard with daily streaks, kana confusion preview, and export controls.
pub fn view<'a>(
    streak: &'a DailyStreak,
    export_status: Option<&'a str>,
    resolved_theme: ResolvedTheme,
) -> Element<'a, Message> {
    let colors = theme::tokens(resolved_theme);

    let goal_status = if streak.is_daily_goal_met() {
        "Goal complete"
    } else {
        "In progress"
    };

    let stats_row = row![
        metric_card(
            "Current Streak",
            format!("{} days", streak.current_streak_days),
            format!("Best: {} days", streak.longest_streak_days),
            colors,
        ),
        metric_card(
            "Today's Progress",
            format!("{} cards", streak.cards_practiced_today),
            format!("{} · {}", streak.daily_goal.label(), goal_status),
            colors,
        ),
        metric_card(
            "Total Sessions",
            streak.total_sessions.to_string(),
            format!("{} min practiced today", streak.minutes_practiced_today),
            colors,
        ),
    ]
    .spacing(14);

    let goal_card = container(
        column![
            row![
                text("Daily Goal Completion")
                    .size(16)
                    .color(colors.text_primary),
                text(format!(
                    "{}%",
                    (streak.goal_progress_ratio() * 100.0).round() as u32
                ))
                .size(14)
                .color(colors.sage),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
            progress_bar(0.0..=1.0, streak.goal_progress_ratio()),
        ]
        .spacing(10),
    )
    .width(Length::Fill)
    .padding(18)
    .style(theme::card_style);

    let sample_breakdown = sample_kana_breakdown();

    let breakdown_card = container(
        column![
            text("Per-Kana Clarity Breakdown (Preview)")
                .size(18)
                .color(colors.text_primary),
            text("Once practice sessions are recorded, each kana lists exact accuracy and its top 5 substitutions.")
                .size(14)
                .color(colors.text_muted),
            container(text(sample_breakdown).size(14).color(colors.text_primary))
                .width(Length::Fill)
                .padding(14)
                .style(theme::muted_card_style),
        ]
        .spacing(10),
    )
    .width(Length::Fill)
    .padding(20)
    .style(theme::card_style);

    let mut export_col = column![
        text("Export Session History & Confusion Matrix")
            .size(17)
            .color(colors.text_primary),
        text("Export full pronunciation history, per-kana mistake counts, and pitch accent error logs.")
            .size(14)
            .color(colors.text_muted),
        row![
            button(text("Export CSV").size(14))
                .padding([10, 16])
                .style(theme::secondary_button_style)
                .on_press(Message::ExportCsvRequested),
            button(text("Export JSON").size(14))
                .padding([10, 16])
                .style(theme::secondary_button_style)
                .on_press(Message::ExportJsonRequested),
        ]
        .spacing(12),
    ]
    .spacing(10);

    if let Some(status) = export_status {
        export_col = export_col.push(
            container(text(status).size(13).color(colors.sage))
                .width(Length::Fill)
                .padding([8, 12])
                .style(theme::muted_card_style),
        );
    }

    let export_card = container(export_col)
        .width(Length::Fill)
        .padding(20)
        .style(theme::card_style);

    column![stats_row, goal_card, breakdown_card, export_card]
        .spacing(18)
        .width(Length::Fill)
        .into()
}

fn metric_card<'a>(
    title: &'a str,
    value: String,
    caption: String,
    colors: &'static ThemeTokens,
) -> Element<'a, Message> {
    container(
        column![
            text(title).size(13).color(colors.text_muted),
            text(value).size(24).color(colors.text_primary),
            text(caption).size(12).color(colors.sage),
        ]
        .spacing(6),
    )
    .width(Length::Fill)
    .padding(18)
    .style(theme::card_style)
    .into()
}

fn sample_kana_breakdown() -> String {
    let mut stats = KanaStats::new("つ");
    let now = Utc::now();
    for _ in 0..82 {
        stats.record_attempt(&MoraAttempt {
            card_id: 0,
            expected_kana: "つ".to_string(),
            heard_kana: Some("つ".to_string()),
            confidence: 0.95,
            recorded_at: now,
        });
    }
    for (heard, count) in [
        (Some("す"), 5),
        (Some("ず"), 4),
        (Some("すう"), 3),
        (None, 6),
    ] {
        for _ in 0..count {
            stats.record_attempt(&MoraAttempt {
                card_id: 0,
                expected_kana: "つ".to_string(),
                heard_kana: heard.map(str::to_string),
                confidence: 0.4,
                recorded_at: now,
            });
        }
    }
    stats.format_breakdown()
}

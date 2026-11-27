// SPDX-License-Identifier: MPL-2.0

use chrono::Utc;
use iced::alignment::Vertical;
use iced::widget::text::Alignment as TextAlignment;
use iced::widget::{
    button, canvas, column, container, progress_bar, row, space, text, text_input,
};
use iced::{
    Alignment, Color, Element, Length, Pixels, Point, Rectangle, Renderer, Size, Theme, mouse,
};

use crate::models::{
    CardPitchSummary, DailyGoal, DailyStreak, DashboardSnapshot, DashboardTimeRange, KanaSortOrder,
    KanaStats, MoraAttempt, NavTab, PitchErrorLogEntry, ResolvedTheme,
};
use crate::scoring::mora::{mora_equivalence_rule, normalize_to_hiragana};
use crate::ui::Message;
use crate::ui::theme::{self, ThemeTokens};

const MAX_KANA_ROWS: usize = 45;
const MAX_PITCH_CARD_ROWS: usize = 40;
const MAX_CARD_HISTORY_ENTRIES: usize = 10;

/// Mutable UI state for the Analytics Dashboard screen.
#[derive(Debug, Clone, Default)]
pub struct DashboardState {
    /// Latest analytics snapshot loaded from SQLite.
    pub snapshot: DashboardSnapshot,
    /// Whether an asynchronous SQLite snapshot refresh is currently running.
    pub is_loading: bool,
    /// Active calendar time-window filter for trend charts.
    pub time_range: DashboardTimeRange,
    /// Active sort order for the per-kana clarity breakdown list.
    pub kana_sort: KanaSortOrder,
    /// Search query filtering per-kana rows by kana or confused sound.
    pub kana_filter: String,
    /// Currently expanded kana character in the per-kana breakdown list.
    pub selected_kana: Option<String>,
    /// Currently expanded Anki card ID in the per-card pitch error list.
    pub selected_pitch_card_id: Option<i64>,
    /// Search query filtering per-card pitch history rows by word, kana, or deck.
    pub pitch_card_filter: String,
    /// Whether at least one SQLite dashboard load has completed.
    pub has_loaded: bool,
}

impl DashboardState {
    /// Returns per-kana clarity rows matching `kana_filter` and sorted by `kana_sort`.
    pub fn filtered_kana_stats(&self) -> Vec<&KanaStats> {
        let raw_trimmed = self.kana_filter.trim();
        let query_lower = raw_trimmed.to_lowercase();
        let normalized_query = normalize_to_hiragana(raw_trimmed);

        let mut rows: Vec<&KanaStats> = self
            .snapshot
            .kana_stats
            .iter()
            .filter(|stats| {
                if raw_trimmed.is_empty() {
                    return true;
                }
                if normalize_to_hiragana(&stats.kana).contains(&normalized_query)
                    || stats.kana.to_lowercase().contains(&query_lower)
                {
                    return true;
                }
                stats.top_mistakes(5).into_iter().any(|mistake| {
                    normalize_to_hiragana(&mistake.heard_kana).contains(&normalized_query)
                        || mistake.heard_kana.to_lowercase().contains(&query_lower)
                })
            })
            .collect();

        rows.sort_by(|left, right| match self.kana_sort {
            KanaSortOrder::LowestAccuracy => left
                .accuracy_percent()
                .total_cmp(&right.accuracy_percent())
                .then_with(|| right.total_attempts.cmp(&left.total_attempts))
                .then_with(|| left.kana.cmp(&right.kana)),
            KanaSortOrder::MostPracticed => right
                .total_attempts
                .cmp(&left.total_attempts)
                .then_with(|| left.accuracy_percent().total_cmp(&right.accuracy_percent()))
                .then_with(|| left.kana.cmp(&right.kana)),
            KanaSortOrder::Alphabetical => left.kana.cmp(&right.kana),
        });

        rows
    }

    /// Returns per-card pitch error summaries matching `pitch_card_filter`.
    pub fn filtered_card_pitch_summaries(&self) -> Vec<&CardPitchSummary> {
        let raw_trimmed = self.pitch_card_filter.trim();
        if raw_trimmed.is_empty() {
            return self.snapshot.card_pitch_summaries.iter().collect();
        }
        let query_lower = raw_trimmed.to_lowercase();
        let normalized_query = normalize_to_hiragana(raw_trimmed);

        self.snapshot
            .card_pitch_summaries
            .iter()
            .filter(|summary| {
                summary.word.to_lowercase().contains(&query_lower)
                    || summary.kana.to_lowercase().contains(&query_lower)
                    || summary.deck_name.to_lowercase().contains(&query_lower)
                    || (!normalized_query.is_empty()
                        && normalize_to_hiragana(&summary.kana).contains(&normalized_query))
            })
            .collect()
    }

    /// Returns detailed pitch error review entries for `card_id` ordered newest first.
    pub fn pitch_history_for_card(&self, card_id: i64) -> Vec<&PitchErrorLogEntry> {
        self.snapshot
            .pitch_error_logs
            .iter()
            .filter(|entry| entry.card_id == card_id)
            .collect()
    }
}

/// Computes a clean upper Y-axis bound (`40`, `60`, `80`, or `100`%) for pitch error charts.
pub fn compute_pitch_axis_max(values: &[f32]) -> f32 {
    let max_val = values
        .iter()
        .copied()
        .filter(|v| v.is_finite() && *v >= 0.0)
        .fold(0.0_f32, f32::max);

    if max_val <= 40.0 {
        40.0
    } else if max_val <= 60.0 {
        60.0
    } else if max_val <= 80.0 {
        80.0
    } else {
        100.0
    }
}

/// Maps chronological percentage values onto screen coordinates inside `plot`.
pub fn map_trend_points(values: &[f32], plot: Rectangle, y_min: f32, y_max: f32) -> Vec<Point> {
    if values.is_empty() || plot.width <= 0.0 || plot.height <= 0.0 {
        return Vec::new();
    }
    let span = (y_max - y_min).max(1.0);
    let len = values.len();

    values
        .iter()
        .enumerate()
        .map(|(idx, &raw)| {
            let x = if len == 1 {
                plot.x + plot.width * 0.5
            } else {
                plot.x + (idx as f32 / (len - 1) as f32) * plot.width
            };
            let safe = if raw.is_finite() { raw } else { y_min };
            let norm_y = ((safe - y_min) / span).clamp(0.0, 1.0);
            let y = plot.y + plot.height * (1.0 - norm_y);
            Point::new(x, y)
        })
        .collect()
}

/// Chooses non-overlapping X-axis tick indices for a time series of length `count`.
pub fn select_x_tick_indices(count: usize) -> Vec<usize> {
    match count {
        0 => Vec::new(),
        1 => vec![0],
        2 => vec![0, 1],
        3..=5 => (0..count).collect(),
        _ => vec![0, count / 2, count - 1],
    }
}

/// Iced `Canvas` program rendering a smooth line/area trend chart with percentage gridlines.
pub struct TrendChartProgram {
    /// Chronological `(label, percentage)` data points to plot.
    pub points: Vec<(String, f32)>,
    /// Upper Y-axis bound in percentage (`0..=y_max`).
    pub y_max: f32,
    /// Accent color for the line, area fill, and data markers.
    pub accent: Color,
    /// Friendly message displayed inside the chart when `points` is empty.
    pub empty_message: &'static str,
    /// Active Light or Dark appearance.
    pub resolved_theme: ResolvedTheme,
}

impl canvas::Program<Message> for TrendChartProgram {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let colors = theme::tokens(self.resolved_theme);
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let width = bounds.width.max(1.0);
        let height = bounds.height.max(1.0);

        let bg = canvas::Path::rounded_rectangle(
            Point::ORIGIN,
            Size::new(width, height),
            12.0.into(),
        );
        frame.fill(&bg, colors.card_muted);
        frame.stroke(
            &bg,
            canvas::Stroke::default()
                .with_color(colors.border)
                .with_width(1.0),
        );

        let left_pad = 44.0_f32;
        let right_pad = 16.0_f32;
        let top_pad = 14.0_f32;
        let bottom_pad = 28.0_f32;
        let plot = Rectangle {
            x: left_pad,
            y: top_pad,
            width: (width - left_pad - right_pad).max(1.0),
            height: (height - top_pad - bottom_pad).max(1.0),
        };

        let y_max = self.y_max.max(1.0);
        for i in 0..=4 {
            let frac = i as f32 / 4.0;
            let val = y_max * frac;
            let y = plot.y + plot.height * (1.0 - frac);
            let grid_line =
                canvas::Path::line(Point::new(plot.x, y), Point::new(plot.x + plot.width, y));
            let line_color = if i == 0 {
                Color::from_rgba(
                    colors.text_muted.r,
                    colors.text_muted.g,
                    colors.text_muted.b,
                    0.45,
                )
            } else {
                Color::from_rgba(colors.border.r, colors.border.g, colors.border.b, 0.85)
            };
            frame.stroke(
                &grid_line,
                canvas::Stroke::default()
                    .with_color(line_color)
                    .with_width(1.0),
            );
            frame.fill_text(canvas::Text {
                content: format!("{val:.0}%"),
                position: Point::new(left_pad - 8.0, y),
                color: colors.text_muted,
                size: Pixels(11.0),
                align_x: TextAlignment::Right,
                align_y: Vertical::Center,
                ..canvas::Text::default()
            });
        }

        if self.points.is_empty() {
            frame.fill_text(canvas::Text {
                content: self.empty_message.to_string(),
                position: Point::new(plot.x + plot.width * 0.5, plot.y + plot.height * 0.5),
                color: colors.text_muted,
                size: Pixels(13.0),
                align_x: TextAlignment::Center,
                align_y: Vertical::Center,
                ..canvas::Text::default()
            });
            return vec![frame.into_geometry()];
        }

        let values: Vec<f32> = self.points.iter().map(|(_, v)| *v).collect();
        let coords = map_trend_points(&values, plot, 0.0, y_max);

        for idx in select_x_tick_indices(self.points.len()) {
            let align_x = if coords.len() == 1 {
                TextAlignment::Center
            } else if idx == 0 {
                TextAlignment::Left
            } else if idx + 1 == coords.len() {
                TextAlignment::Right
            } else {
                TextAlignment::Center
            };
            frame.fill_text(canvas::Text {
                content: self.points[idx].0.clone(),
                position: Point::new(coords[idx].x, plot.y + plot.height + 14.0),
                color: colors.text_muted,
                size: Pixels(11.0),
                align_x,
                align_y: Vertical::Center,
                ..canvas::Text::default()
            });
        }

        if coords.len() == 1 {
            let pt = coords[0];
            let guide = canvas::Path::line(
                Point::new(plot.x, pt.y),
                Point::new(plot.x + plot.width, pt.y),
            );
            frame.stroke(
                &guide,
                canvas::Stroke::default()
                    .with_color(Color::from_rgba(
                        self.accent.r,
                        self.accent.g,
                        self.accent.b,
                        0.45,
                    ))
                    .with_width(1.5),
            );
            let dot = canvas::Path::circle(pt, 4.5);
            frame.fill(&dot, self.accent);

            let label_y = if pt.y > plot.y + 18.0 {
                pt.y - 12.0
            } else {
                pt.y + 12.0
            };
            frame.fill_text(canvas::Text {
                content: format!("{:.1}%", self.points[0].1),
                position: Point::new(pt.x, label_y),
                color: self.accent,
                size: Pixels(12.0),
                align_x: TextAlignment::Center,
                align_y: Vertical::Center,
                ..canvas::Text::default()
            });
            return vec![frame.into_geometry()];
        }

        let baseline_y = plot.y + plot.height;
        let area = canvas::Path::new(|builder| {
            builder.move_to(Point::new(coords[0].x, baseline_y));
            for &pt in &coords {
                builder.line_to(pt);
            }
            if let Some(&last) = coords.last() {
                builder.line_to(Point::new(last.x, baseline_y));
            }
            builder.close();
        });
        frame.fill(
            &area,
            Color::from_rgba(self.accent.r, self.accent.g, self.accent.b, 0.16),
        );

        let line = canvas::Path::new(|builder| {
            builder.move_to(coords[0]);
            for &pt in &coords[1..] {
                builder.line_to(pt);
            }
        });
        frame.stroke(
            &line,
            canvas::Stroke::default()
                .with_color(self.accent)
                .with_width(2.5)
                .with_line_cap(canvas::LineCap::Round)
                .with_line_join(canvas::LineJoin::Round),
        );

        let stride = if coords.len() > 30 { 2 } else { 1 };
        for (idx, &pt) in coords.iter().enumerate() {
            if idx % stride == 0 || idx + 1 == coords.len() {
                let dot = canvas::Path::circle(pt, 3.0);
                frame.fill(&dot, self.accent);
            }
        }

        vec![frame.into_geometry()]
    }
}

/// Renders the full Analytics Dashboard with streaks, accuracy charts, per-kana confusion breakdown, per-card pitch history, and export controls.
pub fn view<'a>(
    state: &'a DashboardState,
    streak: &'a DailyStreak,
    export_status: Option<&'a str>,
    resolved_theme: ResolvedTheme,
) -> Element<'a, Message> {
    let colors = theme::tokens(resolved_theme);
    let snapshot = &state.snapshot;

    let hero_and_kpis = render_streak_and_kpis(state, streak, snapshot, colors);
    let charts_card = render_accuracy_charts_card(state, snapshot, resolved_theme, colors);
    let kana_card = render_kana_breakdown_card(state, snapshot, colors);
    let pitch_card = render_card_pitch_history_card(state, snapshot, resolved_theme, colors);
    let export_card = render_export_card(snapshot, export_status, colors);

    column![
        hero_and_kpis,
        charts_card,
        kana_card,
        pitch_card,
        export_card,
    ]
    .spacing(18)
    .width(Length::Fill)
    .into()
}

fn render_streak_and_kpis<'a>(
    state: &'a DashboardState,
    streak: &'a DailyStreak,
    snapshot: &'a DashboardSnapshot,
    colors: &'static ThemeTokens,
) -> Element<'a, Message> {
    let streak_active = streak.current_streak_days > 0;
    let goal_met = streak.is_daily_goal_met();

    let (badge_text, badge_fg, badge_bg, badge_border) = if goal_met {
        (
            format!("{} Day Streak · Goal Complete", streak.current_streak_days),
            colors.sage,
            colors.sage_soft,
            colors.sage,
        )
    } else if streak_active {
        (
            format!("{} Day Streak Active", streak.current_streak_days),
            colors.sage,
            colors.sage_soft,
            colors.sage,
        )
    } else {
        (
            "Start Your Streak Today".to_string(),
            colors.coral_text,
            colors.coral_soft,
            colors.coral,
        )
    };

    let streak_copy = match (streak.last_practice_date, goal_met) {
        (_, true) => "Daily pronunciation target reached — great consistency!".to_string(),
        (Some(date), false) => format!(
            "Last practiced on {} · Best streak: {} days · {} total reviews",
            date.format("%Y-%m-%d"),
            streak.longest_streak_days,
            snapshot.total_reviews
        ),
        (None, false) => "Complete a quick Mora, Pitch, or Shadowing card to light up your streak.".to_string(),
    };

    let refresh_label = if state.is_loading {
        "Refreshing..."
    } else {
        "Refresh"
    };
    let mut refresh_btn = button(text(refresh_label).size(13))
        .padding([8, 14])
        .style(theme::secondary_button_style);
    if !state.is_loading {
        refresh_btn = refresh_btn.on_press(Message::DashboardRefreshRequested);
    }

    let streak_banner = container(
        row![
            column![
                row![
                    container(text(badge_text).size(13).color(badge_fg))
                        .padding([5, 12])
                        .style(theme::badge_style(badge_bg, badge_border)),
                    text("Pronunciation Progress")
                        .size(18)
                        .color(colors.text_primary),
                ]
                .spacing(12)
                .align_y(Alignment::Center),
                text(streak_copy).size(13).color(colors.text_muted),
            ]
            .spacing(6),
            space().width(Length::Fill),
            refresh_btn,
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding(18)
    .style(theme::card_style);

    let today_value = match streak.daily_goal {
        DailyGoal::Cards(_) => format!("{} cards", streak.cards_practiced_today),
        DailyGoal::Minutes(_) => format!("{} min", streak.minutes_practiced_today),
    };
    let today_caption = format!(
        "{} · {}",
        streak.daily_goal.label(),
        if goal_met { "Goal met" } else { "In progress" }
    );

    let mora_value = snapshot
        .overall_mora_accuracy_percent
        .map_or_else(|| "—".to_string(), |v| format!("{v:.1}%"));
    let mora_caption = if snapshot.tracked_kana_count > 0 {
        format!("Across {} tracked kana", snapshot.tracked_kana_count)
    } else {
        "Record Mora or Shadowing drills".to_string()
    };

    let pitch_value = snapshot
        .overall_pitch_error_percent
        .map_or_else(|| "—".to_string(), |v| format!("{v:.1}%"));
    let pitch_caption = if snapshot.card_pitch_summaries.is_empty() {
        "Record Pitch or Shadowing drills".to_string()
    } else {
        format!(
            "Across {} cards · {} logs",
            snapshot.card_pitch_summaries.len(),
            snapshot.pitch_error_logs.len()
        )
    };

    let kpi_row = row![
        metric_card(
            "Current Streak",
            format!("{} days", streak.current_streak_days),
            format!(
                "Best: {} days · {} sessions",
                streak.longest_streak_days, streak.total_sessions
            ),
            colors.sage,
            colors,
        ),
        metric_card(
            "Today's Progress",
            today_value,
            today_caption,
            if goal_met { colors.sage } else { colors.coral_text },
            colors,
        ),
        metric_card(
            "Mora Clarity",
            mora_value,
            mora_caption,
            colors.sage,
            colors,
        ),
        metric_card(
            "Avg Pitch Error",
            pitch_value,
            pitch_caption,
            colors.coral_text,
            colors,
        ),
    ]
    .spacing(14);

    let goal_ratio = streak.goal_progress_ratio();
    let goal_pct = (goal_ratio * 100.0).round() as u32;
    let goal_fraction = match streak.daily_goal {
        DailyGoal::Cards(target) => {
            format!("{} / {} cards", streak.cards_practiced_today, target)
        }
        DailyGoal::Minutes(target) => {
            format!("{} / {} min", streak.minutes_practiced_today, target)
        }
    };

    let mut goal_header = row![
        text("Daily Goal Completion")
            .size(16)
            .color(colors.text_primary),
        container(text(format!("{goal_fraction} ({goal_pct}%)")).size(12).color(
            if goal_met { colors.sage } else { colors.coral_text }
        ))
        .padding([4, 10])
        .style(theme::badge_style(
            if goal_met {
                colors.sage_soft
            } else {
                colors.coral_soft
            },
            if goal_met { colors.sage } else { colors.coral },
        )),
        space().width(Length::Fill),
    ]
    .spacing(10)
    .align_y(Alignment::Center);

    if !goal_met {
        goal_header = goal_header.push(
            button(text("Practice Now").size(13))
                .padding([6, 14])
                .style(theme::primary_button_style)
                .on_press(Message::SelectTab(NavTab::Practice)),
        );
    }

    let goal_card = container(
        column![
            goal_header,
            progress_bar(0.0..=1.0, goal_ratio)
                .length(Length::Fill)
                .girth(10)
                .style(theme::progress_bar_style(if goal_met {
                    colors.sage
                } else {
                    colors.coral
                })),
        ]
        .spacing(10),
    )
    .width(Length::Fill)
    .padding(18)
    .style(theme::card_style);

    column![streak_banner, kpi_row, goal_card]
        .spacing(14)
        .width(Length::Fill)
        .into()
}

fn render_accuracy_charts_card<'a>(
    state: &'a DashboardState,
    snapshot: &'a DashboardSnapshot,
    resolved_theme: ResolvedTheme,
    colors: &'static ThemeTokens,
) -> Element<'a, Message> {
    let today = Utc::now().date_naive();
    let series = snapshot.filtered_series(state.time_range, today);

    let mora_points: Vec<(String, f32)> = series
        .iter()
        .filter_map(|point| {
            point
                .avg_mora_accuracy_percent
                .map(|val| (point.date.format("%m/%d").to_string(), val))
        })
        .collect();

    let pitch_points: Vec<(String, f32)> = series
        .iter()
        .filter_map(|point| {
            point
                .avg_pitch_error_percent
                .map(|val| (point.date.format("%m/%d").to_string(), val))
        })
        .collect();

    let mora_latest_badge = mora_points
        .last()
        .map(|(_, val)| format!("Latest: {val:.1}%"))
        .unwrap_or_else(|| "No data".to_string());

    let pitch_latest_badge = pitch_points
        .last()
        .map(|(_, val)| format!("Latest: {val:.1}%"))
        .unwrap_or_else(|| "No data".to_string());

    let pitch_values: Vec<f32> = pitch_points.iter().map(|(_, v)| *v).collect();
    let pitch_y_max = compute_pitch_axis_max(&pitch_values);

    let range_pills = DashboardTimeRange::ALL.into_iter().fold(
        row![].spacing(8).align_y(Alignment::Center),
        |r, range| {
            r.push(
                button(text(range.label()).size(12))
                    .padding([6, 12])
                    .style(theme::pill_button_style(state.time_range == range))
                    .on_press(Message::DashboardSelectTimeRange(range)),
            )
        },
    );

    let mora_panel = container(
        column![
            row![
                text("Mora Clarity % (Higher is better)")
                    .size(14)
                    .color(colors.text_primary),
                space().width(Length::Fill),
                container(text(mora_latest_badge).size(12).color(colors.sage))
                    .padding([3, 9])
                    .style(theme::badge_style(colors.sage_soft, colors.sage)),
            ]
            .align_y(Alignment::Center),
            canvas(TrendChartProgram {
                points: mora_points,
                y_max: 100.0,
                accent: colors.sage,
                empty_message: "Complete Mora Purity or Shadowing drills to plot clarity %",
                resolved_theme,
            })
            .width(Length::Fill)
            .height(210),
        ]
        .spacing(10),
    )
    .width(Length::Fill)
    .padding(14)
    .style(theme::muted_card_style);

    let pitch_panel = container(
        column![
            row![
                text("Pitch Accent Error % (Lower is better)")
                    .size(14)
                    .color(colors.text_primary),
                space().width(Length::Fill),
                container(text(pitch_latest_badge).size(12).color(colors.coral_text))
                    .padding([3, 9])
                    .style(theme::badge_style(colors.coral_soft, colors.coral)),
            ]
            .align_y(Alignment::Center),
            canvas(TrendChartProgram {
                points: pitch_points,
                y_max: pitch_y_max,
                accent: colors.coral,
                empty_message: "Complete Pitch Accent or Shadowing drills to plot error %",
                resolved_theme,
            })
            .width(Length::Fill)
            .height(210),
        ]
        .spacing(10),
    )
    .width(Length::Fill)
    .padding(14)
    .style(theme::muted_card_style);

    container(
        column![
            row![
                column![
                    text("Accuracy Over Time")
                        .size(18)
                        .color(colors.text_primary),
                    text("Daily average mora clarity and pitch contour error percentages.")
                        .size(13)
                        .color(colors.text_muted),
                ]
                .spacing(4),
                space().width(Length::Fill),
                range_pills,
            ]
            .spacing(12)
            .align_y(Alignment::Center),
            row![mora_panel, pitch_panel].spacing(14),
        ]
        .spacing(14),
    )
    .width(Length::Fill)
    .padding(20)
    .style(theme::card_style)
    .into()
}

fn render_kana_breakdown_card<'a>(
    state: &'a DashboardState,
    snapshot: &'a DashboardSnapshot,
    colors: &'static ThemeTokens,
) -> Element<'a, Message> {
    let sort_pills = KanaSortOrder::ALL.into_iter().fold(
        row![].spacing(8).align_y(Alignment::Center),
        |r, order| {
            r.push(
                button(text(order.label()).size(12))
                    .padding([6, 12])
                    .style(theme::pill_button_style(state.kana_sort == order))
                    .on_press(Message::DashboardSelectKanaSort(order)),
            )
        },
    );

    let mut card_col = column![
        row![
            column![
                row![
                    text("Per-Kana Pronunciation Clarity")
                        .size(18)
                        .color(colors.text_primary),
                    container(
                        text(format!("{} tracked", snapshot.kana_stats.len()))
                            .size(12)
                            .color(colors.text_muted),
                    )
                    .padding([3, 9])
                    .style(theme::badge_style(colors.card_muted, colors.border)),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
                text("Exact accuracy % and top 5 most common substitutions per kana. Click a row to inspect details.")
                    .size(13)
                    .color(colors.text_muted),
            ]
            .spacing(4),
            space().width(Length::Fill),
            sort_pills,
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    ]
    .spacing(12);

    if snapshot.kana_stats.is_empty() {
        let example_line = sample_kana_breakdown();
        card_col = card_col.push(
            container(
                column![
                    row![
                        container(text("Example Preview").size(12).color(colors.amber))
                            .padding([3, 9])
                            .style(theme::badge_style(colors.amber_soft, colors.amber)),
                        text("No kana attempts recorded yet.")
                            .size(13)
                            .color(colors.text_muted),
                        space().width(Length::Fill),
                        button(text("Open Practice").size(13))
                            .padding([6, 14])
                            .style(theme::primary_button_style)
                            .on_press(Message::SelectTab(NavTab::Practice)),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center),
                    text(example_line).size(14).color(colors.text_primary),
                ]
                .spacing(8),
            )
            .width(Length::Fill)
            .padding(14)
            .style(theme::muted_card_style),
        );
        return container(card_col)
            .width(Length::Fill)
            .padding(20)
            .style(theme::card_style)
            .into();
    }

    card_col = card_col.push(
        row![
            text_input(
                "Filter kana or confused sound (e.g. つ, ツ, す)...",
                &state.kana_filter,
            )
            .on_input(Message::DashboardKanaFilterChanged)
            .padding([8, 12])
            .width(Length::Fill)
            .style(theme::text_input_style),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    );

    let filtered = state.filtered_kana_stats();
    let total_matching = filtered.len();

    if filtered.is_empty() {
        card_col = card_col.push(
            container(
                text(format!(
                    "No tracked kana matched \"{}\". Clear the search box to see all {} kana.",
                    state.kana_filter.trim(),
                    snapshot.kana_stats.len()
                ))
                .size(13)
                .color(colors.text_muted),
            )
            .width(Length::Fill)
            .padding(14)
            .style(theme::muted_card_style),
        );
        return container(card_col)
            .width(Length::Fill)
            .padding(20)
            .style(theme::card_style)
            .into();
    }

    let mut list_col = column![].spacing(10);
    for stats in filtered.iter().take(MAX_KANA_ROWS) {
        let is_selected = state.selected_kana.as_deref() == Some(stats.kana.as_str());
        list_col = list_col.push(render_kana_row(stats, is_selected, colors));
        if is_selected {
            list_col = list_col.push(render_kana_detail_panel(stats, colors));
        }
    }

    card_col = card_col.push(list_col);

    if total_matching > MAX_KANA_ROWS || total_matching < snapshot.kana_stats.len() {
        let shown = total_matching.min(MAX_KANA_ROWS);
        card_col = card_col.push(
            text(format!(
                "Showing {} of {} tracked kana.",
                shown,
                snapshot.kana_stats.len()
            ))
            .size(12)
            .color(colors.text_muted),
        );
    }

    container(card_col)
        .width(Length::Fill)
        .padding(20)
        .style(theme::card_style)
        .into()
}

fn render_kana_row<'a>(
    stats: &'a KanaStats,
    is_selected: bool,
    colors: &'static ThemeTokens,
) -> Element<'a, Message> {
    let accuracy = stats.accuracy_percent();
    let (acc_fg, acc_bg, acc_border) = if accuracy >= 85.0 {
        (colors.sage, colors.sage_soft, colors.sage)
    } else if accuracy >= 65.0 {
        (colors.amber, colors.amber_soft, colors.amber)
    } else {
        (colors.danger, colors.danger_soft, colors.danger)
    };

    let mistakes = stats.top_mistakes(5);
    let mut mistake_pills = row![].spacing(6).align_y(Alignment::Center);
    if mistakes.is_empty() {
        mistake_pills = mistake_pills.push(
            container(text("No substitutions").size(11).color(colors.sage))
                .padding([3, 8])
                .style(theme::badge_style(colors.sage_soft, colors.sage)),
        );
    } else {
        for entry in &mistakes {
            mistake_pills = mistake_pills.push(
                container(
                    text(format!(
                        "{}% {}",
                        entry.percentage.round() as u64,
                        entry.heard_kana
                    ))
                    .size(11)
                    .color(colors.text_primary),
                )
                .padding([3, 8])
                .style(theme::badge_style(colors.card_surface, colors.border)),
            );
        }
    }

    let next_selected = if is_selected {
        None
    } else {
        Some(stats.kana.clone())
    };

    button(
        column![
            row![
                text(&stats.kana).size(22).color(colors.text_primary),
                container(text(format!("{accuracy:.0}% correct")).size(12).color(acc_fg))
                    .padding([3, 9])
                    .style(theme::badge_style(acc_bg, acc_border)),
                text(format!(
                    "{}/{} attempts",
                    stats.correct_attempts, stats.total_attempts
                ))
                .size(12)
                .color(colors.text_muted),
                space().width(Length::Fill),
                mistake_pills,
            ]
            .spacing(10)
            .align_y(Alignment::Center),
            text(stats.format_breakdown())
                .size(13)
                .color(colors.text_muted),
            progress_bar(0.0..=100.0, accuracy.clamp(0.0, 100.0))
                .length(Length::Fill)
                .girth(6)
                .style(theme::progress_bar_style(acc_fg)),
        ]
        .spacing(8),
    )
    .width(Length::Fill)
    .padding(14)
    .style(theme::selectable_row_button_style(is_selected))
    .on_press(Message::DashboardSelectKana(next_selected))
    .into()
}

fn render_kana_detail_panel<'a>(
    stats: &'a KanaStats,
    colors: &'static ThemeTokens,
) -> Element<'a, Message> {
    let rule = mora_equivalence_rule(&stats.kana);
    let realizations = rule
        .valid_realizations
        .iter()
        .map(|r| format!("[{}] {}", r.ipa, r.label))
        .collect::<Vec<_>>()
        .join(" · ");
    let devoicing_note = if rule.allows_vowel_devoicing {
        "High-vowel devoicing accepted when a voiceless consonant holds the full mora beat."
    } else {
        "Standard voiced mora timing expected."
    };

    let mut detail_col = column![
        row![
            text(format!("{} — Top 5 Substitutions & Phonetic Rules", stats.kana))
                .size(14)
                .color(colors.text_primary),
            space().width(Length::Fill),
            text(format!(
                "{} correct / {} total",
                stats.correct_attempts, stats.total_attempts
            ))
            .size(12)
            .color(colors.sage),
        ]
        .align_y(Alignment::Center),
    ]
    .spacing(10);

    let mistakes = stats.top_mistakes(5);
    if mistakes.is_empty() {
        detail_col = detail_col.push(
            text("Every recorded attempt for this kana matched accepted native realizations.")
                .size(13)
                .color(colors.sage),
        );
    } else {
        for entry in mistakes {
            detail_col = detail_col.push(
                column![
                    row![
                        text(format!("Heard as \"{}\"", entry.heard_kana))
                            .size(13)
                            .color(colors.text_primary),
                        space().width(Length::Fill),
                        text(format!("{} times ({:.1}%)", entry.count, entry.percentage))
                            .size(12)
                            .color(colors.coral_text),
                    ]
                    .align_y(Alignment::Center),
                    progress_bar(0.0..=100.0, entry.percentage.clamp(0.0, 100.0))
                        .length(Length::Fill)
                        .girth(6)
                        .style(theme::progress_bar_style(colors.coral)),
                ]
                .spacing(4),
            );
        }
    }

    if !realizations.is_empty() {
        detail_col = detail_col.push(
            text(format!("Accepted realizations: {realizations}"))
                .size(12)
                .color(colors.text_muted),
        );
    }
    detail_col = detail_col.push(text(devoicing_note).size(12).color(colors.text_muted));

    container(detail_col)
        .width(Length::Fill)
        .padding(14)
        .style(theme::muted_card_style)
        .into()
}

fn render_card_pitch_history_card<'a>(
    state: &'a DashboardState,
    snapshot: &'a DashboardSnapshot,
    resolved_theme: ResolvedTheme,
    colors: &'static ThemeTokens,
) -> Element<'a, Message> {
    let avg_badge = snapshot
        .overall_pitch_error_percent
        .map_or_else(|| "No pitch data".to_string(), |v| format!("Avg {v:.1}% error"));

    let mut card_col = column![
        row![
            column![
                row![
                    text("Per-Card Pitch Accent Error History")
                        .size(18)
                        .color(colors.text_primary),
                    container(
                        text(format!(
                            "{} cards · {}",
                            snapshot.card_pitch_summaries.len(),
                            avg_badge
                        ))
                        .size(12)
                        .color(colors.coral_text),
                    )
                    .padding([3, 9])
                    .style(theme::badge_style(colors.coral_soft, colors.coral)),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
                text("Review cards ordered by highest recent pitch error %. Click a card to view its DTW error trend.")
                    .size(13)
                    .color(colors.text_muted),
            ]
            .spacing(4),
        ]
        .align_y(Alignment::Center),
    ]
    .spacing(12);

    if snapshot.card_pitch_summaries.is_empty() {
        card_col = card_col.push(
            container(
                row![
                    text("No pitch-scored reviews recorded yet. Practice in Pitch Accent or Combined Shadowing mode to track per-card pitch error %.")
                        .size(13)
                        .color(colors.text_muted),
                    space().width(Length::Fill),
                    button(text("Open Practice").size(13))
                        .padding([6, 14])
                        .style(theme::secondary_button_style)
                        .on_press(Message::SelectTab(NavTab::Practice)),
                ]
                .spacing(12)
                .align_y(Alignment::Center),
            )
            .width(Length::Fill)
            .padding(14)
            .style(theme::muted_card_style),
        );
        return container(card_col)
            .width(Length::Fill)
            .padding(20)
            .style(theme::card_style)
            .into();
    }

    card_col = card_col.push(
        text_input(
            "Filter cards by word, kana, or deck...",
            &state.pitch_card_filter,
        )
        .on_input(Message::DashboardPitchCardFilterChanged)
        .padding([8, 12])
        .width(Length::Fill)
        .style(theme::text_input_style),
    );

    let filtered = state.filtered_card_pitch_summaries();
    let total_matching = filtered.len();

    if filtered.is_empty() {
        card_col = card_col.push(
            container(
                text(format!(
                    "No reviewed cards matched \"{}\".",
                    state.pitch_card_filter.trim()
                ))
                .size(13)
                .color(colors.text_muted),
            )
            .width(Length::Fill)
            .padding(14)
            .style(theme::muted_card_style),
        );
        return container(card_col)
            .width(Length::Fill)
            .padding(20)
            .style(theme::card_style)
            .into();
    }

    let mut list_col = column![].spacing(10);
    for summary in filtered.iter().take(MAX_PITCH_CARD_ROWS) {
        let is_selected = state.selected_pitch_card_id == Some(summary.card_id);
        list_col = list_col.push(render_pitch_card_row(summary, is_selected, colors));
        if is_selected {
            let history = state.pitch_history_for_card(summary.card_id);
            list_col = list_col.push(render_pitch_card_detail_panel(
                summary,
                &history,
                resolved_theme,
                colors,
            ));
        }
    }

    card_col = card_col.push(list_col);

    if total_matching > MAX_PITCH_CARD_ROWS || total_matching < snapshot.card_pitch_summaries.len() {
        let shown = total_matching.min(MAX_PITCH_CARD_ROWS);
        card_col = card_col.push(
            text(format!(
                "Showing {} of {} pitch-tracked cards.",
                shown,
                snapshot.card_pitch_summaries.len()
            ))
            .size(12)
            .color(colors.text_muted),
        );
    }

    container(card_col)
        .width(Length::Fill)
        .padding(20)
        .style(theme::card_style)
        .into()
}

fn render_pitch_card_row<'a>(
    summary: &'a CardPitchSummary,
    is_selected: bool,
    colors: &'static ThemeTokens,
) -> Element<'a, Message> {
    let err_pct = summary.latest_pitch_error_percent;
    let (err_fg, err_bg, err_border) = if err_pct <= 15.0 {
        (colors.sage, colors.sage_soft, colors.sage)
    } else if err_pct <= 30.0 {
        (colors.amber, colors.amber_soft, colors.amber)
    } else {
        (colors.danger, colors.danger_soft, colors.danger)
    };

    let mut metrics_row = row![
        container(text(format!("Latest: {err_pct:.1}%")).size(12).color(err_fg))
            .padding([3, 9])
            .style(theme::badge_style(err_bg, err_border)),
        text(format!(
            "Avg {:.1}% · Best {:.1}%",
            summary.average_pitch_error_percent, summary.best_pitch_error_percent
        ))
        .size(12)
        .color(colors.text_muted),
    ]
    .spacing(10)
    .align_y(Alignment::Center);

    if let Some(delta) = summary.trend_delta_percent() {
        let (delta_text, delta_fg, delta_bg, delta_border) = if delta <= -0.5 {
            (
                format!("{delta:+.1}% improved"),
                colors.sage,
                colors.sage_soft,
                colors.sage,
            )
        } else if delta >= 0.5 {
            (
                format!("{delta:+.1}% higher"),
                colors.coral_text,
                colors.coral_soft,
                colors.coral,
            )
        } else {
            (
                "Steady".to_string(),
                colors.text_muted,
                colors.card_surface,
                colors.border,
            )
        };
        metrics_row = metrics_row.push(
            container(text(delta_text).size(11).color(delta_fg))
                .padding([3, 8])
                .style(theme::badge_style(delta_bg, delta_border)),
        );
    }

    let next_selected = if is_selected {
        None
    } else {
        Some(summary.card_id)
    };

    button(
        row![
            column![
                text(summary.display_label())
                    .size(15)
                    .color(colors.text_primary),
                text(format!(
                    "{} · {} review(s) · Last {}",
                    summary.deck_name,
                    summary.review_count,
                    summary.last_reviewed_at.format("%Y-%m-%d")
                ))
                .size(12)
                .color(colors.text_muted),
            ]
            .spacing(4),
            space().width(Length::Fill),
            metrics_row,
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding(14)
    .style(theme::selectable_row_button_style(is_selected))
    .on_press(Message::DashboardSelectPitchCard(next_selected))
    .into()
}

fn render_pitch_card_detail_panel<'a>(
    summary: &'a CardPitchSummary,
    history: &[&'a PitchErrorLogEntry],
    resolved_theme: ResolvedTheme,
    colors: &'static ThemeTokens,
) -> Element<'a, Message> {
    let mut detail_col = column![
        row![
            text(format!("{} — Pitch Review Log", summary.display_label()))
                .size(14)
                .color(colors.text_primary),
            space().width(Length::Fill),
            text(format!("Latest DTW: {:.2} st", summary.latest_dtw_distance))
                .size(12)
                .color(colors.text_muted),
        ]
        .align_y(Alignment::Center),
    ]
    .spacing(10);

    if summary.recent_errors.len() >= 2 {
        let points: Vec<(String, f32)> = summary
            .recent_errors
            .iter()
            .enumerate()
            .map(|(idx, &val)| (format!("#{}", idx + 1), val))
            .collect();
        let y_max = compute_pitch_axis_max(&summary.recent_errors);
        detail_col = detail_col.push(
            canvas(TrendChartProgram {
                points,
                y_max,
                accent: colors.coral,
                empty_message: "Need at least 2 reviews to plot card pitch trend",
                resolved_theme,
            })
            .width(Length::Fill)
            .height(120),
        );
    }

    let mut entries_col = column![].spacing(6);
    for entry in history.iter().take(MAX_CARD_HISTORY_ENTRIES) {
        let accuracy = (100.0 - entry.pitch_error_percent).clamp(0.0, 100.0);
        entries_col = entries_col.push(
            row![
                text(entry.reviewed_at.format("%Y-%m-%d %H:%M").to_string())
                    .size(12)
                    .color(colors.text_muted),
                space().width(Length::Fill),
                text(format!("Error: {:.1}%", entry.pitch_error_percent))
                    .size(12)
                    .color(colors.coral_text),
                text(format!("Accuracy: {accuracy:.1}%"))
                    .size(12)
                    .color(colors.sage),
                text(format!("DTW: {:.2} st", entry.dtw_distance))
                    .size(12)
                    .color(colors.text_muted),
            ]
            .spacing(14)
            .align_y(Alignment::Center),
        );
    }

    detail_col = detail_col.push(entries_col);

    container(detail_col)
        .width(Length::Fill)
        .padding(14)
        .style(theme::muted_card_style)
        .into()
}

fn render_export_card<'a>(
    snapshot: &'a DashboardSnapshot,
    export_status: Option<&'a str>,
    colors: &'static ThemeTokens,
) -> Element<'a, Message> {
    let summary_badge = format!(
        "{} reviews · {} kana · {} pitch logs",
        snapshot.total_reviews,
        snapshot.kana_stats.len(),
        snapshot.pitch_error_logs.len()
    );

    let mut export_col = column![
        row![
            text("Export Session History & Confusion Matrix")
                .size(17)
                .color(colors.text_primary),
            space().width(Length::Fill),
            container(text(summary_badge).size(12).color(colors.text_muted))
                .padding([3, 10])
                .style(theme::badge_style(colors.card_muted, colors.border)),
        ]
        .align_y(Alignment::Center),
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

    container(export_col)
        .width(Length::Fill)
        .padding(20)
        .style(theme::card_style)
        .into()
}

fn metric_card<'a>(
    title: &'a str,
    value: String,
    caption: String,
    caption_color: Color,
    colors: &'static ThemeTokens,
) -> Element<'a, Message> {
    container(
        column![
            text(title).size(13).color(colors.text_muted),
            text(value).size(24).color(colors.text_primary),
            text(caption).size(12).color(caption_color),
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

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn sample_time(day: u32) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, day, 12, 0, 0)
            .single()
            .unwrap_or_else(Utc::now)
    }

    fn kana_with_counts(kana: &str, correct: u64, mistake: Option<(&str, u64)>) -> KanaStats {
        let mut stats = KanaStats::new(kana);
        let now = sample_time(26);
        for _ in 0..correct {
            stats.record_attempt(&MoraAttempt {
                card_id: 1,
                expected_kana: kana.to_string(),
                heard_kana: Some(kana.to_string()),
                confidence: 0.9,
                recorded_at: now,
            });
        }
        if let Some((heard, count)) = mistake {
            for _ in 0..count {
                stats.record_attempt(&MoraAttempt {
                    card_id: 1,
                    expected_kana: kana.to_string(),
                    heard_kana: Some(heard.to_string()),
                    confidence: 0.4,
                    recorded_at: now,
                });
            }
        }
        stats
    }

    #[test]
    fn chart_geometry_helpers_handle_bounds_single_points_and_ticks() {
        assert!((compute_pitch_axis_max(&[]) - 40.0).abs() < 1e-5);
        assert!((compute_pitch_axis_max(&[12.0, 39.5]) - 40.0).abs() < 1e-5);
        assert!((compute_pitch_axis_max(&[45.0]) - 60.0).abs() < 1e-5);
        assert!((compute_pitch_axis_max(&[61.0, 79.0]) - 80.0).abs() < 1e-5);
        assert!((compute_pitch_axis_max(&[85.0, f32::NAN]) - 100.0).abs() < 1e-5);

        let plot = Rectangle {
            x: 40.0,
            y: 10.0,
            width: 200.0,
            height: 100.0,
        };
        assert!(map_trend_points(&[], plot, 0.0, 100.0).is_empty());

        let single = map_trend_points(&[50.0], plot, 0.0, 100.0);
        assert_eq!(single.len(), 1);
        assert!((single[0].x - 140.0).abs() < 1e-4);
        assert!((single[0].y - 60.0).abs() < 1e-4);

        let multi = map_trend_points(&[0.0, 120.0], plot, 0.0, 100.0);
        assert_eq!(multi.len(), 2);
        assert!((multi[0].x - 40.0).abs() < 1e-4);
        assert!((multi[0].y - 110.0).abs() < 1e-4);
        assert!((multi[1].x - 240.0).abs() < 1e-4);
        assert!((multi[1].y - 10.0).abs() < 1e-4);

        assert!(select_x_tick_indices(0).is_empty());
        assert_eq!(select_x_tick_indices(1), vec![0]);
        assert_eq!(select_x_tick_indices(2), vec![0, 1]);
        assert_eq!(select_x_tick_indices(4), vec![0, 1, 2, 3]);
        assert_eq!(select_x_tick_indices(9), vec![0, 4, 8]);
    }

    #[test]
    fn dashboard_state_filters_and_sorts_kana_and_pitch_summaries() {
        let tsu = kana_with_counts("つ", 8, Some(("す", 2)));
        let ka = kana_with_counts("か", 3, Some(("が", 2)));
        let a = kana_with_counts("あ", 20, None);

        let mut state = DashboardState {
            snapshot: DashboardSnapshot {
                kana_stats: vec![tsu, ka, a],
                card_pitch_summaries: vec![
                    CardPitchSummary {
                        card_id: 1,
                        deck_name: "Core 2k".to_string(),
                        word: "橋".to_string(),
                        kana: "はし".to_string(),
                        review_count: 2,
                        latest_pitch_error_percent: 25.0,
                        average_pitch_error_percent: 27.5,
                        best_pitch_error_percent: 25.0,
                        latest_dtw_distance: 1.3,
                        last_reviewed_at: sample_time(26),
                        recent_errors: vec![30.0, 25.0],
                    },
                    CardPitchSummary {
                        card_id: 2,
                        deck_name: "JLPT N4".to_string(),
                        word: "雨".to_string(),
                        kana: "あめ".to_string(),
                        review_count: 1,
                        latest_pitch_error_percent: 10.0,
                        average_pitch_error_percent: 10.0,
                        best_pitch_error_percent: 10.0,
                        latest_dtw_distance: 0.6,
                        last_reviewed_at: sample_time(27),
                        recent_errors: vec![10.0],
                    },
                ],
                pitch_error_logs: vec![
                    PitchErrorLogEntry {
                        review_id: 10,
                        card_id: 1,
                        deck_name: "Core 2k".to_string(),
                        word: "橋".to_string(),
                        kana: "はし".to_string(),
                        pitch_error_percent: 25.0,
                        dtw_distance: 1.3,
                        reviewed_at: sample_time(26),
                    },
                    PitchErrorLogEntry {
                        review_id: 11,
                        card_id: 2,
                        deck_name: "JLPT N4".to_string(),
                        word: "雨".to_string(),
                        kana: "あめ".to_string(),
                        pitch_error_percent: 10.0,
                        dtw_distance: 0.6,
                        reviewed_at: sample_time(27),
                    },
                ],
                ..DashboardSnapshot::default()
            },
            ..DashboardState::default()
        };

        let lowest: Vec<&str> = state
            .filtered_kana_stats()
            .into_iter()
            .map(|k| k.kana.as_str())
            .collect();
        assert_eq!(lowest, vec!["か", "つ", "あ"]);

        state.kana_sort = KanaSortOrder::MostPracticed;
        let practiced: Vec<&str> = state
            .filtered_kana_stats()
            .into_iter()
            .map(|k| k.kana.as_str())
            .collect();
        assert_eq!(practiced, vec!["あ", "つ", "か"]);

        state.kana_sort = KanaSortOrder::Alphabetical;
        let alpha: Vec<&str> = state
            .filtered_kana_stats()
            .into_iter()
            .map(|k| k.kana.as_str())
            .collect();
        assert_eq!(alpha, vec!["あ", "か", "つ"]);

        state.kana_filter = "ツ".to_string();
        assert_eq!(
            state
                .filtered_kana_stats()
                .into_iter()
                .map(|k| k.kana.as_str())
                .collect::<Vec<_>>(),
            vec!["つ"]
        );

        state.kana_filter = "す".to_string();
        assert_eq!(
            state
                .filtered_kana_stats()
                .into_iter()
                .map(|k| k.kana.as_str())
                .collect::<Vec<_>>(),
            vec!["つ"]
        );

        state.pitch_card_filter = "ハシ".to_string();
        let filtered_cards = state.filtered_card_pitch_summaries();
        assert_eq!(filtered_cards.len(), 1);
        assert_eq!(filtered_cards[0].card_id, 1);

        state.pitch_card_filter = "jlpt".to_string();
        let by_deck = state.filtered_card_pitch_summaries();
        assert_eq!(by_deck.len(), 1);
        assert_eq!(by_deck[0].card_id, 2);

        let card_1_logs = state.pitch_history_for_card(1);
        assert_eq!(card_1_logs.len(), 1);
        assert_eq!(card_1_logs[0].review_id, 10);
    }
}

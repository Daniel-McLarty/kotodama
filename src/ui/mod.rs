// SPDX-License-Identifier: MPL-2.0

/// Analytics dashboard views for streaks, per-kana clarity, and exports.
pub mod dashboard;
/// Pronunciation practice views for mora, pitch accent, and shadowing.
pub mod practice;
/// Deck setup, field mapping, and audio quality pre-flight views.
pub mod setup;
/// Warm, playful color palette and Iced widget style helpers.
pub mod theme;

use anyhow::Error;
use chrono::Utc;
use iced::alignment::{Horizontal, Vertical};
use iced::widget::{button, column, container, row, scrollable, space, stack, text, toggler};
use iced::{Alignment, Element, Length, Task, Theme};

use crate::models::{
    AppErrorToast, AppSettings, DailyGoal, DailyStreak, DeckConfig, NavTab, PlaybackSpeed,
    PracticeMode,
};

/// Top-level UI messages handled by [`KotodamaApp`].
#[derive(Debug, Clone)]
pub enum Message {
    /// Switches the active sidebar navigation tab.
    SelectTab(NavTab),
    /// Changes the selected pronunciation practice mode.
    SelectPracticeMode(PracticeMode),
    /// Enables or disables automatic trailing-silence recording stop.
    ToggleAutoStopVad(bool),
    /// Enables or disables Blind / Audio-Only card display.
    ToggleBlindMode(bool),
    /// Switches reference playback speed (`0.75x` or `1.0x`).
    SelectPlaybackSpeed(PlaybackSpeed),
    /// Updates the daily card goal preset.
    SetDailyGoalCards(u32),
    /// Simulates a recoverable feature/setup error surfaced as a dismissible toast.
    TriggerRecoverableAction,
    /// Triggers a sample recoverable error toast from Settings for testing UX.
    TriggerSampleError,
    /// Dismisses a single toast notification by ID.
    DismissToast(u64),
    /// Clears all currently visible toast notifications.
    ClearAllToasts,
}

/// Root Iced application state for Kotodama.
pub struct KotodamaApp {
    active_tab: NavTab,
    active_mode: PracticeMode,
    deck_config: Option<DeckConfig>,
    streak: DailyStreak,
    settings: AppSettings,
    toasts: Vec<AppErrorToast>,
    next_toast_id: u64,
}

impl Default for KotodamaApp {
    fn default() -> Self {
        let settings = AppSettings::default();
        let streak = DailyStreak {
            daily_goal: settings.daily_goal,
            ..DailyStreak::default()
        };
        Self {
            active_tab: NavTab::Setup,
            active_mode: PracticeMode::CombinedShadowing,
            deck_config: None,
            streak,
            settings,
            toasts: Vec::new(),
            next_toast_id: 1,
        }
    }
}

impl KotodamaApp {
    /// Returns the desktop window title.
    pub fn title(&self) -> String {
        format!("Kotodama — {}", self.active_tab.label())
    }

    /// Returns Kotodama's custom warm theme.
    pub fn theme(&self) -> Theme {
        theme::kotodama_theme()
    }

    /// Records a recoverable error into the dismissible toast stack.
    pub fn report_error(
        &mut self,
        title: impl Into<String>,
        error: &Error,
        recovery_hint: Option<String>,
    ) {
        let id = self.allocate_toast_id();
        self.toasts.push(AppErrorToast::error(
            id,
            title,
            format!("{error:#}"),
            recovery_hint,
            Utc::now(),
        ));
    }

    /// Records a non-fatal warning into the dismissible toast stack.
    pub fn report_warning(
        &mut self,
        title: impl Into<String>,
        message: impl Into<String>,
        recovery_hint: Option<String>,
    ) {
        let id = self.allocate_toast_id();
        self.toasts.push(AppErrorToast::warning(
            id,
            title,
            message,
            recovery_hint,
            Utc::now(),
        ));
    }

    /// Processes UI messages and updates application state without crashing on recoverable errors.
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::SelectTab(tab) => {
                self.active_tab = tab;
            }
            Message::SelectPracticeMode(mode) => {
                self.active_mode = mode;
            }
            Message::ToggleAutoStopVad(enabled) => {
                self.settings.auto_stop_vad = enabled;
            }
            Message::ToggleBlindMode(enabled) => {
                self.settings.blind_mode = enabled;
            }
            Message::SelectPlaybackSpeed(speed) => {
                self.settings.playback_speed = speed;
            }
            Message::SetDailyGoalCards(cards) => {
                let goal = DailyGoal::Cards(cards);
                self.settings.daily_goal = goal;
                self.streak.daily_goal = goal;
            }
            Message::TriggerRecoverableAction => {
                self.report_warning(
                    "Connect an Anki Deck First",
                    "Anki collection ingestion and audio capture are scheduled for Phases 2 and 3.",
                    Some("Pick a collection.anki2 file once Phase 2 lands, or preview settings now.".to_string()),
                );
            }
            Message::TriggerSampleError => {
                let err = anyhow::anyhow!(
                    "Could not open collection.anki2: no deck file has been selected yet."
                );
                self.report_error(
                    "Deck Path Unavailable",
                    &err,
                    Some("Choose a valid collection.anki2 file or retry AnkiConnect.".to_string()),
                );
            }
            Message::DismissToast(id) => {
                self.toasts.retain(|toast| toast.id != id);
            }
            Message::ClearAllToasts => {
                self.toasts.clear();
            }
        }
        Task::none()
    }

    /// Builds the root Iced widget tree including navigation sidebar, active view, and toast overlay.
    pub fn view(&self) -> Element<'_, Message> {
        let sidebar = self.render_sidebar();
        let active_content = match self.active_tab {
            NavTab::Setup => setup::view(self.deck_config.as_ref()),
            NavTab::Practice => practice::view(
                self.active_mode,
                &self.settings,
                &self.streak,
                self.deck_config.is_some(),
            ),
            NavTab::Dashboard => dashboard::view(&self.streak),
            NavTab::Settings => self.render_settings(),
        };

        let header = column![
            text(self.active_tab.label())
                .size(26)
                .color(theme::WARM_SLATE),
            text(self.active_tab.subtitle())
                .size(14)
                .color(theme::MUTED_SLATE),
        ]
        .spacing(4);

        let main_pane = container(
            scrollable(
                column![header, active_content]
                    .spacing(20)
                    .padding(28)
                    .width(Length::Fill),
            )
            .width(Length::Fill)
            .height(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::app_shell_style);

        let shell = row![sidebar, main_pane]
            .width(Length::Fill)
            .height(Length::Fill);

        if self.toasts.is_empty() {
            shell.into()
        } else {
            let toast_overlay = self.render_toast_overlay();
            stack![shell, toast_overlay]
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        }
    }

    fn allocate_toast_id(&mut self) -> u64 {
        let id = self.next_toast_id;
        self.next_toast_id = self.next_toast_id.saturating_add(1);
        id
    }

    fn render_sidebar(&self) -> Element<'_, Message> {
        let nav_buttons = NavTab::ALL.iter().fold(column![].spacing(8), |col, &tab| {
            let is_active = tab == self.active_tab;
            col.push(
                button(text(tab.label()).size(15))
                    .width(Length::Fill)
                    .padding([12, 14])
                    .style(theme::nav_button_style(is_active))
                    .on_press(Message::SelectTab(tab)),
            )
        });

        let streak_pill = container(
            column![
                text(format!("{} day streak", self.streak.current_streak_days))
                    .size(14)
                    .color(theme::SAGE),
                text(self.settings.daily_goal.label())
                    .size(12)
                    .color(theme::MUTED_SLATE),
            ]
            .spacing(2),
        )
        .width(Length::Fill)
        .padding(12)
        .style(theme::muted_card_style);

        container(
            column![
                column![
                    text("言霊 Kotodama").size(22).color(theme::CORAL_HOVER),
                    text("Japanese Pronunciation")
                        .size(12)
                        .color(theme::MUTED_SLATE),
                ]
                .spacing(2),
                nav_buttons,
                space().height(Length::Fill),
                streak_pill,
                text(format!("v{}", env!("CARGO_PKG_VERSION")))
                    .size(11)
                    .color(theme::MUTED_SLATE),
            ]
            .spacing(20)
            .height(Length::Fill),
        )
        .width(220)
        .height(Length::Fill)
        .padding(18)
        .style(theme::sidebar_style)
        .into()
    }

    fn render_settings(&self) -> Element<'_, Message> {
        let speed_buttons = [PlaybackSpeed::Slow, PlaybackSpeed::Normal]
            .into_iter()
            .fold(row![].spacing(10), |r, speed| {
                r.push(
                    button(text(speed.label()).size(14))
                        .padding([8, 14])
                        .style(theme::pill_button_style(
                            self.settings.playback_speed == speed,
                        ))
                        .on_press(Message::SelectPlaybackSpeed(speed)),
                )
            });

        let goal_presets = [5_u32, 10, 20, 30].into_iter().fold(row![].spacing(10), |r, count| {
            let selected = self.settings.daily_goal == DailyGoal::Cards(count);
            r.push(
                button(text(format!("{count} cards")).size(14))
                    .padding([8, 14])
                    .style(theme::pill_button_style(selected))
                    .on_press(Message::SetDailyGoalCards(count)),
            )
        });

        let ergonomics_card = container(
            column![
                text("Practice Ergonomics").size(18).color(theme::WARM_SLATE),
                toggler(self.settings.auto_stop_vad)
                    .label(format!(
                        "Auto-stop recording after {}ms trailing silence (VAD)",
                        self.settings.vad_silence_ms
                    ))
                    .on_toggle(Message::ToggleAutoStopVad),
                toggler(self.settings.blind_mode)
                    .label("Blind / Audio-Only Mode (hide text until after speaking)")
                    .on_toggle(Message::ToggleBlindMode),
                column![
                    text("Reference Playback Speed")
                        .size(14)
                        .color(theme::MUTED_SLATE),
                    speed_buttons,
                ]
                .spacing(8),
                column![
                    text("Daily Session Goal").size(14).color(theme::MUTED_SLATE),
                    goal_presets,
                ]
                .spacing(8),
            ]
            .spacing(14),
        )
        .width(Length::Fill)
        .padding(20)
        .style(theme::card_style);

        let thresholds = &self.settings.score_thresholds;
        let fsrs_card = container(
            column![
                text("FSRS Score Thresholds").size(18).color(theme::WARM_SLATE),
                text(format!(
                    "Hard >= {:.0}% · Good >= {:.0}% · Easy >= {:.0}%",
                    thresholds.hard_min, thresholds.good_min, thresholds.easy_min
                ))
                .size(14)
                .color(theme::MUTED_SLATE),
                button(text("Preview Recoverable Error Toast").size(14))
                    .padding([10, 16])
                    .style(theme::secondary_button_style)
                    .on_press(Message::TriggerSampleError),
            ]
            .spacing(12),
        )
        .width(Length::Fill)
        .padding(20)
        .style(theme::card_style);

        column![ergonomics_card, fsrs_card]
            .spacing(18)
            .width(Length::Fill)
            .into()
    }

    fn render_toast_overlay(&self) -> Element<'_, Message> {
        let mut toast_col = column![].spacing(10).width(380);

        if self.toasts.len() > 1 {
            toast_col = toast_col.push(
                row![
                    space().width(Length::Fill),
                    button(text("Dismiss all").size(12))
                        .padding([6, 12])
                        .style(theme::secondary_button_style)
                        .on_press(Message::ClearAllToasts),
                ]
                .align_y(Alignment::Center),
            );
        }

        for toast in &self.toasts {
            let mut body = column![
                row![
                    text(&toast.title).size(15).color(theme::WARM_SLATE),
                    space().width(Length::Fill),
                    button(text("Dismiss").size(12))
                        .padding([4, 10])
                        .style(theme::secondary_button_style)
                        .on_press(Message::DismissToast(toast.id)),
                ]
                .align_y(Alignment::Center),
                text(&toast.message).size(13).color(theme::WARM_SLATE),
            ]
            .spacing(6);

            if let Some(hint) = &toast.recovery_hint {
                body = body.push(text(hint).size(12).color(theme::MUTED_SLATE));
            }

            toast_col = toast_col.push(
                container(body)
                    .width(Length::Fill)
                    .padding(14)
                    .style(theme::toast_card_style(toast.severity)),
            );
        }

        container(toast_col)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(20)
            .align_x(Horizontal::Right)
            .align_y(Vertical::Bottom)
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ErrorSeverity;

    #[test]
    fn update_switches_tabs_modes_and_settings() {
        let mut app = KotodamaApp::default();
        assert_eq!(app.active_tab, NavTab::Setup);

        let _ = app.update(Message::SelectTab(NavTab::Practice));
        assert_eq!(app.active_tab, NavTab::Practice);
        assert_eq!(app.title(), "Kotodama — Practice");

        let _ = app.update(Message::SelectPracticeMode(PracticeMode::PitchAccent));
        assert_eq!(app.active_mode, PracticeMode::PitchAccent);

        let _ = app.update(Message::ToggleAutoStopVad(false));
        let _ = app.update(Message::ToggleBlindMode(true));
        let _ = app.update(Message::SelectPlaybackSpeed(PlaybackSpeed::Slow));
        let _ = app.update(Message::SetDailyGoalCards(20));

        assert!(!app.settings.auto_stop_vad);
        assert!(app.settings.blind_mode);
        assert_eq!(app.settings.playback_speed, PlaybackSpeed::Slow);
        assert_eq!(app.settings.daily_goal, DailyGoal::Cards(20));
        assert_eq!(app.streak.daily_goal, DailyGoal::Cards(20));
    }

    #[test]
    fn recoverable_errors_create_and_dismiss_toasts_without_panicking() {
        let mut app = KotodamaApp::default();
        assert!(app.toasts.is_empty());

        let err = anyhow::anyhow!("Missing audio file test.mp3");
        app.report_error("Audio Missing", &err, Some("Pick another deck".to_string()));
        let _ = app.update(Message::TriggerSampleError);
        assert_eq!(app.toasts.len(), 2);
        assert_eq!(app.toasts[0].severity, ErrorSeverity::Error);

        let first_id = app.toasts[0].id;
        let _ = app.update(Message::DismissToast(first_id));
        assert_eq!(app.toasts.len(), 1);
        assert_ne!(app.toasts[0].id, first_id);

        let _ = app.update(Message::ClearAllToasts);
        assert!(app.toasts.is_empty());
    }
}

// SPDX-License-Identifier: MPL-2.0

/// Analytics dashboard views for streaks, per-kana clarity, and exports.
pub mod dashboard;
/// Pronunciation practice views for mora, pitch accent, and shadowing.
pub mod practice;
/// Deck setup, field mapping, and audio quality pre-flight views.
pub mod setup;
/// Warm, playful Light and Dark palettes and Iced widget style helpers.
pub mod theme;

use std::path::PathBuf;

use anyhow::Error;
use chrono::Utc;
use iced::alignment::{Horizontal, Vertical};
use iced::widget::{button, column, container, row, scrollable, space, stack, text, toggler};
use iced::{Alignment, Element, Length, Subscription, Task, Theme};

use crate::models::{
    AppErrorToast, AppSettings, DailyGoal, DailyStreak, DeckConfig, NavTab, PlaybackSpeed,
    PracticeMode, ResolvedTheme, ThemePreference,
};
use crate::storage;
use crate::ui::theme::ThemeTokens;

/// Top-level UI messages handled by [`KotodamaApp`].
#[derive(Debug, Clone)]
pub enum Message {
    /// Switches the active sidebar navigation tab.
    SelectTab(NavTab),
    /// Changes the selected pronunciation practice mode.
    SelectPracticeMode(PracticeMode),
    /// Updates the cached OS light/dark preference when detected or changed live.
    SystemThemeChanged(iced::theme::Mode),
    /// Sets the user's theme preference (`System`, `Light`, or `Dark`).
    SetThemePreference(ThemePreference),
    /// Cycles the quick theme toggle (`System -> Light -> Dark -> System`).
    CycleThemePreference,
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
    system_is_dark: Option<bool>,
    settings_path: Option<PathBuf>,
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
            system_is_dark: None,
            settings_path: None,
            toasts: Vec::new(),
            next_toast_id: 1,
        }
    }
}

impl KotodamaApp {
    /// Loads persisted local settings and queries the OS color scheme on startup.
    pub fn boot() -> (Self, Task<Message>) {
        let mut app = Self::default();

        match storage::default_settings_path() {
            Ok(path) => {
                match storage::load_settings(&path) {
                    Ok(loaded) => {
                        app.streak.daily_goal = loaded.daily_goal;
                        app.settings = loaded;
                    }
                    Err(err) => {
                        app.report_warning(
                            "Settings Reset to Defaults",
                            format!("{err:#}"),
                            Some("Default preferences are active; new changes will overwrite the file.".to_string()),
                        );
                    }
                }
                app.settings_path = Some(path);
            }
            Err(err) => {
                app.report_warning(
                    "Settings Persistence Unavailable",
                    format!("{err:#}"),
                    Some("Theme and practice changes will apply for this session only.".to_string()),
                );
            }
        }

        (app, iced::system::theme().map(Message::SystemThemeChanged))
    }

    /// Creates an application instance bound to an explicit settings file path.
    #[cfg(test)]
    fn with_settings_path(settings_path: PathBuf) -> Self {
        Self {
            settings_path: Some(settings_path),
            ..Self::default()
        }
    }

    /// Returns the desktop window title.
    pub fn title(&self) -> String {
        format!("Kotodama — {}", self.active_tab.label())
    }

    /// Resolves the concrete Light or Dark appearance from preference and OS state.
    pub fn resolved_theme(&self) -> ResolvedTheme {
        self.settings.theme_preference.resolve(self.system_is_dark)
    }

    /// Returns Kotodama's custom warm Light or Dark theme.
    pub fn theme(&self) -> Theme {
        theme::kotodama_theme(self.resolved_theme())
    }

    /// Subscribes to live OS light/dark theme change notifications.
    pub fn subscription(&self) -> Subscription<Message> {
        iced::system::theme_changes().map(Message::SystemThemeChanged)
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
            Message::SystemThemeChanged(mode) => {
                self.system_is_dark = match mode {
                    iced::theme::Mode::Dark => Some(true),
                    iced::theme::Mode::Light => Some(false),
                    iced::theme::Mode::None => None,
                };
            }
            Message::SetThemePreference(preference) => {
                self.settings.theme_preference = preference;
                self.persist_settings();
            }
            Message::CycleThemePreference => {
                self.settings.theme_preference = self.settings.theme_preference.cycle_next();
                self.persist_settings();
            }
            Message::ToggleAutoStopVad(enabled) => {
                self.settings.auto_stop_vad = enabled;
                self.persist_settings();
            }
            Message::ToggleBlindMode(enabled) => {
                self.settings.blind_mode = enabled;
                self.persist_settings();
            }
            Message::SelectPlaybackSpeed(speed) => {
                self.settings.playback_speed = speed;
                self.persist_settings();
            }
            Message::SetDailyGoalCards(cards) => {
                let goal = DailyGoal::Cards(cards);
                self.settings.daily_goal = goal;
                self.streak.daily_goal = goal;
                self.persist_settings();
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

    /// Builds the root Iced widget tree including navigation sidebar, top bar, active view, and toast overlay.
    pub fn view(&self) -> Element<'_, Message> {
        let resolved_theme = self.resolved_theme();
        let colors = theme::tokens(resolved_theme);

        let sidebar = self.render_sidebar(colors);
        let top_bar = self.render_top_bar(resolved_theme, colors);

        let active_content = match self.active_tab {
            NavTab::Setup => setup::view(self.deck_config.as_ref(), resolved_theme),
            NavTab::Practice => practice::view(
                self.active_mode,
                &self.settings,
                &self.streak,
                self.deck_config.is_some(),
                resolved_theme,
            ),
            NavTab::Dashboard => dashboard::view(&self.streak, resolved_theme),
            NavTab::Settings => self.render_settings(resolved_theme, colors),
        };

        let content_area = scrollable(
            column![active_content]
                .spacing(20)
                .padding(28)
                .width(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill);

        let main_pane = container(
            column![top_bar, content_area]
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
            let toast_overlay = self.render_toast_overlay(colors);
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

    fn persist_settings(&mut self) {
        let Some(path) = self.settings_path.as_deref() else {
            return;
        };

        if let Err(err) = storage::save_settings(path, &self.settings) {
            self.report_error(
                "Could Not Save Settings",
                &err,
                Some("Ensure your configuration directory is writable.".to_string()),
            );
        }
    }

    fn render_top_bar(
        &self,
        resolved_theme: ResolvedTheme,
        colors: &'static ThemeTokens,
    ) -> Element<'_, Message> {
        let toggle_label = match self.settings.theme_preference {
            ThemePreference::System => format!("Theme: System ({})", resolved_theme.label()),
            ThemePreference::Light => "Theme: Light".to_string(),
            ThemePreference::Dark => "Theme: Dark".to_string(),
        };

        container(
            row![
                column![
                    text(self.active_tab.label())
                        .size(22)
                        .color(colors.text_primary),
                    text(self.active_tab.subtitle())
                        .size(13)
                        .color(colors.text_muted),
                ]
                .spacing(2)
                .width(Length::Fill),
                button(text(toggle_label).size(13))
                    .padding([8, 14])
                    .style(theme::secondary_button_style)
                    .on_press(Message::CycleThemePreference),
            ]
            .spacing(16)
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .padding([16, 28])
        .style(theme::top_bar_style)
        .into()
    }

    fn render_sidebar(&self, colors: &'static ThemeTokens) -> Element<'_, Message> {
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
                    .color(colors.sage),
                text(self.settings.daily_goal.label())
                    .size(12)
                    .color(colors.text_muted),
            ]
            .spacing(2),
        )
        .width(Length::Fill)
        .padding(12)
        .style(theme::muted_card_style);

        container(
            column![
                column![
                    text("言霊 Kotodama").size(22).color(colors.coral_text),
                    text("Japanese Pronunciation")
                        .size(12)
                        .color(colors.text_muted),
                ]
                .spacing(2),
                nav_buttons,
                space().height(Length::Fill),
                streak_pill,
                text(format!("v{}", env!("CARGO_PKG_VERSION")))
                    .size(11)
                    .color(colors.text_muted),
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

    fn render_settings(
        &self,
        resolved_theme: ResolvedTheme,
        colors: &'static ThemeTokens,
    ) -> Element<'_, Message> {
        let theme_buttons = ThemePreference::ALL
            .into_iter()
            .fold(row![].spacing(10), |r, preference| {
                r.push(
                    button(text(preference.label()).size(14))
                        .padding([8, 16])
                        .style(theme::pill_button_style(
                            self.settings.theme_preference == preference,
                        ))
                        .on_press(Message::SetThemePreference(preference)),
                )
            });

        let appearance_status = match self.settings.theme_preference {
            ThemePreference::System => match self.system_is_dark {
                Some(_) => format!(
                    "Following OS color scheme (currently {} mode).",
                    resolved_theme.label()
                ),
                None => format!(
                    "Following OS color scheme (defaulting to {} until OS reports a preference).",
                    resolved_theme.label()
                ),
            },
            ThemePreference::Light => "Manual Light mode override active.".to_string(),
            ThemePreference::Dark => "Manual Dark mode override active.".to_string(),
        };

        let appearance_card = container(
            column![
                text("Appearance").size(18).color(colors.text_primary),
                text("Follow your operating system's Light/Dark setting automatically or pin a warm palette.")
                    .size(14)
                    .color(colors.text_muted),
                theme_buttons,
                text(appearance_status).size(13).color(colors.sage),
            ]
            .spacing(12),
        )
        .width(Length::Fill)
        .padding(20)
        .style(theme::card_style);

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
                text("Practice Ergonomics").size(18).color(colors.text_primary),
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
                        .color(colors.text_muted),
                    speed_buttons,
                ]
                .spacing(8),
                column![
                    text("Daily Session Goal").size(14).color(colors.text_muted),
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
                text("FSRS Score Thresholds").size(18).color(colors.text_primary),
                text(format!(
                    "Hard >= {:.0}% · Good >= {:.0}% · Easy >= {:.0}%",
                    thresholds.hard_min, thresholds.good_min, thresholds.easy_min
                ))
                .size(14)
                .color(colors.text_muted),
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

        column![appearance_card, ergonomics_card, fsrs_card]
            .spacing(18)
            .width(Length::Fill)
            .into()
    }

    fn render_toast_overlay(&self, colors: &'static ThemeTokens) -> Element<'_, Message> {
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
                    text(&toast.title).size(15).color(colors.text_primary),
                    space().width(Length::Fill),
                    button(text("Dismiss").size(12))
                        .padding([4, 10])
                        .style(theme::secondary_button_style)
                        .on_press(Message::DismissToast(toast.id)),
                ]
                .align_y(Alignment::Center),
                text(&toast.message).size(13).color(colors.text_primary),
            ]
            .spacing(6);

            if let Some(hint) = &toast.recovery_hint {
                body = body.push(text(hint).size(12).color(colors.text_muted));
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
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::models::ErrorSeverity;

    static UI_TEMP_COUNTER: AtomicU64 = AtomicU64::new(1);

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
    fn system_theme_updates_follow_os_only_when_preference_is_system() {
        let mut app = KotodamaApp::default();
        assert_eq!(app.resolved_theme(), ResolvedTheme::Light);

        let _ = app.update(Message::SystemThemeChanged(iced::theme::Mode::Dark));
        assert_eq!(app.resolved_theme(), ResolvedTheme::Dark);
        assert_eq!(
            app.theme().palette().background,
            theme::DARK_TOKENS.canvas_bg
        );

        let _ = app.update(Message::SetThemePreference(ThemePreference::Light));
        assert_eq!(app.resolved_theme(), ResolvedTheme::Light);

        let _ = app.update(Message::SystemThemeChanged(iced::theme::Mode::Dark));
        assert_eq!(app.resolved_theme(), ResolvedTheme::Light);

        let _ = app.update(Message::CycleThemePreference);
        assert_eq!(app.settings.theme_preference, ThemePreference::Dark);
        assert_eq!(app.resolved_theme(), ResolvedTheme::Dark);

        let _ = app.update(Message::CycleThemePreference);
        assert_eq!(app.settings.theme_preference, ThemePreference::System);
        assert_eq!(app.resolved_theme(), ResolvedTheme::Dark);
    }

    #[test]
    fn theme_preference_changes_persist_to_settings_file() -> anyhow::Result<()> {
        let id = UI_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "kotodama_ui_theme_test_{}_{}",
            std::process::id(),
            id
        ));
        let path = dir.join("settings.json");

        let mut app = KotodamaApp::with_settings_path(path.clone());
        let _ = app.update(Message::SetThemePreference(ThemePreference::Dark));
        assert!(app.toasts.is_empty());

        let loaded = storage::load_settings(&path)?;
        assert_eq!(loaded.theme_preference, ThemePreference::Dark);

        let _ = fs::remove_dir_all(&dir);
        Ok(())
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

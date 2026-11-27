// SPDX-License-Identifier: MPL-2.0

/// Analytics dashboard views for streaks, per-kana clarity, and exports.
pub mod dashboard;
/// Pronunciation practice views for mora, pitch accent, and shadowing.
pub mod practice;
/// Deck setup, field mapping, and audio quality pre-flight views.
pub mod setup;
/// Warm, playful Light and Dark palettes and Iced widget style helpers.
pub mod theme;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Error;
use chrono::Utc;
use iced::alignment::{Horizontal, Vertical};
use iced::widget::{
    button, canvas, column, container, pick_list, row, scrollable, slider, space, stack, text,
    toggler,
};
use iced::{
    Alignment, Color, Element, Length, Point, Rectangle, Renderer, Size, Subscription, Task, Theme,
    mouse,
};

use crate::anki::{reader, validator};
use crate::audio::capture::{self, MicMonitorHandle, RecordingSession};
use crate::audio::{dsp, playback};
use crate::models::{
    AnkiCard, AnkiCollectionSummary, AnkiRawNote, AppErrorToast, AppSettings, CsvExportReport,
    DailyGoal, DailyStreak, DashboardSnapshot, DashboardTimeRange, DecodedAudio, DeckAudioReport,
    DeckConfig, DeckWorkspaceConfig, FsrsCardState, FsrsRating, FsrsSchedulingOutcome,
    InputDeviceInfo, KanaSortOrder, MicLevelSnapshot, NavTab, PitchExtractionConfig,
    PlaybackSpeed, PracticeDeckStatus, PracticeEvaluationResult, PracticeMicViewState,
    PracticeMode, PracticeShortcutAction, PracticeTargetClip, RecordedTake,
    ReferenceContourPreview, ResolvedTheme, ScoreThresholds, ThemePreference, VadConfig,
};
use crate::scoring::fsrs_bridge::FsrsScheduler;
use crate::scoring::mora::{HeuristicAcousticMoraScorer, MoraScorer};
use crate::scoring::pitch;
use crate::storage::{self, db::KotodamaDb, export};
use crate::ui::dashboard::DashboardState;
use crate::ui::practice::{PracticePhase, PracticeState};
use crate::ui::setup::{NONE_FIELD_OPTION, SetupSourceMode, SetupState};
use crate::ui::theme::ThemeTokens;

const SAMPLE_PREVIEW_LIMIT: usize = 25;
const SYSTEM_DEFAULT_MIC_OPTION: &str = "System Default";

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
    /// Updates the daily practice duration goal preset in minutes.
    SetDailyGoalMinutes(u32),
    /// Updates the score-to-FSRS rating thresholds.
    SetScoreThresholds(ScoreThresholds),
    /// Prompts for a folder and exports review history, kana stats, and pitch logs to CSV.
    ExportCsvRequested,
    /// Receives the outcome of exporting database records to CSV files.
    CsvExportCompleted(Result<Option<CsvExportReport>, String>),
    /// Prompts for a file path and exports the full local database snapshot to JSON.
    ExportJsonRequested,
    /// Receives the outcome of exporting the full local database snapshot to JSON.
    JsonExportCompleted(Result<Option<PathBuf>, String>),
    /// Refreshes the list of available microphone input devices.
    RefreshInputDevices,
    /// Receives the enumerated list of microphone input devices.
    InputDevicesLoaded(Result<Vec<InputDeviceInfo>, String>),
    /// Selects a preferred microphone input device (`System Default` clears override).
    SelectInputDevice(String),
    /// Adjusts the Voice Activity Detection RMS speech sensitivity threshold.
    SetVadRmsThreshold(f32),
    /// Adjusts the trailing silence auto-stop duration preset in milliseconds.
    SetVadSilenceMs(u64),
    /// Starts or stops the live microphone calibration level monitor in Settings.
    ToggleMicCalibration,
    /// Polls live microphone monitor or active recording telemetry on a timer tick.
    MicMonitorTick,
    /// Starts or stops a short microphone test recording in Settings.
    ToggleTestRecording,
    /// Plays back the most recently recorded Settings test take.
    PlayTestTake,
    /// Receives the completion result of playing the Settings test take.
    TestTakePlaybackFinished(Result<(), String>),
    /// Switches between local `collection.anki2` and AnkiConnect setup modes.
    SelectSetupSourceMode(SetupSourceMode),
    /// Updates the local `collection.anki2` path input.
    CollectionPathChanged(String),
    /// Updates the local `collection.media` directory input.
    MediaDirChanged(String),
    /// Updates the AnkiConnect HTTP endpoint URL input.
    AnkiConnectUrlChanged(String),
    /// Opens a native file dialog to pick `collection.anki2`.
    BrowseCollectionFile,
    /// Receives the result of picking `collection.anki2`.
    CollectionFilePicked(Result<Option<PathBuf>, String>),
    /// Opens a native folder dialog to pick `collection.media`.
    BrowseMediaDir,
    /// Receives the result of picking `collection.media`.
    MediaDirPicked(Result<Option<PathBuf>, String>),
    /// Starts inspecting the selected local collection or AnkiConnect endpoint.
    LoadCollection,
    /// Receives inspected collection decks, note types, and optional media path.
    CollectionLoaded(Result<(AnkiCollectionSummary, Option<PathBuf>), String>),
    /// Selects a deck and loads sample graduated (`queue = 2`) notes for preview.
    SelectDeck(String),
    /// Receives sample learned notes for the selected deck.
    DeckSamplesLoaded(Result<Vec<AnkiRawNote>, String>),
    /// Updates the Target Word / Expression field mapping.
    MapWordField(String),
    /// Updates the Kana Reading field mapping.
    MapKanaField(String),
    /// Updates the optional Example Sentence field mapping.
    MapSentenceField(String),
    /// Updates the Word Audio field mapping.
    MapWordAudioField(String),
    /// Updates the optional Sentence Audio field mapping.
    MapSentenceAudioField(String),
    /// Moves to the previous sample card in the Setup live preview.
    PrevPreviewCard,
    /// Moves to the next sample card in the Setup live preview.
    NextPreviewCard,
    /// Plays the sample preview card's word or sentence reference audio clip.
    PlayPreviewAudio {
        /// Whether to play the optional sentence clip instead of word audio.
        sentence: bool,
    },
    /// Receives the completion result of playing a sample preview audio clip.
    AudioPlaybackFinished(Result<(), String>),
    /// Validates and imports all graduated cards for the mapped deck, then saves config.
    ValidateAndSaveDeck,
    /// Receives the validated deck config, learned cards, and GIGO audio report.
    DeckImportCompleted(Result<(DeckConfig, Vec<AnkiCard>, DeckAudioReport), String>),
    /// Excludes the currently previewed card from Kotodama practice queues.
    ExcludePreviewCard,
    /// Restores a previously excluded card back into active practice for `deck_name`.
    RestoreCard {
        /// Deck containing the excluded card.
        deck_name: String,
        /// Anki card ID to restore.
        card_id: i64,
    },
    /// Excludes all cards with flagged audio quality warnings for the active deck.
    AutoExcludeInvalidAudioCards,
    /// Restores all excluded cards for the given deck name.
    RestoreAllExcludedCards(String),
    /// Toggles visibility of the excluded-cards list in Setup.
    ToggleExcludedCardsList,
    /// Starts a pronunciation practice session (`extra_study` bypasses the due-only filter).
    PracticeStartSession {
        /// Whether to include all active cards instead of only due/unseen cards.
        extra_study: bool,
    },
    /// Receives loaded active cards, due queue order, pitch history, media dir, and initial FSRS context.
    #[allow(clippy::type_complexity)]
    PracticeDeckPrepared(
        Result<
            (
                Vec<AnkiCard>,
                Vec<i64>,
                BTreeMap<i64, f32>,
                Option<PathBuf>,
                Option<FsrsCardState>,
                [u32; 4],
            ),
            String,
        >,
    ),
    /// Ends the active practice session and returns to the Practice Lobby.
    PracticeEndSession,
    /// Continues practicing beyond the daily cap or due queue.
    PracticeContinueExtra,
    /// Switches between word and sentence audio for the active practice card.
    PracticeSelectTargetClip(PracticeTargetClip),
    /// Selects a target mora chip in Mora Purity or Combined Shadowing views.
    PracticeSelectMora(usize),
    /// Reveals masked Japanese text early while in Blind / Audio-Only Mode.
    PracticeRevealText,
    /// Enables or disables automatic countdown recording after Combined Shadowing playback.
    PracticeToggleShadowAutoRecord(bool),
    /// Plays the reference clip and triggers the 3-2-1 Combined Shadowing countdown.
    PracticeStartShadowFlow,
    /// Decodes (if needed) and plays the active card's reference audio clip.
    PracticePlayReference,
    /// Receives decoded reference audio and optional precomputed `F0` contour.
    PracticeReferenceDecoded {
        /// Card generation token preventing stale updates across card switches.
        generation: u64,
        /// Decoded PCM audio and optional normalized pitch contour.
        result: Result<(DecodedAudio, Option<ReferenceContourPreview>), String>,
    },
    /// Receives completion notification for reference audio playback.
    PracticeReferencePlaybackFinished {
        /// Card generation token matching the clip that finished playing.
        generation: u64,
        /// Playback completion status.
        result: Result<(), String>,
    },
    /// Starts or stops Practice microphone capture.
    PracticeToggleRecording,
    /// Receives background mora and/or pitch scoring results for a completed take.
    PracticeTakeEvaluated {
        /// Take generation token preventing stale evaluation updates.
        take_generation: u64,
        /// Evaluated mora and/or pitch scoring result.
        result: Result<PracticeEvaluationResult, String>,
    },
    /// Plays back the user's most recent recorded take at the active playback speed.
    PracticePlayUserTake,
    /// Receives completion notification for playing the user's recorded take.
    PracticeUserTakePlaybackFinished(Result<(), String>),
    /// Submits an FSRS rating (`Again`, `Hard`, `Good`, `Easy`) for the active card.
    PracticeSubmitRating {
        /// Selected FSRS rating.
        rating: FsrsRating,
        /// Whether this rating came from a manual user choice rather than auto-accept.
        is_manual: bool,
    },
    /// Receives the persisted FSRS scheduling outcome and updated streak after committing a review.
    PracticeReviewCommitted {
        /// Card generation token ensuring the commit belongs to the active card.
        card_generation: u64,
        /// Whether the applied rating differed from the automated recommendation.
        was_manual_override: bool,
        /// Scheduling outcome, updated daily streak, and latest pitch error percentage.
        result: Result<(FsrsSchedulingOutcome, DailyStreak, Option<f32>), String>,
    },
    /// Receives FSRS state and prospective day intervals for the current practice card.
    PracticeCardContextLoaded {
        /// Card generation token matching the requested card.
        card_generation: u64,
        /// Existing FSRS state and prospective `[Again, Hard, Good, Easy]` intervals in days.
        result: Result<(Option<FsrsCardState>, [u32; 4]), String>,
    },
    /// Skips the current practice card without recording an FSRS review.
    PracticeSkipCard,
    /// Dispatches a keyboard shortcut captured while the Practice tab is active.
    PracticeKeyShortcut(PracticeShortcutAction),
    /// Increments the active practice session timer once per second.
    PracticeSessionTick,
    /// Triggers a sample recoverable error toast from Settings for testing UX.
    TriggerSampleError,
    /// Requests a fresh SQLite analytics snapshot for the Dashboard view.
    DashboardRefreshRequested,
    /// Receives a loaded analytics snapshot for the Dashboard view.
    DashboardLoaded(Result<DashboardSnapshot, String>),
    /// Changes the trend chart calendar window on the Dashboard.
    DashboardSelectTimeRange(DashboardTimeRange),
    /// Changes the sorting order of the per-kana breakdown list on the Dashboard.
    DashboardSelectKanaSort(KanaSortOrder),
    /// Updates the search filter for per-kana clarity rows on the Dashboard.
    DashboardKanaFilterChanged(String),
    /// Expands or collapses a specific kana breakdown detail panel on the Dashboard.
    DashboardSelectKana(Option<String>),
    /// Updates the search filter for per-card pitch history rows on the Dashboard.
    DashboardPitchCardFilterChanged(String),
    /// Expands or collapses a specific card's pitch error history panel on the Dashboard.
    DashboardSelectPitchCard(Option<i64>),
    /// Dismisses a single toast notification by ID.
    DismissToast(u64),
    /// Clears all currently visible toast notifications.
    ClearAllToasts,
}

/// Root Iced application state for Kotodama.
pub struct KotodamaApp {
    active_tab: NavTab,
    active_mode: PracticeMode,
    deck_workspace: DeckWorkspaceConfig,
    loaded_cards: Vec<AnkiCard>,
    setup_state: SetupState,
    practice_state: PracticeState,
    dashboard_state: DashboardState,
    practice_recording: Option<RecordingSession>,
    practice_mic_level: MicLevelSnapshot,
    streak: DailyStreak,
    settings: AppSettings,
    input_devices: Vec<InputDeviceInfo>,
    mic_monitor: Option<MicMonitorHandle>,
    mic_recording: Option<RecordingSession>,
    mic_level: MicLevelSnapshot,
    last_test_take: Option<RecordedTake>,
    is_playing_test_take: bool,
    system_is_dark: Option<bool>,
    settings_path: Option<PathBuf>,
    deck_config_path: Option<PathBuf>,
    db_path: Option<PathBuf>,
    last_export_status: Option<String>,
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
            deck_workspace: DeckWorkspaceConfig::default(),
            loaded_cards: Vec::new(),
            setup_state: SetupState::default(),
            practice_state: PracticeState::default(),
            dashboard_state: DashboardState::default(),
            practice_recording: None,
            practice_mic_level: MicLevelSnapshot::default(),
            streak,
            settings,
            input_devices: Vec::new(),
            mic_monitor: None,
            mic_recording: None,
            mic_level: MicLevelSnapshot::default(),
            last_test_take: None,
            is_playing_test_take: false,
            system_is_dark: None,
            settings_path: None,
            deck_config_path: None,
            db_path: None,
            last_export_status: None,
            toasts: Vec::new(),
            next_toast_id: 1,
        }
    }
}

impl KotodamaApp {
    /// Loads persisted local settings and deck workspace, then queries the OS color scheme on startup.
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

        match storage::default_deck_config_path() {
            Ok(path) => {
                match storage::load_deck_workspace(&path) {
                    Ok(workspace) => {
                        app.setup_state.apply_workspace(&workspace);
                        app.deck_workspace = workspace;
                    }
                    Err(err) => {
                        app.report_warning(
                            "Deck Configuration Reset",
                            format!("{err:#}"),
                            Some("Re-select your Anki deck in Setup to save a fresh configuration.".to_string()),
                        );
                    }
                }
                app.deck_config_path = Some(path);
            }
            Err(err) => {
                app.report_warning(
                    "Deck Persistence Unavailable",
                    format!("{err:#}"),
                    Some("Deck field mappings will apply for this session only.".to_string()),
                );
            }
        }

        match storage::default_database_path() {
            Ok(db_path) => match KotodamaDb::open(&db_path) {
                Ok(db) => {
                    match db.load_dashboard_snapshot() {
                        Ok(mut snapshot) => {
                            snapshot.streak.daily_goal = app.settings.daily_goal;
                            app.streak = snapshot.streak.clone();
                            app.dashboard_state.snapshot = snapshot;
                            app.dashboard_state.has_loaded = true;
                        }
                        Err(err) => {
                            app.report_warning(
                                "Dashboard History Unavailable",
                                format!("{err:#}"),
                                Some("Daily streak counters and analytics will refresh once the database is readable.".to_string()),
                            );
                        }
                    }
                    app.db_path = Some(db_path);
                }
                Err(err) => {
                    app.report_warning(
                        "Database Initialization Warning",
                        format!("{err:#}"),
                        Some("FSRS scheduling and review history persistence are unavailable.".to_string()),
                    );
                }
            },
            Err(err) => {
                app.report_warning(
                    "Local Database Unavailable",
                    format!("{err:#}"),
                    Some("Analytics exports and local FSRS persistence require a writable data directory.".to_string()),
                );
            }
        }

        (
            app,
            Task::batch([
                iced::system::theme().map(Message::SystemThemeChanged),
                Task::perform(
                    run_blocking(capture::list_input_devices),
                    Message::InputDevicesLoaded,
                ),
            ]),
        )
    }

    /// Creates an application instance bound to explicit config paths for testing.
    #[cfg(test)]
    fn with_config_paths(settings_path: PathBuf, deck_config_path: PathBuf) -> Self {
        Self {
            settings_path: Some(settings_path),
            deck_config_path: Some(deck_config_path),
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

    /// Subscribes to live OS light/dark theme change notifications, active mic telemetry, session timers, and Practice shortcuts.
    pub fn subscription(&self) -> Subscription<Message> {
        let mut subs = vec![iced::system::theme_changes().map(Message::SystemThemeChanged)];

        if self.mic_monitor.is_some()
            || self.mic_recording.is_some()
            || self.practice_recording.is_some()
            || self.practice_state.countdown.is_some()
        {
            subs.push(
                iced::time::every(Duration::from_millis(50)).map(|_| Message::MicMonitorTick),
            );
        }

        if self.practice_state.phase == PracticePhase::ActiveCard {
            subs.push(
                iced::time::every(Duration::from_secs(1)).map(|_| Message::PracticeSessionTick),
            );
        }

        if self.active_tab == NavTab::Practice {
            subs.push(iced::event::listen_with(practice_keyboard_event));
        }

        Subscription::batch(subs)
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
                if self.active_tab == NavTab::Settings && tab != NavTab::Settings {
                    self.stop_mic_activity();
                }
                if self.active_tab == NavTab::Practice && tab != NavTab::Practice {
                    self.stop_practice_audio_and_countdown();
                }
                self.active_tab = tab;
                if tab == NavTab::Dashboard {
                    return self.launch_dashboard_refresh();
                }
            }
            Message::SelectPracticeMode(mode) => {
                self.active_mode = mode;
                if self.practice_state.phase == PracticePhase::ActiveCard {
                    self.stop_practice_audio_and_countdown();
                    self.practice_state
                        .prepare_current_card(self.active_mode, self.settings.blind_mode);
                    return self.launch_active_card_background_tasks();
                }
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
                if !enabled {
                    self.practice_state.text_revealed = true;
                }
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
                self.dashboard_state.snapshot.streak.daily_goal = goal;
                self.persist_settings();
            }
            Message::SetDailyGoalMinutes(minutes) => {
                let goal = DailyGoal::Minutes(minutes);
                self.settings.daily_goal = goal;
                self.streak.daily_goal = goal;
                self.dashboard_state.snapshot.streak.daily_goal = goal;
                self.persist_settings();
            }
            Message::SetScoreThresholds(thresholds) => match thresholds.validate() {
                Ok(()) => {
                    self.settings.score_thresholds = thresholds;
                    self.persist_settings();
                }
                Err(err) => {
                    self.report_error(
                        "Invalid FSRS Thresholds",
                        &err,
                        Some("Ensure Hard < Good < Easy within 0%..=100%.".to_string()),
                    );
                }
            },
            Message::ExportCsvRequested => {
                let db_path = match self
                    .db_path
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("Local database path is unavailable."))
                    .or_else(|_| storage::default_database_path())
                {
                    Ok(path) => path,
                    Err(err) => {
                        self.report_error(
                            "CSV Export Unavailable",
                            &err,
                            Some("Ensure your system data directory is accessible.".to_string()),
                        );
                        return Task::none();
                    }
                };

                return Task::perform(
                    async move {
                        let picked = rfd::AsyncFileDialog::new()
                            .set_title("Choose Folder for Kotodama CSV Exports")
                            .pick_folder()
                            .await;
                        let Some(folder) = picked else {
                            return Ok(None);
                        };
                        let output_dir = folder.path().to_path_buf();
                        run_blocking(move || {
                            let db = KotodamaDb::open(&db_path)?;
                            export::export_db_to_csv_directory(&db, &output_dir)
                        })
                        .await
                        .map(Some)
                    },
                    Message::CsvExportCompleted,
                );
            }
            Message::CsvExportCompleted(result) => match result {
                Ok(Some(report)) => {
                    let dir_display = report
                        .reviews_path
                        .parent()
                        .map_or_else(|| report.reviews_path.display().to_string(), |p| p.display().to_string());
                    self.last_export_status = Some(format!(
                        "Exported CSV files to {dir_display} ({} reviews, {} kana rows, {} pitch rows).",
                        report.review_rows, report.kana_rows, report.pitch_rows
                    ));
                }
                Ok(None) => {}
                Err(err_msg) => {
                    let err = anyhow::anyhow!(err_msg);
                    self.report_error(
                        "CSV Export Failed",
                        &err,
                        Some("Choose a writable folder and retry exporting.".to_string()),
                    );
                }
            },
            Message::ExportJsonRequested => {
                let db_path = match self
                    .db_path
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("Local database path is unavailable."))
                    .or_else(|_| storage::default_database_path())
                {
                    Ok(path) => path,
                    Err(err) => {
                        self.report_error(
                            "JSON Export Unavailable",
                            &err,
                            Some("Ensure your system data directory is accessible.".to_string()),
                        );
                        return Task::none();
                    }
                };

                return Task::perform(
                    async move {
                        let picked = rfd::AsyncFileDialog::new()
                            .set_title("Export Kotodama Database as JSON")
                            .set_file_name("kotodama_export.json")
                            .add_filter("JSON", &["json"])
                            .save_file()
                            .await;
                        let Some(file) = picked else {
                            return Ok(None);
                        };
                        let out_path = file.path().to_path_buf();
                        run_blocking(move || {
                            let db = KotodamaDb::open(&db_path)?;
                            export::export_db_to_json(&db, &out_path)?;
                            Ok(out_path)
                        })
                        .await
                        .map(Some)
                    },
                    Message::JsonExportCompleted,
                );
            }
            Message::JsonExportCompleted(result) => match result {
                Ok(Some(path)) => {
                    self.last_export_status =
                        Some(format!("Exported full JSON snapshot to {}.", path.display()));
                }
                Ok(None) => {}
                Err(err_msg) => {
                    let err = anyhow::anyhow!(err_msg);
                    self.report_error(
                        "JSON Export Failed",
                        &err,
                        Some("Choose a writable destination file and retry exporting.".to_string()),
                    );
                }
            },
            Message::RefreshInputDevices => {
                return Task::perform(
                    run_blocking(capture::list_input_devices),
                    Message::InputDevicesLoaded,
                );
            }
            Message::InputDevicesLoaded(result) => match result {
                Ok(devices) => {
                    self.input_devices = devices;
                }
                Err(err_msg) => {
                    let err = anyhow::anyhow!(err_msg);
                    self.report_error(
                        "Microphone Detection Failed",
                        &err,
                        Some("Connect a microphone and click Refresh Mics.".to_string()),
                    );
                }
            },
            Message::SelectInputDevice(choice) => {
                let preferred = if choice == SYSTEM_DEFAULT_MIC_OPTION {
                    None
                } else {
                    Some(choice)
                };
                self.settings.preferred_input_device = preferred;
                self.persist_settings();

                if self.mic_monitor.is_some() {
                    self.mic_monitor = None;
                    self.mic_level = MicLevelSnapshot::default();
                    match MicMonitorHandle::start(
                        self.settings.preferred_input_device.as_deref(),
                        self.settings.vad_rms_threshold,
                    ) {
                        Ok(handle) => {
                            self.mic_monitor = Some(handle);
                        }
                        Err(err) => {
                            self.report_error(
                                "Microphone Unavailable",
                                &err,
                                Some("Pick System Default or click Refresh Mics.".to_string()),
                            );
                        }
                    }
                }
            }
            Message::SetVadRmsThreshold(value) => {
                let clamped = value.clamp(0.005, 0.15);
                self.settings.vad_rms_threshold = clamped;
                if let Some(monitor) = &self.mic_monitor {
                    monitor.set_vad_threshold(clamped);
                }
                self.persist_settings();
            }
            Message::SetVadSilenceMs(ms) => {
                self.settings.vad_silence_ms = ms;
                self.persist_settings();
            }
            Message::ToggleMicCalibration => {
                if let Some(session) = self.mic_recording.take() {
                    drop(session);
                }
                if let Some(mut monitor) = self.mic_monitor.take() {
                    monitor.stop();
                    self.mic_level = MicLevelSnapshot::default();
                } else {
                    match MicMonitorHandle::start(
                        self.settings.preferred_input_device.as_deref(),
                        self.settings.vad_rms_threshold,
                    ) {
                        Ok(handle) => {
                            self.mic_level = handle.snapshot();
                            self.mic_monitor = Some(handle);
                        }
                        Err(err) => {
                            self.report_error(
                                "Could Not Start Mic Calibration",
                                &err,
                                Some("Verify your microphone is plugged in and permissions allow audio capture.".to_string()),
                            );
                        }
                    }
                }
            }
            Message::MicMonitorTick => {
                if let Some(session) = self.mic_recording.as_ref() {
                    self.mic_level = session.snapshot();
                    if let Some(err_msg) = session.take_error() {
                        self.mic_recording = None;
                        self.mic_level = MicLevelSnapshot::default();
                        let err = anyhow::anyhow!(err_msg);
                        self.report_error(
                            "Microphone Stream Interrupted",
                            &err,
                            Some("Check your microphone connection and try again.".to_string()),
                        );
                    } else if session.is_finished()
                        && let Some(finished_session) = self.mic_recording.take()
                    {
                        self.finalize_test_recording(finished_session);
                    }
                } else if let Some(monitor) = self.mic_monitor.as_ref() {
                    self.mic_level = monitor.snapshot();
                    if let Some(err_msg) = monitor.take_error() {
                        self.mic_monitor = None;
                        self.mic_level = MicLevelSnapshot::default();
                        let err = anyhow::anyhow!(err_msg);
                        self.report_error(
                            "Microphone Monitor Interrupted",
                            &err,
                            Some("Reconnect your microphone or switch input devices.".to_string()),
                        );
                    }
                }

                if let Some(session) = self.practice_recording.as_ref() {
                    self.practice_mic_level = session.snapshot();
                    if let Some(err_msg) = session.take_error() {
                        self.practice_recording = None;
                        self.practice_mic_level = MicLevelSnapshot::default();
                        let err = anyhow::anyhow!(err_msg);
                        self.report_error(
                            "Practice Microphone Interrupted",
                            &err,
                            Some("Check your microphone connection and press R to record again.".to_string()),
                        );
                    } else if session.is_finished()
                        && let Some(finished_session) = self.practice_recording.take()
                    {
                        return self.finalize_practice_recording(finished_session);
                    }
                }

                if self.practice_state.advance_countdown(Instant::now())
                    && self.active_tab == NavTab::Practice
                    && self.practice_state.phase == PracticePhase::ActiveCard
                    && self.practice_recording.is_none()
                {
                    self.start_practice_recording();
                }
            }
            Message::ToggleTestRecording => {
                if let Some(mut monitor) = self.mic_monitor.take() {
                    monitor.stop();
                }
                if let Some(session) = self.mic_recording.take() {
                    self.finalize_test_recording(session);
                } else {
                    let vad_config = VadConfig::from_settings(&self.settings);
                    match RecordingSession::start(
                        self.settings.preferred_input_device.as_deref(),
                        vad_config,
                    ) {
                        Ok(session) => {
                            self.mic_level = session.snapshot();
                            self.mic_recording = Some(session);
                        }
                        Err(err) => {
                            self.report_error(
                                "Could Not Start Test Recording",
                                &err,
                                Some("Check your microphone selection in Settings and try again.".to_string()),
                            );
                        }
                    }
                }
            }
            Message::PlayTestTake => {
                let Some(take) = self.last_test_take.as_ref() else {
                    self.report_warning(
                        "No Test Recording Yet",
                        "Record a short test clip first to preview playback.",
                        None,
                    );
                    return Task::none();
                };
                let audio = if take.trimmed_audio.samples.is_empty() {
                    take.raw_audio.clone()
                } else {
                    take.trimmed_audio.clone()
                };
                if audio.samples.is_empty() {
                    self.report_warning(
                        "Empty Test Recording",
                        "The recorded clip contained zero audio samples.",
                        Some("Speak louder or adjust your VAD threshold and try again.".to_string()),
                    );
                    return Task::none();
                }
                let speed = self.settings.playback_speed;
                self.is_playing_test_take = true;
                return Task::perform(
                    run_blocking(move || playback::play_decoded_audio(audio, speed)),
                    Message::TestTakePlaybackFinished,
                );
            }
            Message::TestTakePlaybackFinished(result) => {
                self.is_playing_test_take = false;
                if let Err(err_msg) = result {
                    let err = anyhow::anyhow!(err_msg);
                    self.report_error(
                        "Test Playback Failed",
                        &err,
                        Some("Verify your output speaker or headphone device is connected.".to_string()),
                    );
                }
            }
            Message::SelectSetupSourceMode(mode) => {
                self.setup_state.source_mode = mode;
            }
            Message::CollectionPathChanged(path) => {
                self.update_collection_path(path);
            }
            Message::MediaDirChanged(path) => {
                self.setup_state.media_dir_input = path;
                self.setup_state.refresh_preview_audio_issues();
            }
            Message::AnkiConnectUrlChanged(url) => {
                self.setup_state.ankiconnect_url_input = url;
            }
            Message::BrowseCollectionFile => {
                return Task::perform(
                    async {
                        let picked = rfd::AsyncFileDialog::new()
                            .set_title("Select Anki collection.anki2")
                            .add_filter("Anki Collection", &["anki2"])
                            .pick_file()
                            .await;
                        Ok(picked.map(|handle| handle.path().to_path_buf()))
                    },
                    Message::CollectionFilePicked,
                );
            }
            Message::CollectionFilePicked(result) => match result {
                Ok(Some(path)) => {
                    self.update_collection_path(path.display().to_string());
                }
                Ok(None) => {}
                Err(err_msg) => {
                    let err = anyhow::anyhow!(err_msg);
                    self.report_error(
                        "File Picker Unavailable",
                        &err,
                        Some("You can paste the path to collection.anki2 directly into the text field.".to_string()),
                    );
                }
            },
            Message::BrowseMediaDir => {
                return Task::perform(
                    async {
                        let picked = rfd::AsyncFileDialog::new()
                            .set_title("Select collection.media Folder")
                            .pick_folder()
                            .await;
                        Ok(picked.map(|handle| handle.path().to_path_buf()))
                    },
                    Message::MediaDirPicked,
                );
            }
            Message::MediaDirPicked(result) => match result {
                Ok(Some(path)) => {
                    self.setup_state.media_dir_input = path.display().to_string();
                    self.setup_state.refresh_preview_audio_issues();
                }
                Ok(None) => {}
                Err(err_msg) => {
                    let err = anyhow::anyhow!(err_msg);
                    self.report_error(
                        "Folder Picker Unavailable",
                        &err,
                        Some("Enter the collection.media directory path directly in the text field.".to_string()),
                    );
                }
            },
            Message::LoadCollection => {
                let source = match self.setup_state.build_deck_source() {
                    Ok(source) => source,
                    Err(err) => {
                        self.report_error(
                            "Invalid Anki Source",
                            &err,
                            Some("Select a valid collection.anki2 file or check your AnkiConnect URL.".to_string()),
                        );
                        return Task::none();
                    }
                };

                self.setup_state.is_busy = true;
                self.setup_state.busy_label = Some("Inspecting Anki collection...".to_string());

                return Task::perform(
                    run_blocking(move || reader::inspect_collection(&source)),
                    Message::CollectionLoaded,
                );
            }
            Message::CollectionLoaded(result) => {
                self.setup_state.is_busy = false;
                self.setup_state.busy_label = None;

                match result {
                    Ok((summary, discovered_media)) => {
                        let empty_decks = summary.decks.is_empty();
                        self.setup_state.set_collection(summary, discovered_media);
                        if empty_decks {
                            self.report_warning(
                                "No Decks Found",
                                "The selected Anki collection did not report any decks.",
                                Some("Verify you opened the right profile's collection.anki2.".to_string()),
                            );
                        } else if let Some(selected_deck) = self.setup_state.selected_deck_name.clone() {
                            return self.dispatch_load_samples(selected_deck);
                        }
                    }
                    Err(err_msg) => {
                        let err = anyhow::anyhow!(err_msg);
                        self.report_error(
                            "Could Not Load Anki Collection",
                            &err,
                            Some("Check the collection.anki2 path or confirm Anki is running with AnkiConnect.".to_string()),
                        );
                    }
                }
            }
            Message::SelectDeck(deck_name) => {
                self.setup_state.selected_deck_name = Some(deck_name.clone());
                self.setup_state.audio_report = None;
                return self.dispatch_load_samples(deck_name);
            }
            Message::DeckSamplesLoaded(result) => {
                self.setup_state.is_busy = false;
                self.setup_state.busy_label = None;

                match result {
                    Ok(notes) => {
                        let no_notes = notes.is_empty();
                        self.setup_state
                            .set_sample_notes(notes, &self.deck_workspace.saved_mappings);
                        if no_notes && let Some(deck) = &self.setup_state.selected_deck_name {
                            self.report_warning(
                                "No Learned Cards Found",
                                format!("Deck '{deck}' currently has no graduated cards (queue = 2)."),
                                Some("Study new cards in Anki until they graduate, or choose another deck.".to_string()),
                            );
                        }
                    }
                    Err(err_msg) => {
                        let err = anyhow::anyhow!(err_msg);
                        self.report_error(
                            "Could Not Load Deck Notes",
                            &err,
                            Some("Choose another deck or reload the Anki collection.".to_string()),
                        );
                    }
                }
            }
            Message::MapWordField(field) => {
                self.setup_state.draft_mapping.word_field = field;
                self.sync_active_mapping_changes();
            }
            Message::MapKanaField(field) => {
                self.setup_state.draft_mapping.kana_field = field;
                self.sync_active_mapping_changes();
            }
            Message::MapSentenceField(field) => {
                self.setup_state.draft_mapping.sentence_field = normalize_optional_field(field);
                self.sync_active_mapping_changes();
            }
            Message::MapWordAudioField(field) => {
                self.setup_state.draft_mapping.word_audio_field = field;
                self.sync_active_mapping_changes();
            }
            Message::MapSentenceAudioField(field) => {
                self.setup_state.draft_mapping.sentence_audio_field =
                    normalize_optional_field(field);
                self.sync_active_mapping_changes();
            }
            Message::PrevPreviewCard => {
                if self.setup_state.sample_index > 0 {
                    self.setup_state.sample_index -= 1;
                    self.setup_state.refresh_preview_audio_issues();
                }
            }
            Message::NextPreviewCard => {
                if self.setup_state.sample_index + 1 < self.setup_state.sample_notes.len() {
                    self.setup_state.sample_index += 1;
                    self.setup_state.refresh_preview_audio_issues();
                }
            }
            Message::PlayPreviewAudio { sentence } => {
                return self.handle_play_preview_audio(sentence);
            }
            Message::AudioPlaybackFinished(result) => {
                self.setup_state.is_playing_audio = false;
                if let Err(err_msg) = result {
                    let err = anyhow::anyhow!(err_msg);
                    self.report_error(
                        "Audio Playback Failed",
                        &err,
                        Some("Verify your audio clip exists in collection.media and your output device is connected.".to_string()),
                    );
                }
            }
            Message::ValidateAndSaveDeck => {
                return self.handle_validate_and_save_deck();
            }
            Message::DeckImportCompleted(result) => {
                self.setup_state.is_busy = false;
                self.setup_state.busy_label = None;

                match result {
                    Ok((config, cards, report)) => {
                        self.deck_workspace
                            .saved_mappings
                            .insert(config.deck_name.clone(), config.field_mapping.clone());
                        self.deck_workspace.active_deck = Some(config);
                        self.loaded_cards = cards;
                        self.setup_state.audio_report = Some(report.clone());
                        self.persist_deck_workspace();

                        if report.has_issues() {
                            self.report_warning(
                                "Audio Quality Warnings Detected",
                                report.summary_line(),
                                Some("Use Auto-Exclude Invalid Audio Cards in Setup or replace flagged clips in Anki.".to_string()),
                            );
                        }
                    }
                    Err(err_msg) => {
                        let err = anyhow::anyhow!(err_msg);
                        self.report_error(
                            "Deck Import Failed",
                            &err,
                            Some("Check your field mappings, collection path, and media directory, then retry.".to_string()),
                        );
                    }
                }
            }
            Message::ExcludePreviewCard => {
                let Some(deck_name) = self.active_or_selected_deck_name().map(str::to_string) else {
                    return Task::none();
                };
                let Some(card) = self.setup_state.current_preview_card() else {
                    return Task::none();
                };
                if self.deck_workspace.exclude_card(
                    &deck_name,
                    card.card_id,
                    Some(card.display_label()),
                ) {
                    self.persist_deck_workspace();
                }
            }
            Message::RestoreCard { deck_name, card_id } => {
                if self.deck_workspace.restore_card(&deck_name, card_id) {
                    if self.deck_workspace.excluded_count_for_deck(&deck_name) == 0 {
                        self.setup_state.show_excluded_list = false;
                    }
                    self.persist_deck_workspace();
                }
            }
            Message::AutoExcludeInvalidAudioCards => {
                let Some(deck_name) = self.active_or_selected_deck_name().map(str::to_string) else {
                    return Task::none();
                };
                let added = self
                    .deck_workspace
                    .exclude_invalid_audio_cards(&deck_name, &self.loaded_cards);
                if added > 0 {
                    self.persist_deck_workspace();
                } else {
                    self.report_warning(
                        "No New Invalid Cards",
                        "All flagged cards for this deck are already excluded or no invalid clips were found.",
                        None,
                    );
                }
            }
            Message::RestoreAllExcludedCards(deck_name) => {
                if self.deck_workspace.restore_all_for_deck(&deck_name) > 0 {
                    self.setup_state.show_excluded_list = false;
                    self.persist_deck_workspace();
                }
            }
            Message::ToggleExcludedCardsList => {
                self.setup_state.show_excluded_list = !self.setup_state.show_excluded_list;
            }
            Message::PracticeStartSession { extra_study } => {
                return self.handle_practice_start_session(extra_study);
            }
            Message::PracticeDeckPrepared(result) => {
                return self.handle_practice_deck_prepared(result);
            }
            Message::PracticeEndSession => {
                self.stop_practice_audio_and_countdown();
                self.practice_state.reset_to_lobby();
            }
            Message::PracticeContinueExtra => {
                self.practice_state.cap_override_active = true;
                if self.practice_state.current_index < self.practice_state.queue.len() {
                    self.practice_state.phase = PracticePhase::ActiveCard;
                    self.practice_state
                        .prepare_current_card(self.active_mode, self.settings.blind_mode);
                    return self.launch_active_card_background_tasks();
                }
                return self.handle_practice_start_session(true);
            }
            Message::PracticeSelectTargetClip(clip) => {
                if self.practice_state.phase == PracticePhase::ActiveCard
                    && self.practice_state.target_clip != clip
                {
                    self.stop_practice_audio_and_countdown();
                    self.practice_state.switch_target_clip(clip);
                    return self.launch_reference_decode_task();
                }
            }
            Message::PracticeSelectMora(index) => {
                self.practice_state.selected_mora_index = Some(index);
            }
            Message::PracticeRevealText => {
                self.practice_state.text_revealed = true;
            }
            Message::PracticeToggleShadowAutoRecord(enabled) => {
                self.practice_state.shadow_auto_record = enabled;
                if !enabled {
                    self.practice_state.cancel_countdown();
                }
            }
            Message::PracticeStartShadowFlow => {
                if self.practice_state.phase == PracticePhase::ActiveCard
                    && self.practice_recording.is_none()
                    && !self.practice_state.is_playing_reference
                {
                    self.practice_state.cancel_countdown();
                    self.practice_state.awaiting_shadow_countdown = true;
                    return self.handle_practice_play_reference();
                }
            }
            Message::PracticePlayReference => {
                if self.practice_state.phase == PracticePhase::ActiveCard
                    && self.practice_recording.is_none()
                    && !self.practice_state.is_playing_reference
                {
                    if self.active_mode == PracticeMode::CombinedShadowing
                        && self.practice_state.shadow_auto_record
                    {
                        self.practice_state.awaiting_shadow_countdown = true;
                    }
                    return self.handle_practice_play_reference();
                }
            }
            Message::PracticeReferenceDecoded { generation, result } => {
                if generation != self.practice_state.card_generation {
                    return Task::none();
                }
                self.practice_state.is_decoding_reference = false;
                match result {
                    Ok((decoded, contour)) => {
                        self.practice_state.reference_audio = Some(decoded);
                        self.practice_state.reference_contour = contour;
                        self.practice_state.reference_audio_error = None;
                    }
                    Err(err_msg) => {
                        self.practice_state.reference_audio_error = Some(err_msg);
                    }
                }
            }
            Message::PracticeReferencePlaybackFinished { generation, result } => {
                self.practice_state.is_playing_reference = false;
                if generation != self.practice_state.card_generation {
                    return Task::none();
                }
                if let Err(err_msg) = result {
                    self.practice_state.awaiting_shadow_countdown = false;
                    let err = anyhow::anyhow!(err_msg);
                    self.report_error(
                        "Reference Playback Failed",
                        &err,
                        Some("Verify your media directory and audio output device are connected.".to_string()),
                    );
                } else if self.practice_state.awaiting_shadow_countdown
                    && self.active_tab == NavTab::Practice
                    && self.practice_state.phase == PracticePhase::ActiveCard
                {
                    self.practice_state.start_countdown(Instant::now());
                }
            }
            Message::PracticeToggleRecording => {
                if self.practice_state.phase != PracticePhase::ActiveCard {
                    return Task::none();
                }
                if let Some(session) = self.practice_recording.take() {
                    return self.finalize_practice_recording(session);
                }
                self.practice_state.cancel_countdown();
                self.start_practice_recording();
            }
            Message::PracticeTakeEvaluated {
                take_generation,
                result,
            } => {
                if take_generation != self.practice_state.take_generation {
                    return Task::none();
                }
                self.practice_state.is_scoring = false;
                match result {
                    Ok(evaluation) => {
                        self.practice_state.last_evaluation = Some(evaluation);
                    }
                    Err(err_msg) => {
                        let err = anyhow::anyhow!(err_msg);
                        self.report_error(
                            "Pronunciation Scoring Failed",
                            &err,
                            Some("Speak clearly into the microphone or re-record your take.".to_string()),
                        );
                    }
                }
            }
            Message::PracticePlayUserTake => {
                if self.practice_state.phase != PracticePhase::ActiveCard
                    || self.practice_recording.is_some()
                    || self.practice_state.is_playing_user_take
                {
                    return Task::none();
                }
                let Some(take) = self.practice_state.last_take.as_ref() else {
                    self.report_warning(
                        "No Recorded Take Yet",
                        "Press R to record your pronunciation first, then press T to play it back.",
                        None,
                    );
                    return Task::none();
                };
                let audio = if take.trimmed_audio.samples.is_empty() {
                    take.raw_audio.clone()
                } else {
                    take.trimmed_audio.clone()
                };
                if audio.samples.is_empty() {
                    self.report_warning(
                        "Empty Recording",
                        "The recorded take contained zero playable samples.",
                        Some("Check your microphone input level in Settings and try again.".to_string()),
                    );
                    return Task::none();
                }
                self.practice_state.is_playing_user_take = true;
                let speed = self.settings.playback_speed;
                return Task::perform(
                    run_blocking(move || playback::play_decoded_audio(audio, speed)),
                    Message::PracticeUserTakePlaybackFinished,
                );
            }
            Message::PracticeUserTakePlaybackFinished(result) => {
                self.practice_state.is_playing_user_take = false;
                if let Err(err_msg) = result {
                    let err = anyhow::anyhow!(err_msg);
                    self.report_error(
                        "Take Playback Failed",
                        &err,
                        Some("Verify your audio output device is connected.".to_string()),
                    );
                }
            }
            Message::PracticeSubmitRating { rating, is_manual } => {
                return self.handle_practice_submit_rating(rating, is_manual);
            }
            Message::PracticeReviewCommitted {
                card_generation,
                was_manual_override,
                result,
            } => {
                if card_generation != self.practice_state.card_generation {
                    return Task::none();
                }
                self.practice_state.is_committing = false;
                match result {
                    Ok((outcome, streak, latest_pitch)) => {
                        self.streak = streak;
                        self.dashboard_state.snapshot.streak = self.streak.clone();
                        if let Some(pitch_err) = latest_pitch
                            && let Some(card) = self.practice_state.current_card()
                        {
                            let card_id = card.card_id;
                            for loaded in &mut self.loaded_cards {
                                if loaded.card_id == card_id {
                                    loaded.last_pitch_error_percent = Some(pitch_err);
                                }
                            }
                            for queued in &mut self.practice_state.queue {
                                if queued.card_id == card_id {
                                    queued.last_pitch_error_percent = Some(pitch_err);
                                }
                            }
                        }

                        let eval_snapshot = self.practice_state.last_evaluation.clone();
                        self.practice_state.record_committed_card(
                            eval_snapshot.as_ref(),
                            was_manual_override,
                            outcome.applied_rating,
                        );
                        self.practice_state.current_index =
                            self.practice_state.current_index.saturating_add(1);

                        if !self.practice_state.cap_override_active
                            && self.streak.is_daily_goal_met()
                        {
                            self.practice_state.summary.daily_cap_reached = true;
                            self.practice_state.phase = PracticePhase::SessionComplete {
                                cap_reached: true,
                            };
                            self.practice_state.card_started_at = None;
                            return Task::none();
                        }

                        if self.practice_state.current_index >= self.practice_state.queue.len() {
                            self.practice_state.phase = PracticePhase::SessionComplete {
                                cap_reached: false,
                            };
                            self.practice_state.card_started_at = None;
                            return Task::none();
                        }

                        self.practice_state
                            .prepare_current_card(self.active_mode, self.settings.blind_mode);
                        return self.launch_active_card_background_tasks();
                    }
                    Err(err_msg) => {
                        let err = anyhow::anyhow!(err_msg);
                        self.report_error(
                            "Could Not Save Review",
                            &err,
                            Some("Ensure your local Kotodama database is writable and retry rating.".to_string()),
                        );
                    }
                }
            }
            Message::PracticeCardContextLoaded {
                card_generation,
                result,
            } => {
                if card_generation != self.practice_state.card_generation {
                    return Task::none();
                }
                if let Ok((state, previews)) = result {
                    self.practice_state.current_fsrs_state = state;
                    self.practice_state.preview_intervals_days = Some(previews);
                }
            }
            Message::PracticeSkipCard => {
                if self.practice_state.phase != PracticePhase::ActiveCard
                    || self.practice_state.is_committing
                {
                    return Task::none();
                }
                self.stop_practice_audio_and_countdown();
                self.practice_state.current_index =
                    self.practice_state.current_index.saturating_add(1);
                if self.practice_state.current_index >= self.practice_state.queue.len() {
                    self.practice_state.phase = PracticePhase::SessionComplete {
                        cap_reached: false,
                    };
                    self.practice_state.card_started_at = None;
                    return Task::none();
                }
                self.practice_state
                    .prepare_current_card(self.active_mode, self.settings.blind_mode);
                return self.launch_active_card_background_tasks();
            }
            Message::PracticeKeyShortcut(action) => {
                return self.handle_practice_shortcut(action);
            }
            Message::PracticeSessionTick => {
                if self.practice_state.phase == PracticePhase::ActiveCard {
                    self.practice_state.session_elapsed_ms =
                        self.practice_state.session_elapsed_ms.saturating_add(1000);
                    if !self.practice_state.cap_override_active
                        && matches!(self.streak.daily_goal, DailyGoal::Minutes(_))
                    {
                        let effective_minutes = self
                            .practice_state
                            .effective_today_minutes(self.streak.minutes_practiced_today);
                        if let DailyGoal::Minutes(goal) = self.streak.daily_goal
                            && effective_minutes >= goal
                        {
                            let added = self.practice_state.take_newly_elapsed_minutes();
                            if added > 0 {
                                self.streak.minutes_practiced_today =
                                    self.streak.minutes_practiced_today.saturating_add(added);
                                if let Some(db_path) = self.db_path.as_deref()
                                    && let Ok(db) = KotodamaDb::open(db_path)
                                {
                                    let _ = db.save_daily_streak(&self.streak);
                                }
                            }
                            self.stop_practice_audio_and_countdown();
                            self.practice_state.summary.session_duration_ms =
                                self.practice_state.session_elapsed_ms;
                            self.practice_state.summary.daily_cap_reached = true;
                            self.practice_state.phase = PracticePhase::SessionComplete {
                                cap_reached: true,
                            };
                            self.practice_state.card_started_at = None;
                        }
                    }
                }
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
            Message::DashboardRefreshRequested => {
                return self.launch_dashboard_refresh();
            }
            Message::DashboardLoaded(result) => {
                self.dashboard_state.is_loading = false;
                match result {
                    Ok(mut snapshot) => {
                        snapshot.streak.daily_goal = self.settings.daily_goal;
                        self.streak = snapshot.streak.clone();

                        if let Some(selected_kana) = &self.dashboard_state.selected_kana
                            && !snapshot
                                .kana_stats
                                .iter()
                                .any(|stats| &stats.kana == selected_kana)
                        {
                            self.dashboard_state.selected_kana = None;
                        }
                        if let Some(selected_card_id) = self.dashboard_state.selected_pitch_card_id
                            && !snapshot
                                .card_pitch_summaries
                                .iter()
                                .any(|summary| summary.card_id == selected_card_id)
                        {
                            self.dashboard_state.selected_pitch_card_id = None;
                        }

                        self.dashboard_state.snapshot = snapshot;
                        self.dashboard_state.has_loaded = true;
                    }
                    Err(err_msg) => {
                        let err = anyhow::anyhow!(err_msg);
                        self.report_error(
                            "Dashboard Refresh Failed",
                            &err,
                            Some("Ensure your local Kotodama database is accessible and click Refresh.".to_string()),
                        );
                    }
                }
            }
            Message::DashboardSelectTimeRange(range) => {
                self.dashboard_state.time_range = range;
            }
            Message::DashboardSelectKanaSort(order) => {
                self.dashboard_state.kana_sort = order;
            }
            Message::DashboardKanaFilterChanged(query) => {
                self.dashboard_state.kana_filter = query;
            }
            Message::DashboardSelectKana(kana) => {
                self.dashboard_state.selected_kana = kana;
            }
            Message::DashboardPitchCardFilterChanged(query) => {
                self.dashboard_state.pitch_card_filter = query;
            }
            Message::DashboardSelectPitchCard(card_id) => {
                self.dashboard_state.selected_pitch_card_id = card_id;
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
            NavTab::Setup => setup::view(
                &self.setup_state,
                &self.deck_workspace,
                &self.loaded_cards,
                resolved_theme,
            ),
            NavTab::Practice => practice::view(
                &self.practice_state,
                self.active_mode,
                &self.settings,
                &self.streak,
                PracticeDeckStatus {
                    has_deck: self.deck_workspace.active_deck.is_some(),
                    active_card_count: self.active_practice_cards().len(),
                    excluded_card_count: self.excluded_count_for_active_deck(),
                },
                PracticeMicViewState {
                    is_recording: self.practice_recording.is_some(),
                    mic_level: self.practice_mic_level,
                    elapsed_ms: self
                        .practice_recording
                        .as_ref()
                        .map_or(0, RecordingSession::elapsed_ms),
                },
                resolved_theme,
            ),
            NavTab::Dashboard => dashboard::view(
                &self.dashboard_state,
                &self.streak,
                self.last_export_status.as_deref(),
                resolved_theme,
            ),
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

    fn update_collection_path(&mut self, new_path: String) {
        let old_inferred = (!self.setup_state.collection_path_input.trim().is_empty())
            .then(|| reader::infer_media_dir(&PathBuf::from(self.setup_state.collection_path_input.trim())))
            .map(|path| path.display().to_string());

        let should_update_media = self.setup_state.media_dir_input.trim().is_empty()
            || old_inferred.as_deref() == Some(self.setup_state.media_dir_input.trim());

        self.setup_state.collection_path_input = new_path.clone();
        if should_update_media {
            let trimmed = new_path.trim();
            self.setup_state.media_dir_input = if trimmed.is_empty() {
                String::new()
            } else {
                reader::infer_media_dir(&PathBuf::from(trimmed))
                    .display()
                    .to_string()
            };
        }
        self.setup_state.refresh_preview_audio_issues();
    }

    /// Returns the active practice queue for the current deck with excluded cards filtered out.
    pub fn active_practice_cards(&self) -> Vec<&AnkiCard> {
        let Some(deck_name) = self.active_or_selected_deck_name() else {
            return Vec::new();
        };
        self.deck_workspace
            .filter_active_cards(deck_name, &self.loaded_cards)
    }

    fn active_or_selected_deck_name(&self) -> Option<&str> {
        self.setup_state
            .selected_deck_name
            .as_deref()
            .or_else(|| {
                self.deck_workspace
                    .active_deck
                    .as_ref()
                    .map(|deck| deck.deck_name.as_str())
            })
    }

    fn excluded_count_for_active_deck(&self) -> usize {
        self.active_or_selected_deck_name()
            .map_or(0, |name| self.deck_workspace.excluded_count_for_deck(name))
    }

    fn sync_active_mapping_changes(&mut self) {
        if let Some(deck_name) = &self.setup_state.selected_deck_name {
            self.deck_workspace
                .saved_mappings
                .insert(deck_name.clone(), self.setup_state.draft_mapping.clone());
        }
        self.setup_state.refresh_preview_audio_issues();
    }

    fn dispatch_load_samples(&mut self, deck_name: String) -> Task<Message> {
        let source = match self.setup_state.build_deck_source() {
            Ok(source) => source,
            Err(err) => {
                self.report_error(
                    "Invalid Anki Source",
                    &err,
                    Some("Verify your Anki collection path or AnkiConnect URL.".to_string()),
                );
                return Task::none();
            }
        };

        self.setup_state.is_busy = true;
        self.setup_state.busy_label = Some(format!("Loading learned cards from {deck_name}..."));

        Task::perform(
            run_blocking(move || {
                reader::load_learned_notes(&source, &deck_name, Some(SAMPLE_PREVIEW_LIMIT))
            }),
            Message::DeckSamplesLoaded,
        )
    }

    fn handle_play_preview_audio(&mut self, sentence: bool) -> Task<Message> {
        let Some(card) = self.setup_state.current_preview_card() else {
            self.report_warning(
                "No Sample Card Selected",
                "Select a deck with learned cards before playing test audio.",
                None,
            );
            return Task::none();
        };

        let filename = if sentence {
            card.sentence_audio.unwrap_or_default()
        } else {
            card.word_audio
        };

        let Some(media_dir) = self.setup_state.effective_media_dir() else {
            let err = anyhow::anyhow!("Media directory path is not configured.");
            self.report_error(
                "Media Directory Missing",
                &err,
                Some("Select your collection.media folder in Setup Step 1.".to_string()),
            );
            return Task::none();
        };

        let Some(audio_path) = validator::resolve_media_path(&media_dir, &filename) else {
            let err = anyhow::anyhow!("No valid audio filename was extracted for this card.");
            self.report_error(
                "No Audio Clip Found",
                &err,
                Some("Check that your Word Audio or Sentence Audio field contains a [sound:...] tag.".to_string()),
            );
            return Task::none();
        };

        self.setup_state.is_playing_audio = true;
        let speed = self.settings.playback_speed;
        Task::perform(
            run_blocking(move || playback::play_audio_file_at_speed(&audio_path, speed)),
            Message::AudioPlaybackFinished,
        )
    }

    fn stop_mic_activity(&mut self) {
        if let Some(mut monitor) = self.mic_monitor.take() {
            monitor.stop();
        }
        if let Some(session) = self.mic_recording.take() {
            drop(session);
        }
        self.mic_level = MicLevelSnapshot::default();
    }

    fn finalize_test_recording(&mut self, session: RecordingSession) {
        self.mic_level = MicLevelSnapshot::default();
        match session.finish() {
            Ok(take) => {
                if !take.speech_detected {
                    self.report_warning(
                        "No Speech Above VAD Threshold",
                        "No frames crossed the active VAD sensitivity threshold during that take.",
                        Some("Speak closer to the mic or lower the VAD sensitivity slider.".to_string()),
                    );
                }
                self.last_test_take = Some(take);
            }
            Err(err) => {
                self.report_error(
                    "Test Recording Failed",
                    &err,
                    Some("Check your microphone connection and try recording again.".to_string()),
                );
            }
        }
    }

    fn stop_practice_audio_and_countdown(&mut self) {
        if let Some(session) = self.practice_recording.take() {
            drop(session);
        }
        self.practice_mic_level = MicLevelSnapshot::default();
        self.practice_state.cancel_countdown();
        self.practice_state.is_playing_reference = false;
        self.practice_state.is_playing_user_take = false;
    }

    fn resolve_active_media_dir(&self) -> Option<PathBuf> {
        if let Some(active) = &self.deck_workspace.active_deck
            && let crate::models::DeckSource::LocalCollection { media_dir, .. } = &active.source
            && !media_dir.as_os_str().is_empty()
        {
            return Some(media_dir.clone());
        }
        self.setup_state.effective_media_dir()
    }

    fn resolve_practice_card_audio_path(
        &self,
        card: &AnkiCard,
        clip: PracticeTargetClip,
    ) -> anyhow::Result<PathBuf> {
        let media_dir = self
            .resolve_active_media_dir()
            .ok_or_else(|| anyhow::anyhow!("Media directory path is not configured."))?;
        let filename = match clip {
            PracticeTargetClip::Sentence => card
                .sentence_audio
                .as_deref()
                .filter(|name| !name.trim().is_empty())
                .unwrap_or(&card.word_audio),
            PracticeTargetClip::Word => card.word_audio.as_str(),
        };
        validator::resolve_media_path(&media_dir, filename)
            .ok_or_else(|| anyhow::anyhow!("Card '{}' has no valid audio filename for {}.", card.display_label(), clip.label()))
    }

    fn handle_practice_start_session(&mut self, extra_study: bool) -> Task<Message> {
        if self.deck_workspace.active_deck.is_none() && self.loaded_cards.is_empty() {
            self.report_warning(
                "No Anki Deck Connected",
                "Connect and validate an Anki deck in Setup before starting practice.",
                Some("Open the Setup tab, map your fields, and click Validate Audio & Save Deck.".to_string()),
            );
            return Task::none();
        }

        self.stop_mic_activity();
        self.stop_practice_audio_and_countdown();

        let active_deck = self.deck_workspace.active_deck.clone();
        let existing_cards = self.loaded_cards.clone();
        let deck_name = self
            .active_or_selected_deck_name()
            .map(str::to_string)
            .unwrap_or_default();
        let workspace = self.deck_workspace.clone();
        let db_path = self
            .db_path
            .clone()
            .or_else(|| storage::default_database_path().ok());

        self.practice_state.extra_study_mode = extra_study;
        self.practice_state.phase = PracticePhase::Loading {
            label: if extra_study {
                "Preparing extra study cards...".to_string()
            } else {
                "Loading due FSRS pronunciation queue...".to_string()
            },
        };

        Task::perform(
            run_blocking(move || {
                let mut cards = existing_cards;
                let mut discovered_media: Option<PathBuf> = None;

                if cards.is_empty()
                    && let Some(deck) = active_deck.as_ref()
                {
                    cards = reader::import_learned_cards(
                        &deck.source,
                        &deck.deck_name,
                        &deck.field_mapping,
                    )?;
                    let media_dir = match &deck.source {
                        crate::models::DeckSource::LocalCollection { media_dir, .. } => {
                            Some(media_dir.clone())
                        }
                        crate::models::DeckSource::AnkiConnect { endpoint_url } => {
                            reader::inspect_ankiconnect(endpoint_url)
                                .ok()
                                .and_then(|(_, dir)| dir)
                        }
                    };
                    if let Some(ref dir) = media_dir {
                        let _ = validator::validate_deck_cards(&mut cards, dir);
                    }
                    discovered_media = media_dir;
                }

                let active_ids: Vec<i64> = workspace
                    .filter_active_cards(&deck_name, &cards)
                    .into_iter()
                    .map(|card| card.card_id)
                    .collect();

                if let Some(db_path) = db_path {
                    let db = KotodamaDb::open(&db_path)?;
                    let latest_pitch = db.latest_pitch_errors_by_card()?;
                    let ordered_ids = if extra_study {
                        active_ids
                    } else {
                        db.list_due_card_ids(&active_ids, Utc::now())?
                    };
                    let (first_fsrs, first_previews) = if let Some(&first_id) = ordered_ids.first() {
                        load_card_fsrs_context(&db_path, first_id)?
                    } else {
                        (None, [1, 1, 2, 4])
                    };
                    Ok((
                        cards,
                        ordered_ids,
                        latest_pitch,
                        discovered_media,
                        first_fsrs,
                        first_previews,
                    ))
                } else {
                    Ok((
                        cards,
                        active_ids,
                        BTreeMap::new(),
                        discovered_media,
                        None,
                        [1, 1, 2, 4],
                    ))
                }
            }),
            Message::PracticeDeckPrepared,
        )
    }

    #[allow(clippy::type_complexity)]
    fn handle_practice_deck_prepared(
        &mut self,
        result: Result<
            (
                Vec<AnkiCard>,
                Vec<i64>,
                BTreeMap<i64, f32>,
                Option<PathBuf>,
                Option<FsrsCardState>,
                [u32; 4],
            ),
            String,
        >,
    ) -> Task<Message> {
        if !matches!(self.practice_state.phase, PracticePhase::Loading { .. }) {
            return Task::none();
        }

        match result {
            Ok((
                mut cards,
                ordered_ids,
                latest_pitch,
                discovered_media,
                first_fsrs,
                first_previews,
            )) => {
                for card in &mut cards {
                    if let Some(&pitch_err) = latest_pitch.get(&card.card_id) {
                        card.last_pitch_error_percent = Some(pitch_err);
                    }
                }
                self.loaded_cards = cards;

                if self.setup_state.media_dir_input.trim().is_empty()
                    && let Some(dir) = discovered_media
                {
                    self.setup_state.media_dir_input = dir.display().to_string();
                }

                let active_cards = self.active_practice_cards();
                if active_cards.is_empty() {
                    self.practice_state.reset_to_lobby();
                    self.report_warning(
                        "No Active Cards Available",
                        "All learned cards in this deck are excluded or none were found.",
                        Some("Restore excluded cards or validate another deck in Setup.".to_string()),
                    );
                    return Task::none();
                }

                let by_id: BTreeMap<i64, AnkiCard> = active_cards
                    .into_iter()
                    .map(|card| (card.card_id, card.clone()))
                    .collect();
                let queue: Vec<AnkiCard> = ordered_ids
                    .into_iter()
                    .filter_map(|id| by_id.get(&id).cloned())
                    .collect();

                if queue.is_empty() {
                    self.practice_state.begin_session(
                        Vec::new(),
                        self.active_mode,
                        self.settings.blind_mode,
                        false,
                        Some(0),
                    );
                    return Task::none();
                }

                let extra_study = self.practice_state.extra_study_mode;
                let due_hint = Some(queue.len());
                self.practice_state.begin_session(
                    queue,
                    self.active_mode,
                    self.settings.blind_mode,
                    extra_study,
                    due_hint,
                );
                self.practice_state.current_fsrs_state = first_fsrs;
                self.practice_state.preview_intervals_days = Some(first_previews);
                self.launch_reference_decode_task()
            }
            Err(err_msg) => {
                self.practice_state.reset_to_lobby();
                let err = anyhow::anyhow!(err_msg);
                self.report_error(
                    "Could Not Start Practice Session",
                    &err,
                    Some("Check your Anki source and database settings in Setup.".to_string()),
                );
                Task::none()
            }
        }
    }

    fn launch_active_card_background_tasks(&mut self) -> Task<Message> {
        let decode_task = self.launch_reference_decode_task();
        let Some(card) = self.practice_state.current_card() else {
            return decode_task;
        };
        let card_id = card.card_id;
        let card_generation = self.practice_state.card_generation;
        let Some(db_path) = self
            .db_path
            .clone()
            .or_else(|| storage::default_database_path().ok())
        else {
            return decode_task;
        };

        let fsrs_task = Task::perform(
            run_blocking(move || load_card_fsrs_context(&db_path, card_id)),
            move |result| Message::PracticeCardContextLoaded {
                card_generation,
                result,
            },
        );
        Task::batch([decode_task, fsrs_task])
    }

    fn launch_reference_decode_task(&mut self) -> Task<Message> {
        let Some(card) = self.practice_state.current_card().cloned() else {
            return Task::none();
        };
        let generation = self.practice_state.card_generation;
        let clip = self.practice_state.target_clip;

        match self.resolve_practice_card_audio_path(&card, clip) {
            Ok(audio_path) => {
                self.practice_state.is_decoding_reference = true;
                self.practice_state.reference_audio_error = None;
                Task::perform(
                    run_blocking(move || decode_reference_with_contour(&audio_path)),
                    move |result| Message::PracticeReferenceDecoded { generation, result },
                )
            }
            Err(err) => {
                self.practice_state.is_decoding_reference = false;
                self.practice_state.reference_audio_error = Some(format!("{err:#}"));
                Task::none()
            }
        }
    }

    fn handle_practice_play_reference(&mut self) -> Task<Message> {
        let Some(card) = self.practice_state.current_card().cloned() else {
            return Task::none();
        };
        let generation = self.practice_state.card_generation;
        let speed = self.settings.playback_speed;

        if let Some(decoded) = self.practice_state.reference_audio.clone() {
            self.practice_state.is_playing_reference = true;
            return Task::perform(
                run_blocking(move || playback::play_decoded_audio(decoded, speed)),
                move |result| Message::PracticeReferencePlaybackFinished { generation, result },
            );
        }

        match self.resolve_practice_card_audio_path(&card, self.practice_state.target_clip) {
            Ok(audio_path) => {
                self.practice_state.is_playing_reference = true;
                Task::perform(
                    run_blocking(move || playback::play_audio_file_at_speed(&audio_path, speed)),
                    move |result| Message::PracticeReferencePlaybackFinished { generation, result },
                )
            }
            Err(err) => {
                self.practice_state.awaiting_shadow_countdown = false;
                self.report_error(
                    "Reference Audio Unavailable",
                    &err,
                    Some("Verify your collection.media path in Setup or skip this card.".to_string()),
                );
                Task::none()
            }
        }
    }

    fn start_practice_recording(&mut self) {
        if self.practice_recording.is_some() {
            return;
        }
        let vad_config = VadConfig::from_settings(&self.settings);
        match RecordingSession::start(
            self.settings.preferred_input_device.as_deref(),
            vad_config,
        ) {
            Ok(session) => {
                self.practice_mic_level = session.snapshot();
                self.practice_recording = Some(session);
            }
            Err(err) => {
                self.report_error(
                    "Could Not Start Recording",
                    &err,
                    Some("Check your microphone connection and Settings calibration, then press R again.".to_string()),
                );
            }
        }
    }

    fn finalize_practice_recording(&mut self, session: RecordingSession) -> Task<Message> {
        self.practice_mic_level = MicLevelSnapshot::default();
        match session.finish() {
            Ok(take) => {
                self.practice_state.text_revealed = true;
                self.practice_state.take_generation =
                    self.practice_state.take_generation.saturating_add(1);
                let take_generation = self.practice_state.take_generation;

                let scored_audio = if take.trimmed_audio.samples.is_empty() {
                    take.raw_audio.clone()
                } else {
                    take.trimmed_audio.clone()
                };
                self.practice_state.last_take = Some(take.clone());

                if !take.speech_detected || scored_audio.samples.is_empty() {
                    self.report_warning(
                        "No Speech Above VAD Threshold",
                        "No frames crossed the VAD speech threshold during your recording.",
                        Some("Speak a little louder or lower the VAD sensitivity slider in Settings.".to_string()),
                    );
                    return Task::none();
                }

                let Some(card) = self.practice_state.current_card().cloned() else {
                    return Task::none();
                };
                let mode = self.active_mode;
                let thresholds = self.settings.score_thresholds.clone();
                let ref_audio = self.practice_state.reference_audio.clone();
                let ref_path = self
                    .resolve_practice_card_audio_path(&card, self.practice_state.target_clip)
                    .ok();
                let duration_ms = self
                    .practice_state
                    .card_started_at
                    .map_or(scored_audio.duration_ms(), |started| {
                        started.elapsed().as_millis().max(1) as u64
                    });

                self.practice_state.is_scoring = true;
                Task::perform(
                    run_blocking(move || {
                        evaluate_practice_take(
                            &card,
                            mode,
                            scored_audio,
                            ref_audio,
                            ref_path,
                            &thresholds,
                            duration_ms,
                        )
                    }),
                    move |result| Message::PracticeTakeEvaluated {
                        take_generation,
                        result,
                    },
                )
            }
            Err(err) => {
                self.report_error(
                    "Recording Failed",
                    &err,
                    Some("Check your microphone connection and press R to try again.".to_string()),
                );
                Task::none()
            }
        }
    }

    fn handle_practice_submit_rating(
        &mut self,
        rating: FsrsRating,
        is_manual: bool,
    ) -> Task<Message> {
        if self.practice_state.phase != PracticePhase::ActiveCard
            || self.practice_state.is_committing
        {
            return Task::none();
        }
        self.stop_practice_audio_and_countdown();

        let Some(card) = self.practice_state.current_card().cloned() else {
            return Task::none();
        };
        let card_generation = self.practice_state.card_generation;
        let thresholds = self.settings.score_thresholds.clone();
        let daily_goal = self.settings.daily_goal;

        let (breakdown, mora_attempts, dtw_distance, duration_ms, latest_pitch) =
            if let Some(eval) = &self.practice_state.last_evaluation {
                (
                    eval.breakdown.clone(),
                    eval.mora_evaluation
                        .as_ref()
                        .map(|m| m.attempts.clone())
                        .unwrap_or_default(),
                    eval.pitch_analysis.as_ref().map(|p| p.dtw_distance),
                    eval.duration_ms,
                    eval.pitch_analysis.as_ref().map(|p| p.pitch_error_percent),
                )
            } else {
                let score_percent = manual_rating_score_percent(rating, &thresholds);
                let mode = self.active_mode;
                let fallback_breakdown = crate::models::CombinedScoreBreakdown {
                    mode,
                    mora_accuracy_percent: None,
                    pitch_accuracy_percent: None,
                    pitch_error_percent: None,
                    combined_score_percent: score_percent,
                    recommended_rating: rating,
                };
                let elapsed = self
                    .practice_state
                    .card_started_at
                    .map_or(1000, |t| t.elapsed().as_millis().max(1) as u64);
                (fallback_breakdown, Vec::new(), None, elapsed, None)
            };

        let was_manual_override = is_manual && rating != breakdown.recommended_rating;
        let streak_minutes_to_add = self.practice_state.take_newly_elapsed_minutes();

        let db_path = match self
            .db_path
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Local database path is unavailable."))
            .or_else(|_| storage::default_database_path())
        {
            Ok(path) => path,
            Err(err) => {
                self.report_error(
                    "Database Unavailable",
                    &err,
                    Some("Ensure your system data directory is writable.".to_string()),
                );
                return Task::none();
            }
        };

        self.practice_state.is_committing = true;
        Task::perform(
            run_blocking(move || {
                let mut db = KotodamaDb::open(&db_path)?;
                let scheduler = FsrsScheduler::default_scheduler()?;
                let (_review_id, outcome, streak) = scheduler.record_review_with_streak_minutes(
                    &mut db,
                    &card,
                    &breakdown,
                    &mora_attempts,
                    &thresholds,
                    Some(rating),
                    dtw_distance,
                    duration_ms,
                    streak_minutes_to_add,
                    daily_goal,
                    Utc::now(),
                )?;
                Ok((outcome, streak, latest_pitch))
            }),
            move |result| Message::PracticeReviewCommitted {
                card_generation,
                was_manual_override,
                result,
            },
        )
    }

    fn handle_practice_shortcut(&mut self, action: PracticeShortcutAction) -> Task<Message> {
        match action {
            PracticeShortcutAction::ToggleBlindMode => {
                return self.update(Message::ToggleBlindMode(!self.settings.blind_mode));
            }
            PracticeShortcutAction::TogglePlaybackSpeed => {
                let next_speed = if self.settings.playback_speed == PlaybackSpeed::Slow {
                    PlaybackSpeed::Normal
                } else {
                    PlaybackSpeed::Slow
                };
                return self.update(Message::SelectPlaybackSpeed(next_speed));
            }
            _ => {}
        }

        if self.practice_state.phase != PracticePhase::ActiveCard {
            return Task::none();
        }

        match action {
            PracticeShortcutAction::PlayReference => self.update(Message::PracticePlayReference),
            PracticeShortcutAction::ToggleRecord => self.update(Message::PracticeToggleRecording),
            PracticeShortcutAction::PlayUserTake => self.update(Message::PracticePlayUserTake),
            PracticeShortcutAction::RateCard(rating) => self.update(Message::PracticeSubmitRating {
                rating,
                is_manual: true,
            }),
            PracticeShortcutAction::AcceptRecommended => {
                if let Some(rating) = self
                    .practice_state
                    .last_evaluation
                    .as_ref()
                    .map(|eval| eval.breakdown.recommended_rating)
                {
                    return self.update(Message::PracticeSubmitRating {
                        rating,
                        is_manual: false,
                    });
                }
                Task::none()
            }
            PracticeShortcutAction::CancelActive => {
                if let Some(session) = self.practice_recording.take() {
                    drop(session);
                    self.practice_mic_level = MicLevelSnapshot::default();
                }
                self.practice_state.cancel_countdown();
                Task::none()
            }
            PracticeShortcutAction::ToggleBlindMode | PracticeShortcutAction::TogglePlaybackSpeed => {
                Task::none()
            }
        }
    }

    fn handle_validate_and_save_deck(&mut self) -> Task<Message> {
        let Some(deck_name) = self.setup_state.selected_deck_name.clone() else {
            let err = anyhow::anyhow!("Please choose a deck before saving.");
            self.report_error("No Deck Selected", &err, None);
            return Task::none();
        };

        let source = match self.setup_state.build_deck_source() {
            Ok(source) => source,
            Err(err) => {
                self.report_error(
                    "Invalid Anki Source",
                    &err,
                    Some("Check your collection.anki2 path or AnkiConnect URL.".to_string()),
                );
                return Task::none();
            }
        };

        let mapping = self.setup_state.draft_mapping.clone();
        if let Err(err) = mapping.validate() {
            self.report_error(
                "Incomplete Field Mapping",
                &err,
                Some("Map Word / Expression, Kana Reading, and Word Audio before saving.".to_string()),
            );
            return Task::none();
        }

        let Some(media_dir) = self.setup_state.effective_media_dir() else {
            let err = anyhow::anyhow!("A collection.media folder is required to validate reference audio.");
            self.report_error(
                "Media Directory Missing",
                &err,
                Some("Select your collection.media directory in Setup Step 1.".to_string()),
            );
            return Task::none();
        };

        self.setup_state.is_busy = true;
        self.setup_state.busy_label = Some(format!(
            "Importing learned cards and checking audio quality for {deck_name}..."
        ));

        Task::perform(
            run_blocking(move || {
                let mut cards = reader::import_learned_cards(&source, &deck_name, &mapping)?;
                let report = validator::validate_deck_cards(&mut cards, &media_dir);
                let config = DeckConfig {
                    deck_name,
                    source,
                    field_mapping: mapping,
                    last_synced_at: Some(Utc::now()),
                    flagged_audio_count: report.flagged_cards,
                };
                config.validate()?;
                Ok((config, cards, report))
            }),
            Message::DeckImportCompleted,
        )
    }

    fn allocate_toast_id(&mut self) -> u64 {
        let id = self.next_toast_id;
        self.next_toast_id = self.next_toast_id.saturating_add(1);
        id
    }

    fn launch_dashboard_refresh(&mut self) -> Task<Message> {
        let db_path = match self
            .db_path
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Local database path is unavailable."))
            .or_else(|_| storage::default_database_path())
        {
            Ok(path) => path,
            Err(err) => {
                self.report_warning(
                    "Dashboard Refresh Unavailable",
                    format!("{err:#}"),
                    Some("Ensure your system data directory is writable.".to_string()),
                );
                return Task::none();
            }
        };

        self.dashboard_state.is_loading = true;
        Task::perform(
            run_blocking(move || KotodamaDb::open(&db_path)?.load_dashboard_snapshot()),
            Message::DashboardLoaded,
        )
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

    fn persist_deck_workspace(&mut self) {
        let Some(path) = self.deck_config_path.as_deref() else {
            return;
        };

        if let Err(err) = storage::save_deck_workspace(path, &self.deck_workspace) {
            self.report_error(
                "Could Not Save Deck Configuration",
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

        let mic_calibration_card = self.render_mic_calibration_card(resolved_theme, colors);

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

        let minute_goal_presets =
            [5_u32, 10, 15].into_iter().fold(row![].spacing(10), |r, minutes| {
                let selected = self.settings.daily_goal == DailyGoal::Minutes(minutes);
                r.push(
                    button(text(format!("{minutes} min")).size(14))
                        .padding([8, 14])
                        .style(theme::pill_button_style(selected))
                        .on_press(Message::SetDailyGoalMinutes(minutes)),
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
                    minute_goal_presets,
                ]
                .spacing(8),
            ]
            .spacing(14),
        )
        .width(Length::Fill)
        .padding(20)
        .style(theme::card_style);

        let thresholds = &self.settings.score_thresholds;
        let threshold_presets = [
            ("Lenient (40/65/85)", ScoreThresholds::lenient()),
            ("Balanced (50/75/90)", ScoreThresholds::balanced()),
            ("Strict (60/80/92)", ScoreThresholds::strict()),
        ]
        .into_iter()
        .fold(row![].spacing(10), |r, (label, preset)| {
            let selected = thresholds == &preset;
            r.push(
                button(text(label).size(13))
                    .padding([8, 14])
                    .style(theme::pill_button_style(selected))
                    .on_press(Message::SetScoreThresholds(preset)),
            )
        });

        let fsrs_card = container(
            column![
                text("FSRS Score Thresholds").size(18).color(colors.text_primary),
                text("Choose how combined mora and pitch scores map automatically to FSRS ratings.")
                    .size(14)
                    .color(colors.text_muted),
                threshold_presets,
                text(format!(
                    "Again < {:.0}% · Hard >= {:.0}% · Good >= {:.0}% · Easy >= {:.0}%",
                    thresholds.hard_min, thresholds.hard_min, thresholds.good_min, thresholds.easy_min
                ))
                .size(13)
                .color(colors.sage),
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

        let exclusions_card = if let Some(deck_name) = self.active_or_selected_deck_name() {
            let entries = self.deck_workspace.excluded_entries_for_deck(deck_name);
            let count = entries.len();
            let mut body = column![
                row![
                    text("Excluded Cards").size(18).color(colors.text_primary),
                    space().width(Length::Fill),
                    container(
                        text(format!("{count} excluded"))
                            .size(12)
                            .color(colors.text_muted),
                    )
                    .padding([4, 10])
                    .style(theme::badge_style(colors.card_muted, colors.border)),
                ]
                .align_y(Alignment::Center),
                text(format!(
                    "Cards skipped from Kotodama practice in '{deck_name}' without modifying Anki."
                ))
                .size(14)
                .color(colors.text_muted),
            ]
            .spacing(12);

            if !entries.is_empty() {
                body = body.push(
                    button(text("Restore All Excluded").size(13))
                        .padding([8, 14])
                        .style(theme::secondary_button_style)
                        .on_press(Message::RestoreAllExcludedCards(deck_name.to_string())),
                );
                body = body.push(setup::render_excluded_entries_list(
                    deck_name,
                    &entries,
                    colors,
                ));
            }

            container(body)
                .width(Length::Fill)
                .padding(20)
                .style(theme::card_style)
        } else {
            container(
                column![
                    text("Excluded Cards").size(18).color(colors.text_primary),
                    text("Connect a deck in Setup to manage cards excluded from pronunciation practice.")
                        .size(14)
                        .color(colors.text_muted),
                ]
                .spacing(8),
            )
            .width(Length::Fill)
            .padding(20)
            .style(theme::card_style)
        };

        column![
            appearance_card,
            mic_calibration_card,
            ergonomics_card,
            exclusions_card,
            fsrs_card
        ]
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

    fn render_mic_calibration_card(
        &self,
        resolved_theme: ResolvedTheme,
        colors: &'static ThemeTokens,
    ) -> Element<'_, Message> {
        let mut device_options = vec![SYSTEM_DEFAULT_MIC_OPTION.to_string()];
        for device in &self.input_devices {
            if !device_options.contains(&device.name) {
                device_options.push(device.name.clone());
            }
        }
        if let Some(preferred) = &self.settings.preferred_input_device
            && !device_options.contains(preferred)
        {
            device_options.push(preferred.clone());
        }

        let selected_device = self
            .settings
            .preferred_input_device
            .clone()
            .unwrap_or_else(|| SYSTEM_DEFAULT_MIC_OPTION.to_string());

        let is_monitoring = self.mic_monitor.is_some();
        let is_recording = self.mic_recording.is_some();
        let is_active = is_monitoring || is_recording;

        let (status_label, status_color, status_bg, status_border) = if !is_active {
            (
                "Idle",
                colors.text_muted,
                colors.card_muted,
                colors.border,
            )
        } else if self.mic_level.is_clipping {
            (
                "Clipping!",
                colors.danger,
                colors.danger_soft,
                colors.danger,
            )
        } else if self.mic_level.is_speech {
            (
                "Speech Detected",
                colors.sage,
                colors.sage_soft,
                colors.sage,
            )
        } else {
            (
                "Below VAD",
                colors.amber,
                colors.amber_soft,
                colors.amber,
            )
        };

        let active_device_caption = if let Some(session) = &self.mic_recording {
            format!(
                "Recording on {} · {:.1}s",
                session.device_name(),
                session.elapsed_ms() as f32 / 1000.0
            )
        } else if let Some(monitor) = &self.mic_monitor {
            format!(
                "Monitoring {} ({} Hz)",
                monitor.device_name(),
                monitor.sample_rate()
            )
        } else if let Some(default_dev) = self.input_devices.iter().find(|d| d.is_default) {
            format!("Default mic: {}", default_dev.name)
        } else if self.input_devices.is_empty() {
            "No microphone detected yet — click Refresh Mics or connect an input device.".to_string()
        } else {
            format!("{} microphone(s) available.", self.input_devices.len())
        };

        let monitor_button_label = if is_monitoring {
            "Stop Mic Test"
        } else {
            "Start Mic Test"
        };
        let record_button_label = if is_recording {
            "Stop Recording"
        } else {
            "Record Test Take"
        };

        let mut play_take_button = button(
            text(if self.is_playing_test_take {
                "Playing Take..."
            } else {
                "Play Test Take"
            })
            .size(13),
        )
        .padding([8, 14])
        .style(theme::secondary_button_style);
        if self.last_test_take.is_some() && !self.is_playing_test_take && !is_recording {
            play_take_button = play_take_button.on_press(Message::PlayTestTake);
        }

        let silence_presets = [600_u64, 1000, 1500]
            .into_iter()
            .fold(row![].spacing(10), |r, ms| {
                let selected = self.settings.vad_silence_ms == ms;
                r.push(
                    button(text(format!("{ms} ms")).size(13))
                        .padding([6, 12])
                        .style(theme::pill_button_style(selected))
                        .on_press(Message::SetVadSilenceMs(ms)),
                )
            });

        let threshold_db = capture::rms_to_dbfs(self.settings.vad_rms_threshold);

        let mut body = column![
            row![
                text("Microphone & VAD Calibration")
                    .size(18)
                    .color(colors.text_primary),
                space().width(Length::Fill),
                container(text(status_label).size(12).color(status_color))
                    .padding([4, 10])
                    .style(theme::badge_style(status_bg, status_border)),
            ]
            .align_y(Alignment::Center),
            text("Verify your microphone gain stays green during normal speech without clipping, and tune the coral VAD speech threshold line.")
                .size(14)
                .color(colors.text_muted),
            row![
                pick_list(device_options, Some(selected_device), Message::SelectInputDevice)
                    .width(Length::Fill)
                    .style(theme::pick_list_style)
                    .menu_style(theme::pick_list_menu_style),
                button(text("Refresh Mics").size(13))
                    .padding([8, 14])
                    .style(theme::secondary_button_style)
                    .on_press(Message::RefreshInputDevices),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
            canvas(MicMeterProgram {
                meter_ratio: if is_active {
                    self.mic_level.meter_ratio
                } else {
                    0.0
                },
                vad_threshold_ratio: capture::vad_threshold_to_meter_ratio(
                    self.settings.vad_rms_threshold,
                ),
                is_speech: self.mic_level.is_speech,
                is_clipping: self.mic_level.is_clipping,
                is_active,
                resolved_theme,
            })
            .width(Length::Fill)
            .height(24),
            row![
                text(format!(
                    "{:.1} dBFS",
                    if is_active {
                        self.mic_level.db_fs
                    } else {
                        -60.0
                    }
                ))
                .size(13)
                .color(colors.text_primary),
                space().width(Length::Fill),
                text(active_device_caption)
                    .size(12)
                    .color(colors.text_muted),
            ]
            .align_y(Alignment::Center),
            row![
                button(text(monitor_button_label).size(13))
                    .padding([8, 14])
                    .style(theme::primary_button_style)
                    .on_press(Message::ToggleMicCalibration),
                button(text(record_button_label).size(13))
                    .padding([8, 14])
                    .style(theme::secondary_button_style)
                    .on_press(Message::ToggleTestRecording),
                play_take_button,
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        ]
        .spacing(12);

        if let Some(take) = &self.last_test_take {
            body = body.push(
                container(
                    text(format!(
                        "Last test take: {} ms raw · {} ms VAD-trimmed · peak {:.2} · {}",
                        take.raw_audio.duration_ms(),
                        take.trimmed_audio.duration_ms(),
                        take.peak_level,
                        take.stop_reason.label()
                    ))
                    .size(12)
                    .color(colors.text_muted),
                )
                .width(Length::Fill)
                .padding([8, 12])
                .style(theme::muted_card_style),
            );
        }

        body = body
            .push(
                column![
                    row![
                        text("VAD Speech Sensitivity")
                            .size(14)
                            .color(colors.text_muted),
                        space().width(Length::Fill),
                        text(format!(
                            "RMS {:.3} ({:.1} dBFS)",
                            self.settings.vad_rms_threshold, threshold_db
                        ))
                        .size(13)
                        .color(colors.coral_text),
                    ]
                    .align_y(Alignment::Center),
                    slider(
                        0.005_f32..=0.15_f32,
                        self.settings.vad_rms_threshold,
                        Message::SetVadRmsThreshold,
                    )
                    .step(0.005_f32)
                    .style(theme::slider_style),
                ]
                .spacing(6),
            )
            .push(
                column![
                    text("Trailing Silence Auto-Stop")
                        .size(14)
                        .color(colors.text_muted),
                    silence_presets,
                ]
                .spacing(8),
            );

        container(body)
            .width(Length::Fill)
            .padding(20)
            .style(theme::card_style)
            .into()
    }
}

pub(crate) struct MicMeterProgram {
    pub(crate) meter_ratio: f32,
    pub(crate) vad_threshold_ratio: f32,
    pub(crate) is_speech: bool,
    pub(crate) is_clipping: bool,
    pub(crate) is_active: bool,
    pub(crate) resolved_theme: ResolvedTheme,
}

impl canvas::Program<Message> for MicMeterProgram {
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

        let track = canvas::Path::rounded_rectangle(Point::ORIGIN, Size::new(width, height), 8.0.into());
        frame.fill(&track, colors.card_muted);
        frame.stroke(
            &track,
            canvas::Stroke::default()
                .with_color(colors.border)
                .with_width(1.0),
        );

        let clamped_level = self.meter_ratio.clamp(0.0, 1.0);
        if self.is_active && clamped_level > 0.005 {
            let fill_width = (width * clamped_level).max(4.0).min(width);
            let fill_color = if self.is_clipping {
                colors.danger
            } else if self.is_speech {
                colors.sage
            } else if clamped_level > 0.78 {
                colors.amber
            } else {
                Color::from_rgba(
                    colors.text_muted.r,
                    colors.text_muted.g,
                    colors.text_muted.b,
                    0.55,
                )
            };
            let level_bar = canvas::Path::rounded_rectangle(
                Point::new(1.0, 1.0),
                Size::new((fill_width - 2.0).max(2.0), (height - 2.0).max(2.0)),
                7.0.into(),
            );
            frame.fill(&level_bar, fill_color);
        }

        let marker_x = (width * self.vad_threshold_ratio.clamp(0.02, 0.98)).clamp(2.0, width - 2.0);
        let threshold_line = canvas::Path::line(
            Point::new(marker_x, 2.0),
            Point::new(marker_x, (height - 2.0).max(2.0)),
        );
        frame.stroke(
            &threshold_line,
            canvas::Stroke::default()
                .with_color(colors.coral)
                .with_width(2.5),
        );

        vec![frame.into_geometry()]
    }
}

fn practice_keyboard_event(
    event: iced::Event,
    status: iced::event::Status,
    _window: iced::window::Id,
) -> Option<Message> {
    if status == iced::event::Status::Captured {
        return None;
    }
    let iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
        key,
        physical_key,
        modifiers,
        repeat: false,
        ..
    }) = event
    else {
        return None;
    };
    practice::map_key_to_shortcut(&key, physical_key, modifiers).map(Message::PracticeKeyShortcut)
}

fn decode_reference_with_contour(
    audio_path: &std::path::Path,
) -> anyhow::Result<(DecodedAudio, Option<ReferenceContourPreview>)> {
    let decoded = playback::decode_audio_file(audio_path)?;
    let contour = dsp::extract_f0_contour(&decoded, &PitchExtractionConfig::default())
        .ok()
        .and_then(|f0| dsp::normalize_to_semitones(&f0).ok())
        .map(|(semitones, median_hz)| ReferenceContourPreview {
            semitones,
            median_hz,
        });
    Ok((decoded, contour))
}

fn load_card_fsrs_context(
    db_path: &std::path::Path,
    card_id: i64,
) -> anyhow::Result<(Option<FsrsCardState>, [u32; 4])> {
    let db = KotodamaDb::open(db_path)?;
    let existing = db.get_fsrs_state(card_id)?;
    let scheduler = FsrsScheduler::default_scheduler()?;
    let previews = scheduler.preview_next_intervals(existing.as_ref(), Utc::now())?;
    Ok((existing, previews))
}

fn evaluate_practice_take(
    card: &AnkiCard,
    mode: PracticeMode,
    scored_audio: DecodedAudio,
    reference_audio: Option<DecodedAudio>,
    reference_path: Option<PathBuf>,
    thresholds: &ScoreThresholds,
    duration_ms: u64,
) -> anyhow::Result<PracticeEvaluationResult> {
    let now = Utc::now();
    let reference = match reference_audio {
        Some(audio) => Some(audio),
        None => match reference_path {
            Some(path) if path.is_file() => Some(playback::decode_audio_file(&path)?),
            _ => None,
        },
    };

    let mora_evaluation = match mode {
        PracticeMode::MoraPurity | PracticeMode::CombinedShadowing => {
            Some(HeuristicAcousticMoraScorer.score_morae(&crate::models::MoraScoreRequest {
                card_id: card.card_id,
                target_kana_sequence: card.kana.clone(),
                user_audio: scored_audio.clone(),
                reference_audio: reference.clone(),
                recorded_at: now,
            })?)
        }
        PracticeMode::PitchAccent => None,
    };

    let pitch_analysis = match mode {
        PracticeMode::PitchAccent | PracticeMode::CombinedShadowing => {
            let ref_buf = reference.as_ref().ok_or_else(|| {
                anyhow::anyhow!(
                    "Reference audio is unavailable for '{}'; check collection.media.",
                    card.display_label()
                )
            })?;
            Some(pitch::score_pitch(
                ref_buf,
                &scored_audio,
                &PitchExtractionConfig::default(),
            )?)
        }
        PracticeMode::MoraPurity => None,
    };

    let breakdown = pitch::combine_practice_score(
        mode,
        mora_evaluation
            .as_ref()
            .map(|m| m.mora_accuracy_percent),
        pitch_analysis.as_ref().map(|p| p.pitch_error_percent),
        thresholds,
    )?;

    Ok(PracticeEvaluationResult {
        card_id: card.card_id,
        mode,
        mora_evaluation,
        pitch_analysis,
        breakdown,
        duration_ms,
        evaluated_at: now,
    })
}

fn manual_rating_score_percent(rating: FsrsRating, thresholds: &ScoreThresholds) -> f32 {
    match rating {
        FsrsRating::Again => (thresholds.hard_min * 0.5).clamp(0.0, 100.0),
        FsrsRating::Hard => ((thresholds.hard_min + thresholds.good_min) * 0.5).clamp(0.0, 100.0),
        FsrsRating::Good => ((thresholds.good_min + thresholds.easy_min) * 0.5).clamp(0.0, 100.0),
        FsrsRating::Easy => ((thresholds.easy_min + 100.0) * 0.5).clamp(0.0, 100.0),
    }
}

fn normalize_optional_field(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed == NONE_FIELD_OPTION {
        None
    } else {
        Some(trimmed.to_string())
    }
}

async fn run_blocking<T, F>(work: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
{
    match tokio::task::spawn_blocking(work).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(err)) => Err(format!("{err:#}")),
        Err(err) => Err(format!("Background task stopped unexpectedly: {err}")),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::models::{AnkiDeckSummary, AnkiNoteType, DeckSource, ErrorSeverity, FieldMapping};

    static UI_TEMP_COUNTER: AtomicU64 = AtomicU64::new(1);

    fn temp_config_paths(label: &str) -> (PathBuf, PathBuf, PathBuf) {
        let id = UI_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "kotodama_ui_test_{label}_{}_{}",
            std::process::id(),
            id
        ));
        let settings = dir.join("settings.json");
        let deck = dir.join("deck_config.json");
        (dir, settings, deck)
    }

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
        let (dir, settings_path, deck_path) = temp_config_paths("theme");

        let mut app = KotodamaApp::with_config_paths(settings_path.clone(), deck_path);
        let _ = app.update(Message::SetThemePreference(ThemePreference::Dark));
        assert!(app.toasts.is_empty());

        let loaded = storage::load_settings(&settings_path)?;
        assert_eq!(loaded.theme_preference, ThemePreference::Dark);

        let _ = fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn setup_source_mode_infers_media_dir_and_updates_live_preview() {
        let mut app = KotodamaApp::default();

        let _ = app.update(Message::SelectSetupSourceMode(SetupSourceMode::AnkiConnect));
        assert_eq!(app.setup_state.source_mode, SetupSourceMode::AnkiConnect);
        let _ = app.update(Message::SelectSetupSourceMode(
            SetupSourceMode::LocalCollection,
        ));

        let _ = app.update(Message::CollectionFilePicked(Ok(Some(PathBuf::from(
            "/home/user/.local/share/Anki2/User 1/collection.anki2",
        )))));
        assert_eq!(
            app.setup_state.media_dir_input,
            "/home/user/.local/share/Anki2/User 1/collection.media"
        );

        let summary = AnkiCollectionSummary {
            source: DeckSource::LocalCollection {
                collection_path: PathBuf::from("/home/user/.local/share/Anki2/User 1/collection.anki2"),
                media_dir: PathBuf::from("/home/user/.local/share/Anki2/User 1/collection.media"),
            },
            decks: vec![AnkiDeckSummary {
                deck_id: 10,
                name: "Core 2k".to_string(),
                learned_card_count: 2,
                total_card_count: 5,
            }],
            note_types: vec![AnkiNoteType {
                id: 20,
                name: "Core".to_string(),
                field_names: vec![
                    "Expression".to_string(),
                    "Reading".to_string(),
                    "AltReading".to_string(),
                    "WordAudio".to_string(),
                    "Sentence".to_string(),
                ],
            }],
        };

        let _ = app.update(Message::CollectionLoaded(Ok((summary, None))));
        assert!(app.setup_state.collection.is_some());

        app.setup_state.selected_deck_name = Some("Core 2k".to_string());
        let notes = vec![
            AnkiRawNote {
                card_id: 1,
                note_id: 100,
                deck_name: "Core 2k".to_string(),
                note_type_id: 20,
                anki_queue: AnkiCard::LEARNED_QUEUE,
                fields: BTreeMap::from([
                    ("Expression".to_string(), "靴".to_string()),
                    ("Reading".to_string(), "くつ".to_string()),
                    ("AltReading".to_string(), "クツ".to_string()),
                    ("WordAudio".to_string(), "[sound:kutsu.mp3]".to_string()),
                    ("Sentence".to_string(), "新しい靴".to_string()),
                ]),
            },
            AnkiRawNote {
                card_id: 2,
                note_id: 101,
                deck_name: "Core 2k".to_string(),
                note_type_id: 20,
                anki_queue: AnkiCard::LEARNED_QUEUE,
                fields: BTreeMap::from([
                    ("Expression".to_string(), "水".to_string()),
                    ("Reading".to_string(), "みず".to_string()),
                    ("AltReading".to_string(), "ミズ".to_string()),
                    ("WordAudio".to_string(), "[sound:mizu.mp3]".to_string()),
                    ("Sentence".to_string(), "冷たい水".to_string()),
                ]),
            },
        ];

        let _ = app.update(Message::DeckSamplesLoaded(Ok(notes)));
        let first_preview = app
            .setup_state
            .current_preview_card()
            .expect("preview card should exist");
        assert_eq!(first_preview.word, "靴");
        assert_eq!(first_preview.kana, "くつ");
        assert_eq!(first_preview.word_audio, "kutsu.mp3");

        let _ = app.update(Message::MapKanaField("AltReading".to_string()));
        let _ = app.update(Message::MapSentenceField(NONE_FIELD_OPTION.to_string()));
        let updated_preview = app
            .setup_state
            .current_preview_card()
            .expect("preview card should exist");
        assert_eq!(updated_preview.kana, "クツ");
        assert_eq!(updated_preview.sentence, None);

        let _ = app.update(Message::NextPreviewCard);
        assert_eq!(app.setup_state.sample_index, 1);
        let second_preview = app
            .setup_state
            .current_preview_card()
            .expect("second preview card should exist");
        assert_eq!(second_preview.word, "水");
        assert_eq!(second_preview.kana, "ミズ");

        let _ = app.update(Message::PrevPreviewCard);
        assert_eq!(app.setup_state.sample_index, 0);
    }

    #[test]
    fn deck_import_persists_workspace_and_surfaces_gigo_warning_toast() -> anyhow::Result<()> {
        let (dir, settings_path, deck_path) = temp_config_paths("deck_import");
        let mut app = KotodamaApp::with_config_paths(settings_path, deck_path.clone());

        let mapping = FieldMapping {
            word_field: "Expression".to_string(),
            kana_field: "Reading".to_string(),
            sentence_field: None,
            word_audio_field: "WordAudio".to_string(),
            sentence_audio_field: None,
        };
        let config = DeckConfig {
            deck_name: "Core 2k".to_string(),
            source: DeckSource::AnkiConnect {
                endpoint_url: "http://127.0.0.1:8765".to_string(),
            },
            field_mapping: mapping.clone(),
            last_synced_at: Some(Utc::now()),
            flagged_audio_count: 1,
        };
        let card = AnkiCard {
            card_id: 11,
            note_id: 22,
            deck_name: "Core 2k".to_string(),
            word: "橋".to_string(),
            kana: "はし".to_string(),
            sentence: None,
            word_audio: "hashi.mp3".to_string(),
            sentence_audio: None,
            anki_queue: AnkiCard::LEARNED_QUEUE,
            audio_issues: vec![crate::models::AudioQualityIssue::MissingFile],
            last_pitch_error_percent: None,
        };
        let report = DeckAudioReport {
            total_cards: 1,
            total_clips_checked: 1,
            flagged_cards: 1,
            missing_files: 1,
            too_short_clips: 0,
            clipped_clips: 0,
            decode_failures: 0,
        };

        let _ = app.update(Message::DeckImportCompleted(Ok((
            config.clone(),
            vec![card],
            report,
        ))));

        assert_eq!(app.deck_workspace.active_deck.as_ref(), Some(&config));
        assert_eq!(app.loaded_cards.len(), 1);
        assert_eq!(app.active_practice_cards().len(), 1);
        assert_eq!(app.toasts.len(), 1);
        assert_eq!(app.toasts[0].severity, ErrorSeverity::Warning);

        let persisted = storage::load_deck_workspace(&deck_path)?;
        assert_eq!(persisted.active_deck, Some(config));
        assert_eq!(persisted.saved_mappings.get("Core 2k"), Some(&mapping));

        let _ = fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn card_exclusions_filter_practice_queue_and_persist_across_reloads() -> anyhow::Result<()> {
        let (dir, settings_path, deck_path) = temp_config_paths("card_exclusions");
        let mut app = KotodamaApp::with_config_paths(settings_path, deck_path.clone());

        let mapping = FieldMapping {
            word_field: "Expression".to_string(),
            kana_field: "Reading".to_string(),
            sentence_field: None,
            word_audio_field: "WordAudio".to_string(),
            sentence_audio_field: None,
        };
        let config = DeckConfig {
            deck_name: "Core 2k".to_string(),
            source: DeckSource::AnkiConnect {
                endpoint_url: "http://127.0.0.1:8765".to_string(),
            },
            field_mapping: mapping,
            last_synced_at: Some(Utc::now()),
            flagged_audio_count: 1,
        };
        let clean_card = AnkiCard {
            card_id: 10,
            note_id: 100,
            deck_name: "Core 2k".to_string(),
            word: "水".to_string(),
            kana: "みず".to_string(),
            sentence: None,
            word_audio: "mizu.mp3".to_string(),
            sentence_audio: None,
            anki_queue: AnkiCard::LEARNED_QUEUE,
            audio_issues: Vec::new(),
            last_pitch_error_percent: None,
        };
        let bad_card = AnkiCard {
            card_id: 11,
            note_id: 101,
            deck_name: "Core 2k".to_string(),
            word: "橋".to_string(),
            kana: "はし".to_string(),
            sentence: None,
            word_audio: "missing.mp3".to_string(),
            sentence_audio: None,
            anki_queue: AnkiCard::LEARNED_QUEUE,
            audio_issues: vec![crate::models::AudioQualityIssue::MissingFile],
            last_pitch_error_percent: None,
        };
        let report = DeckAudioReport {
            total_cards: 2,
            total_clips_checked: 2,
            flagged_cards: 1,
            missing_files: 1,
            too_short_clips: 0,
            clipped_clips: 0,
            decode_failures: 0,
        };

        let _ = app.update(Message::DeckImportCompleted(Ok((
            config,
            vec![clean_card.clone(), bad_card],
            report,
        ))));
        assert_eq!(app.active_practice_cards().len(), 2);

        let _ = app.update(Message::AutoExcludeInvalidAudioCards);
        assert_eq!(app.excluded_count_for_active_deck(), 1);
        assert_eq!(app.active_practice_cards(), vec![&clean_card]);

        let reloaded = storage::load_deck_workspace(&deck_path)?;
        assert!(reloaded.is_card_excluded("Core 2k", 11));
        assert_eq!(reloaded.filter_active_cards("Core 2k", &app.loaded_cards).len(), 1);

        app.setup_state.selected_deck_name = Some("Core 2k".to_string());
        app.setup_state.sample_notes = vec![AnkiRawNote {
            card_id: 10,
            note_id: 100,
            deck_name: "Core 2k".to_string(),
            note_type_id: 1,
            anki_queue: AnkiCard::LEARNED_QUEUE,
            fields: std::collections::BTreeMap::from([
                ("Expression".to_string(), "水".to_string()),
                ("Reading".to_string(), "みず".to_string()),
                ("WordAudio".to_string(), "[sound:mizu.mp3]".to_string()),
            ]),
        }];
        app.setup_state.draft_mapping = reloaded
            .active_deck
            .as_ref()
            .expect("active deck should exist")
            .field_mapping
            .clone();

        let _ = app.update(Message::ExcludePreviewCard);
        assert_eq!(app.excluded_count_for_active_deck(), 2);
        assert!(app.active_practice_cards().is_empty());

        let _ = app.update(Message::RestoreCard {
            deck_name: "Core 2k".to_string(),
            card_id: 10,
        });
        assert_eq!(app.excluded_count_for_active_deck(), 1);
        assert_eq!(app.active_practice_cards(), vec![&clean_card]);

        let _ = app.update(Message::RestoreAllExcludedCards("Core 2k".to_string()));
        assert_eq!(app.excluded_count_for_active_deck(), 0);
        assert_eq!(app.active_practice_cards().len(), 2);

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
        let _ = app.update(Message::CollectionFilePicked(Err("Portal closed".to_string())));
        let _ = app.update(Message::CollectionLoaded(Err("Bad SQLite".to_string())));
        let _ = app.update(Message::AudioPlaybackFinished(Err("No device".to_string())));
        assert_eq!(app.toasts.len(), 5);
        assert_eq!(app.toasts[0].severity, ErrorSeverity::Error);

        let first_id = app.toasts[0].id;
        let _ = app.update(Message::DismissToast(first_id));
        assert_eq!(app.toasts.len(), 4);
        assert_ne!(app.toasts[0].id, first_id);

        let _ = app.update(Message::ClearAllToasts);
        assert!(app.toasts.is_empty());
    }

    #[test]
    fn mic_calibration_settings_update_and_persist() -> anyhow::Result<()> {
        let (dir, settings_path, deck_path) = temp_config_paths("mic_calibration");
        let mut app = KotodamaApp::with_config_paths(settings_path.clone(), deck_path);

        let _ = app.update(Message::SetVadRmsThreshold(0.045));
        assert!((app.settings.vad_rms_threshold - 0.045).abs() < 1e-6);

        let _ = app.update(Message::SetVadRmsThreshold(0.99));
        assert!((app.settings.vad_rms_threshold - 0.15).abs() < 1e-6);

        let _ = app.update(Message::SetVadSilenceMs(600));
        assert_eq!(app.settings.vad_silence_ms, 600);

        let _ = app.update(Message::SelectInputDevice("USB Podcast Mic".to_string()));
        assert_eq!(
            app.settings.preferred_input_device.as_deref(),
            Some("USB Podcast Mic")
        );

        let _ = app.update(Message::SelectInputDevice(
            SYSTEM_DEFAULT_MIC_OPTION.to_string(),
        ));
        assert_eq!(app.settings.preferred_input_device, None);

        let _ = app.update(Message::InputDevicesLoaded(Ok(vec![InputDeviceInfo {
            name: "Built-in Mic".to_string(),
            is_default: true,
        }])));
        assert_eq!(app.input_devices.len(), 1);

        let _ = app.update(Message::InputDevicesLoaded(Err(
            "Audio daemon unavailable".to_string(),
        )));
        assert_eq!(app.toasts.len(), 1);

        let persisted = storage::load_settings(&settings_path)?;
        assert!((persisted.vad_rms_threshold - 0.15).abs() < 1e-6);
        assert_eq!(persisted.vad_silence_ms, 600);
        assert_eq!(persisted.preferred_input_device, None);

        let _ = fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn fsrs_threshold_presets_update_and_validate() -> anyhow::Result<()> {
        let (dir, settings_path, deck_path) = temp_config_paths("fsrs_thresholds");
        let mut app = KotodamaApp::with_config_paths(settings_path.clone(), deck_path);

        let _ = app.update(Message::SetScoreThresholds(ScoreThresholds::strict()));
        assert_eq!(app.settings.score_thresholds, ScoreThresholds::strict());
        assert!(app.toasts.is_empty());

        let invalid = ScoreThresholds {
            hard_min: 80.0,
            good_min: 70.0,
            easy_min: 90.0,
        };
        let _ = app.update(Message::SetScoreThresholds(invalid));
        assert_eq!(app.settings.score_thresholds, ScoreThresholds::strict());
        assert_eq!(app.toasts.len(), 1);
        assert_eq!(app.toasts[0].severity, ErrorSeverity::Error);

        let persisted = storage::load_settings(&settings_path)?;
        assert_eq!(persisted.score_thresholds, ScoreThresholds::strict());

        let _ = fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn export_completion_and_error_handlers_update_status_and_toasts() {
        let mut app = KotodamaApp::default();

        let _ = app.update(Message::CsvExportCompleted(Ok(None)));
        assert!(app.last_export_status.is_none());
        assert!(app.toasts.is_empty());

        let report = CsvExportReport {
            reviews_path: PathBuf::from("/tmp/exports/kotodama_review_history.csv"),
            kana_confusion_path: PathBuf::from("/tmp/exports/kotodama_kana_confusion.csv"),
            pitch_errors_path: PathBuf::from("/tmp/exports/kotodama_pitch_errors.csv"),
            review_rows: 3,
            kana_rows: 5,
            pitch_rows: 2,
        };
        let _ = app.update(Message::CsvExportCompleted(Ok(Some(report))));
        assert!(
            app.last_export_status
                .as_deref()
                .is_some_and(|s| s.contains("3 reviews, 5 kana rows, 2 pitch rows"))
        );

        let _ = app.update(Message::JsonExportCompleted(Ok(Some(PathBuf::from(
            "/tmp/exports/kotodama_export.json",
        )))));
        assert!(
            app.last_export_status
                .as_deref()
                .is_some_and(|s| s.contains("kotodama_export.json"))
        );

        let _ = app.update(Message::CsvExportCompleted(Err("Permission denied".to_string())));
        let _ = app.update(Message::JsonExportCompleted(Err("Disk full".to_string())));
        assert_eq!(app.toasts.len(), 2);
        assert!(app.toasts.iter().all(|t| t.severity == ErrorSeverity::Error));
    }

    fn sample_practice_card(card_id: i64, word: &str, kana: &str) -> AnkiCard {
        AnkiCard {
            card_id,
            note_id: card_id * 10,
            deck_name: "Core 2k".to_string(),
            word: word.to_string(),
            kana: kana.to_string(),
            sentence: Some(format!("{word}の例文")),
            word_audio: format!("word_{card_id}.mp3"),
            sentence_audio: Some(format!("sent_{card_id}.mp3")),
            anki_queue: AnkiCard::LEARNED_QUEUE,
            audio_issues: Vec::new(),
            last_pitch_error_percent: None,
        }
    }

    #[test]
    fn practice_shortcuts_toggle_blind_speed_and_cancel_countdown() {
        let mut app = KotodamaApp::default();
        assert!(!app.settings.blind_mode);
        assert_eq!(app.settings.playback_speed, PlaybackSpeed::Normal);

        let _ = app.update(Message::PracticeKeyShortcut(
            PracticeShortcutAction::ToggleBlindMode,
        ));
        let _ = app.update(Message::PracticeKeyShortcut(
            PracticeShortcutAction::TogglePlaybackSpeed,
        ));
        assert!(app.settings.blind_mode);
        assert_eq!(app.settings.playback_speed, PlaybackSpeed::Slow);

        app.practice_state.begin_session(
            vec![sample_practice_card(1, "水", "みず")],
            PracticeMode::CombinedShadowing,
            true,
            false,
            Some(1),
        );
        assert!(!app.practice_state.text_revealed);

        app.practice_state.start_countdown(Instant::now());
        assert!(app.practice_state.countdown.is_some());

        let _ = app.update(Message::PracticeKeyShortcut(
            PracticeShortcutAction::CancelActive,
        ));
        assert!(app.practice_state.countdown.is_none());

        let _ = app.update(Message::PracticeRevealText);
        assert!(app.practice_state.text_revealed);

        let _ = app.update(Message::PracticeKeyShortcut(
            PracticeShortcutAction::ToggleBlindMode,
        ));
        assert!(!app.settings.blind_mode);
    }

    #[test]
    fn practice_daily_goal_minutes_and_card_cap_enforce_and_allow_extra_override() -> anyhow::Result<()> {
        let (dir, settings_path, deck_path) = temp_config_paths("practice_cap");
        let mut app = KotodamaApp::with_config_paths(settings_path.clone(), deck_path);

        let _ = app.update(Message::SetDailyGoalMinutes(5));
        assert_eq!(app.settings.daily_goal, DailyGoal::Minutes(5));
        assert_eq!(app.streak.daily_goal, DailyGoal::Minutes(5));
        assert_eq!(storage::load_settings(&settings_path)?.daily_goal, DailyGoal::Minutes(5));

        app.practice_state.begin_session(
            vec![
                sample_practice_card(1, "橋", "はし"),
                sample_practice_card(2, "雨", "あめ"),
            ],
            PracticeMode::PitchAccent,
            false,
            false,
            Some(2),
        );
        app.practice_state.session_elapsed_ms = 299_000;
        let _ = app.update(Message::PracticeSessionTick);
        assert_eq!(
            app.practice_state.phase,
            PracticePhase::SessionComplete { cap_reached: true }
        );
        assert!(app.practice_state.summary.daily_cap_reached);

        let _ = app.update(Message::PracticeContinueExtra);
        assert!(app.practice_state.cap_override_active);
        assert_eq!(app.practice_state.phase, PracticePhase::ActiveCard);

        let mut db = KotodamaDb::open_in_memory()?;
        let scheduler = FsrsScheduler::default_scheduler()?;
        let breakdown = pitch::combine_practice_score(
            PracticeMode::PitchAccent,
            None,
            Some(12.0),
            &app.settings.score_thresholds,
        )?;
        let card = sample_practice_card(1, "橋", "はし");
        let (_id, outcome, mut streak) = scheduler.record_review_with_streak_minutes(
            &mut db,
            &card,
            &breakdown,
            &[],
            &app.settings.score_thresholds,
            Some(FsrsRating::Good),
            Some(0.72),
            3500,
            0,
            DailyGoal::Cards(1),
            Utc::now(),
        )?;
        streak.daily_goal = DailyGoal::Cards(1);
        app.streak.daily_goal = DailyGoal::Cards(1);
        app.practice_state.cap_override_active = false;
        let card_generation = app.practice_state.card_generation;
        let _ = app.update(Message::PracticeReviewCommitted {
            card_generation,
            was_manual_override: false,
            result: Ok((outcome, streak, Some(12.0))),
        });
        assert_eq!(
            app.practice_state.phase,
            PracticePhase::SessionComplete { cap_reached: true }
        );
        assert_eq!(app.practice_state.summary.cards_completed, 1);

        let _ = fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn practice_deck_prepared_and_error_paths_handle_missing_or_failed_inputs_gracefully() {
        let mut app = KotodamaApp::default();

        let _ = app.update(Message::PracticeStartSession { extra_study: false });
        assert_eq!(app.practice_state.phase, PracticePhase::Lobby);
        assert_eq!(app.toasts.len(), 1);
        assert_eq!(app.toasts[0].severity, ErrorSeverity::Warning);

        let _ = app.update(Message::ClearAllToasts);
        app.setup_state.selected_deck_name = Some("Core 2k".to_string());
        app.practice_state.phase = PracticePhase::Loading {
            label: "Loading...".to_string(),
        };
        let card = sample_practice_card(10, "靴", "くつ");
        let _ = app.update(Message::PracticeDeckPrepared(Ok((
            vec![card.clone()],
            vec![10],
            BTreeMap::from([(10, 14.5)]),
            None,
            None,
            [1, 1, 2, 4],
        ))));
        assert_eq!(app.practice_state.phase, PracticePhase::ActiveCard);
        assert_eq!(
            app.practice_state
                .current_card()
                .and_then(|c| c.last_pitch_error_percent),
            Some(14.5)
        );
        assert!(app.practice_state.reference_audio_error.is_some());

        let _ = app.update(Message::PracticePlayUserTake);
        assert_eq!(app.toasts.len(), 1);

        let take_gen = app.practice_state.take_generation;
        let _ = app.update(Message::PracticeTakeEvaluated {
            take_generation: take_gen,
            result: Err("No voiced pitch frames detected".to_string()),
        });
        assert_eq!(app.toasts.len(), 2);
        assert_eq!(app.toasts[1].severity, ErrorSeverity::Error);

        let card_gen = app.practice_state.card_generation;
        let _ = app.update(Message::PracticeReviewCommitted {
            card_generation: card_gen,
            was_manual_override: false,
            result: Err("SQLite database locked".to_string()),
        });
        assert_eq!(app.toasts.len(), 3);
        assert_eq!(app.toasts[2].severity, ErrorSeverity::Error);
    }

    #[test]
    fn practice_take_scoring_evaluates_synthetic_audio_across_all_modes() -> anyhow::Result<()> {
        let rate = 16_000_u32;
        let len = (rate / 2) as usize;
        let make_tone = |freq: f32| DecodedAudio {
            samples: (0..len)
                .map(|i| (std::f32::consts::TAU * freq * i as f32 / rate as f32).sin() * 0.4)
                .collect(),
            sample_rate: rate,
            channels: 1,
        };
        let ref_audio = make_tone(180.0);
        let user_audio = make_tone(210.0);
        let card = sample_practice_card(3, "水", "みず");
        let thresholds = ScoreThresholds::default();

        for mode in PracticeMode::ALL {
            let eval = evaluate_practice_take(
                &card,
                mode,
                user_audio.clone(),
                Some(ref_audio.clone()),
                None,
                &thresholds,
                500,
            )?;
            assert_eq!(eval.card_id, 3);
            assert_eq!(eval.mode, mode);
            assert!(eval.breakdown.combined_score_percent.is_finite());
        }
        Ok(())
    }

    #[test]
    fn dashboard_messages_update_snapshot_filters_and_handle_errors_safely() {
        let mut app = KotodamaApp::default();
        app.settings.daily_goal = DailyGoal::Minutes(15);
        app.dashboard_state.is_loading = true;
        app.dashboard_state.selected_kana = Some("ぬ".to_string());
        app.dashboard_state.selected_pitch_card_id = Some(999);

        let snapshot = DashboardSnapshot {
            streak: DailyStreak {
                current_streak_days: 4,
                longest_streak_days: 7,
                total_sessions: 12,
                cards_practiced_today: 5,
                minutes_practiced_today: 8,
                last_practice_date: Some(
                    chrono::NaiveDate::from_ymd_opt(2026, 11, 27).unwrap_or(chrono::NaiveDate::MIN),
                ),
                daily_goal: DailyGoal::Cards(10),
            },
            total_reviews: 12,
            overall_mora_accuracy_percent: Some(88.5),
            overall_pitch_error_percent: Some(14.0),
            tracked_kana_count: 1,
            daily_series: Vec::new(),
            kana_stats: vec![crate::models::KanaStats::new("つ")],
            card_pitch_summaries: vec![crate::models::CardPitchSummary {
                card_id: 42,
                deck_name: "Core 2k".to_string(),
                word: "橋".to_string(),
                kana: "はし".to_string(),
                review_count: 2,
                latest_pitch_error_percent: 14.0,
                average_pitch_error_percent: 16.0,
                best_pitch_error_percent: 14.0,
                latest_dtw_distance: 0.7,
                last_reviewed_at: Utc::now(),
                recent_errors: vec![18.0, 14.0],
            }],
            pitch_error_logs: Vec::new(),
        };

        let _ = app.update(Message::DashboardLoaded(Ok(snapshot)));
        assert!(!app.dashboard_state.is_loading);
        assert!(app.dashboard_state.has_loaded);
        assert_eq!(app.streak.current_streak_days, 4);
        assert_eq!(app.streak.daily_goal, DailyGoal::Minutes(15));
        assert_eq!(
            app.dashboard_state.snapshot.streak.daily_goal,
            DailyGoal::Minutes(15)
        );
        assert_eq!(app.dashboard_state.selected_kana, None);
        assert_eq!(app.dashboard_state.selected_pitch_card_id, None);

        let _ = app.update(Message::DashboardSelectTimeRange(DashboardTimeRange::Last7Days));
        let _ = app.update(Message::DashboardSelectKanaSort(KanaSortOrder::MostPracticed));
        let _ = app.update(Message::DashboardKanaFilterChanged("つ".to_string()));
        let _ = app.update(Message::DashboardSelectKana(Some("つ".to_string())));
        let _ = app.update(Message::DashboardPitchCardFilterChanged("橋".to_string()));
        let _ = app.update(Message::DashboardSelectPitchCard(Some(42)));

        assert_eq!(app.dashboard_state.time_range, DashboardTimeRange::Last7Days);
        assert_eq!(app.dashboard_state.kana_sort, KanaSortOrder::MostPracticed);
        assert_eq!(app.dashboard_state.kana_filter, "つ");
        assert_eq!(app.dashboard_state.selected_kana.as_deref(), Some("つ"));
        assert_eq!(app.dashboard_state.pitch_card_filter, "橋");
        assert_eq!(app.dashboard_state.selected_pitch_card_id, Some(42));

        app.dashboard_state.is_loading = true;
        let _ = app.update(Message::DashboardLoaded(Err("Database locked".to_string())));
        assert!(!app.dashboard_state.is_loading);
        assert_eq!(app.toasts.len(), 1);
        assert_eq!(app.toasts[0].severity, ErrorSeverity::Error);
    }
}

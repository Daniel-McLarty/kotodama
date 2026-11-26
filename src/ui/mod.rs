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

use anyhow::Error;
use chrono::Utc;
use iced::alignment::{Horizontal, Vertical};
use iced::widget::{button, column, container, row, scrollable, space, stack, text, toggler};
use iced::{Alignment, Element, Length, Subscription, Task, Theme};

use crate::anki::{reader, validator};
use crate::audio::playback;
use crate::models::{
    AnkiCard, AnkiCollectionSummary, AnkiRawNote, AppErrorToast, AppSettings, DailyGoal,
    DailyStreak, DeckAudioReport, DeckConfig, DeckWorkspaceConfig, FieldMapping, NavTab,
    PlaybackSpeed, PracticeMode, ResolvedTheme, ThemePreference,
};
use crate::storage;
use crate::ui::setup::{NONE_FIELD_OPTION, SetupSourceMode, SetupState};
use crate::ui::theme::ThemeTokens;

const SAMPLE_PREVIEW_LIMIT: usize = 25;

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
    /// Imports all graduated cards for the mapped deck, validates audio, and saves config.
    ValidateAndSaveDeck,
    /// Receives the validated deck config, learned cards, and GIGO audio report.
    DeckImportCompleted(Result<(DeckConfig, Vec<AnkiCard>, DeckAudioReport), String>),
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
    saved_mappings: BTreeMap<String, FieldMapping>,
    loaded_cards: Vec<AnkiCard>,
    setup_state: SetupState,
    streak: DailyStreak,
    settings: AppSettings,
    system_is_dark: Option<bool>,
    settings_path: Option<PathBuf>,
    deck_config_path: Option<PathBuf>,
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
            saved_mappings: BTreeMap::new(),
            loaded_cards: Vec::new(),
            setup_state: SetupState::default(),
            streak,
            settings,
            system_is_dark: None,
            settings_path: None,
            deck_config_path: None,
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
                        app.deck_config = workspace.active_deck;
                        app.saved_mappings = workspace.saved_mappings;
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

        (app, iced::system::theme().map(Message::SystemThemeChanged))
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
                        self.setup_state.set_sample_notes(notes, &self.saved_mappings);
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
                        self.saved_mappings
                            .insert(config.deck_name.clone(), config.field_mapping.clone());
                        self.deck_config = Some(config);
                        self.loaded_cards = cards;
                        self.setup_state.audio_report = Some(report.clone());
                        self.persist_deck_workspace();

                        if report.has_issues() {
                            self.report_warning(
                                "Audio Quality Warnings Detected",
                                report.summary_line(),
                                Some("Review the Setup preview or replace flagged clips in Anki for best grading accuracy.".to_string()),
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
            Message::TriggerRecoverableAction => {
                let message = if self.deck_config.is_some() {
                    "Audio recording, pitch DSP, and analytics export are scheduled for Phases 3–6."
                } else {
                    "Connect and validate an Anki deck in Setup first, then check back as Phases 3–6 land."
                };
                self.report_warning(
                    "Upcoming Practice & Export Feature",
                    message,
                    Some("Your deck and field mappings are ready in Setup.".to_string()),
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
            NavTab::Setup => setup::view(
                &self.setup_state,
                self.deck_config.as_ref(),
                self.loaded_cards.len(),
                resolved_theme,
            ),
            NavTab::Practice => practice::view(
                self.active_mode,
                &self.settings,
                &self.streak,
                self.loaded_cards.len(),
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

    fn sync_active_mapping_changes(&mut self) {
        if let Some(deck_name) = &self.setup_state.selected_deck_name {
            self.saved_mappings
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
        Task::perform(
            run_blocking(move || playback::play_audio_file(&audio_path)),
            Message::AudioPlaybackFinished,
        )
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

        let workspace = DeckWorkspaceConfig {
            active_deck: self.deck_config.clone(),
            saved_mappings: self.saved_mappings.clone(),
        };

        if let Err(err) = storage::save_deck_workspace(path, &workspace) {
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
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::models::{AnkiDeckSummary, AnkiNoteType, DeckSource, ErrorSeverity};

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

        assert_eq!(app.deck_config.as_ref(), Some(&config));
        assert_eq!(app.loaded_cards.len(), 1);
        assert_eq!(app.toasts.len(), 1);
        assert_eq!(app.toasts[0].severity, ErrorSeverity::Warning);

        let persisted = storage::load_deck_workspace(&deck_path)?;
        assert_eq!(persisted.active_deck, Some(config));
        assert_eq!(persisted.saved_mappings.get("Core 2k"), Some(&mapping));

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
}

// SPDX-License-Identifier: MPL-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::PathBuf;

use anyhow::{Result, bail};
use iced::widget::{button, column, container, pick_list, row, space, text, text_input};
use iced::{Alignment, Element, Length};

use crate::anki::{reader, validator};
use crate::models::{
    AnkiCard, AnkiCollectionSummary, AnkiRawNote, AudioQualityIssue, DeckAudioReport, DeckConfig,
    DeckSource, DeckWorkspaceConfig, FieldMapping, ResolvedTheme,
};
use crate::ui::Message;
use crate::ui::theme::{self, ThemeTokens};

/// Sentinel label representing an unset optional note field mapping.
pub const NONE_FIELD_OPTION: &str = "(None)";

/// Default local AnkiConnect HTTP endpoint URL.
pub const DEFAULT_ANKICONNECT_URL: &str = "http://127.0.0.1:8765";

/// Active Anki connection mode selected in the Setup view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupSourceMode {
    /// Local `collection.anki2` SQLite file and `collection.media` folder.
    LocalCollection,
    /// Running AnkiConnect HTTP endpoint.
    AnkiConnect,
}

impl SetupSourceMode {
    /// Ordered list of available Anki connection modes.
    pub const ALL: [Self; 2] = [Self::LocalCollection, Self::AnkiConnect];

    /// Display label for source mode toggle pills.
    pub const fn label(self) -> &'static str {
        match self {
            Self::LocalCollection => "Local collection.anki2",
            Self::AnkiConnect => "AnkiConnect (localhost:8765)",
        }
    }
}

/// Displayable dropdown option for selecting an Anki deck.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeckPickerOption {
    /// Exact deck name inside the Anki collection.
    pub deck_name: String,
    /// Optional count of unique learned (`queue = 2`) notes in the deck.
    pub learned_card_count: Option<usize>,
}

impl fmt::Display for DeckPickerOption {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.learned_card_count {
            Some(count) => write!(f, "{} ({} learned)", self.deck_name, count),
            None => write!(f, "{}", self.deck_name),
        }
    }
}

/// Mutable state backing the interactive Setup, Field Mapper, and Live Preview view.
#[derive(Debug, Clone)]
pub struct SetupState {
    /// Selected Anki source mode.
    pub source_mode: SetupSourceMode,
    /// User-entered path to `collection.anki2`.
    pub collection_path_input: String,
    /// User-entered path to `collection.media`.
    pub media_dir_input: String,
    /// User-entered AnkiConnect URL.
    pub ankiconnect_url_input: String,
    /// Loaded collection metadata (decks and note types).
    pub collection: Option<AnkiCollectionSummary>,
    /// Currently selected deck name in the mapper.
    pub selected_deck_name: Option<String>,
    /// Ordered field names available for dropdown mapping.
    pub available_fields: Vec<String>,
    /// Draft field mapping edited in the dropdowns.
    pub draft_mapping: FieldMapping,
    /// Sample learned (`queue = 2`) notes loaded for live preview.
    pub sample_notes: Vec<AnkiRawNote>,
    /// Index of the currently previewed sample note.
    pub sample_index: usize,
    /// Live GIGO audio quality issues for the current preview card.
    pub preview_audio_issues: Vec<AudioQualityIssue>,
    /// Most recent full-deck GIGO validation report.
    pub audio_report: Option<DeckAudioReport>,
    /// Whether an async inspection/import operation is running.
    pub is_busy: bool,
    /// Human-readable status text describing the active background task.
    pub busy_label: Option<String>,
    /// Whether a sample test audio clip is currently playing.
    pub is_playing_audio: bool,
}

impl Default for SetupState {
    fn default() -> Self {
        Self {
            source_mode: SetupSourceMode::LocalCollection,
            collection_path_input: String::new(),
            media_dir_input: String::new(),
            ankiconnect_url_input: DEFAULT_ANKICONNECT_URL.to_string(),
            collection: None,
            selected_deck_name: None,
            available_fields: Vec::new(),
            draft_mapping: FieldMapping {
                word_field: String::new(),
                kana_field: String::new(),
                sentence_field: None,
                word_audio_field: String::new(),
                sentence_audio_field: None,
            },
            sample_notes: Vec::new(),
            sample_index: 0,
            preview_audio_issues: Vec::new(),
            audio_report: None,
            is_busy: false,
            busy_label: None,
            is_playing_audio: false,
        }
    }
}

impl SetupState {
    /// Restores saved paths, deck name, and field mappings from a persisted [`DeckWorkspaceConfig`].
    pub fn apply_workspace(&mut self, workspace: &DeckWorkspaceConfig) {
        let Some(active) = &workspace.active_deck else {
            return;
        };

        match &active.source {
            DeckSource::LocalCollection {
                collection_path,
                media_dir,
            } => {
                self.source_mode = SetupSourceMode::LocalCollection;
                self.collection_path_input = collection_path.display().to_string();
                self.media_dir_input = media_dir.display().to_string();
            }
            DeckSource::AnkiConnect { endpoint_url } => {
                self.source_mode = SetupSourceMode::AnkiConnect;
                self.ankiconnect_url_input = endpoint_url.clone();
            }
        }

        self.selected_deck_name = Some(active.deck_name.clone());
        self.draft_mapping = active.field_mapping.clone();
        self.seed_fields_from_mapping(&active.field_mapping);
    }

    /// Builds a validated [`DeckSource`] from the active source inputs.
    pub fn build_deck_source(&self) -> Result<DeckSource> {
        match self.source_mode {
            SetupSourceMode::LocalCollection => {
                let raw_collection = self.collection_path_input.trim();
                if raw_collection.is_empty() {
                    bail!("Please select or enter a path to collection.anki2");
                }
                let collection_path = PathBuf::from(raw_collection);
                let raw_media = self.media_dir_input.trim();
                let media_dir = if raw_media.is_empty() {
                    reader::infer_media_dir(&collection_path)
                } else {
                    PathBuf::from(raw_media)
                };
                Ok(DeckSource::LocalCollection {
                    collection_path,
                    media_dir,
                })
            }
            SetupSourceMode::AnkiConnect => {
                let url = self.ankiconnect_url_input.trim();
                if !url.starts_with("http://") && !url.starts_with("https://") {
                    bail!("AnkiConnect URL must start with http:// or https://");
                }
                Ok(DeckSource::AnkiConnect {
                    endpoint_url: url.to_string(),
                })
            }
        }
    }

    /// Returns the media directory path used for audio pre-flight validation and preview playback.
    pub fn effective_media_dir(&self) -> Option<PathBuf> {
        let trimmed = self.media_dir_input.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed));
        }

        if self.source_mode == SetupSourceMode::LocalCollection {
            let col_trimmed = self.collection_path_input.trim();
            if !col_trimmed.is_empty() {
                return Some(reader::infer_media_dir(&PathBuf::from(col_trimmed)));
            }
        }

        None
    }

    /// Stores inspected collection metadata and auto-fills discovered media directory paths.
    pub fn set_collection(
        &mut self,
        summary: AnkiCollectionSummary,
        discovered_media_dir: Option<PathBuf>,
    ) {
        if let Some(media_dir) = discovered_media_dir
            && !media_dir.as_os_str().is_empty()
            && (self.source_mode == SetupSourceMode::AnkiConnect
                || self.media_dir_input.trim().is_empty())
        {
            self.media_dir_input = media_dir.display().to_string();
        }

        let keep_selected = self.selected_deck_name.as_ref().is_some_and(|selected| {
            summary.decks.iter().any(|deck| &deck.name == selected)
        });
        if !keep_selected {
            self.selected_deck_name = None;
            self.sample_notes.clear();
            self.sample_index = 0;
            self.preview_audio_issues.clear();
        }

        self.collection = Some(summary);
    }

    /// Updates preview sample notes, refreshes available note fields, and selects a deck mapping.
    pub fn set_sample_notes(
        &mut self,
        notes: Vec<AnkiRawNote>,
        saved_mappings: &BTreeMap<String, FieldMapping>,
    ) {
        self.sample_notes = notes;
        self.sample_index = 0;
        self.available_fields = self.collect_available_fields();

        if let Some(deck_name) = &self.selected_deck_name
            && let Some(saved) = saved_mappings.get(deck_name)
            && self.mapping_fits_available_fields(saved)
        {
            self.draft_mapping = saved.clone();
        } else if !self.available_fields.is_empty() {
            self.draft_mapping =
                reader::suggest_field_mapping(&self.available_fields, &self.sample_notes);
        }

        self.refresh_preview_audio_issues();
    }

    /// Re-evaluates audio quality issues for the currently displayed sample preview card.
    pub fn refresh_preview_audio_issues(&mut self) {
        let Some(note) = self.sample_notes.get(self.sample_index) else {
            self.preview_audio_issues.clear();
            return;
        };

        let Some(media_dir) = self.effective_media_dir() else {
            self.preview_audio_issues.clear();
            return;
        };

        let mut card = reader::build_anki_card(note, &self.draft_mapping);
        let _ = validator::validate_card_audio(&mut card, &media_dir);
        self.preview_audio_issues = card.audio_issues;
    }

    /// Builds the live [`AnkiCard`] preview from the active sample note and draft field mapping.
    pub fn current_preview_card(&self) -> Option<AnkiCard> {
        let note = self.sample_notes.get(self.sample_index)?;
        let mut card = reader::build_anki_card(note, &self.draft_mapping);
        card.audio_issues = self.preview_audio_issues.clone();
        Some(card)
    }

    /// Returns field dropdown options prefixed with `"(None)"` for optional fields.
    pub fn optional_field_options(&self) -> Vec<String> {
        let mut options = Vec::with_capacity(self.available_fields.len() + 1);
        options.push(NONE_FIELD_OPTION.to_string());
        options.extend(self.available_fields.iter().cloned());
        options
    }

    fn seed_fields_from_mapping(&mut self, mapping: &FieldMapping) {
        let mut fields = BTreeSet::new();
        for name in [
            Some(mapping.word_field.as_str()),
            Some(mapping.kana_field.as_str()),
            mapping.sentence_field.as_deref(),
            Some(mapping.word_audio_field.as_str()),
            mapping.sentence_audio_field.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            let trimmed = name.trim();
            if !trimmed.is_empty() {
                fields.insert(trimmed.to_string());
            }
        }
        self.available_fields = fields.into_iter().collect();
    }

    fn collect_available_fields(&self) -> Vec<String> {
        let mut ordered = Vec::new();
        let mut seen = BTreeSet::new();

        if let Some(first_note) = self.sample_notes.first()
            && let Some(collection) = &self.collection
            && let Some(nt) = collection
                .note_types
                .iter()
                .find(|nt| nt.id == first_note.note_type_id)
        {
            for name in &nt.field_names {
                if seen.insert(name.clone()) {
                    ordered.push(name.clone());
                }
            }
        }

        for note in &self.sample_notes {
            for name in note.fields.keys() {
                if seen.insert(name.clone()) {
                    ordered.push(name.clone());
                }
            }
        }

        if ordered.is_empty()
            && let Some(collection) = &self.collection
        {
            for nt in &collection.note_types {
                for name in &nt.field_names {
                    if seen.insert(name.clone()) {
                        ordered.push(name.clone());
                    }
                }
            }
        }

        ordered
    }

    fn mapping_fits_available_fields(&self, mapping: &FieldMapping) -> bool {
        if self.available_fields.is_empty() {
            return true;
        }
        self.available_fields.contains(&mapping.word_field)
            && self.available_fields.contains(&mapping.kana_field)
            && self.available_fields.contains(&mapping.word_audio_field)
    }
}

/// Renders the interactive Anki deck setup, field mapper, GIGO notice, and live preview view.
pub fn view<'a>(
    state: &'a SetupState,
    deck_config: Option<&'a DeckConfig>,
    loaded_card_count: usize,
    resolved_theme: ResolvedTheme,
) -> Element<'a, Message> {
    let colors = theme::tokens(resolved_theme);

    let gigo_banner = render_gigo_banner(state.audio_report.as_ref(), colors);
    let source_card = render_source_card(state, deck_config, loaded_card_count, colors);
    let mapper_card = render_field_mapper_card(state, colors);
    let preview_card = render_preview_card(state, colors);

    column![gigo_banner, source_card, mapper_card, preview_card]
        .spacing(18)
        .width(Length::Fill)
        .into()
}

fn render_gigo_banner<'a>(
    audio_report: Option<&'a DeckAudioReport>,
    colors: &'static ThemeTokens,
) -> Element<'a, Message> {
    let mut header = row![
        text("Audio Quality Notice (Garbage-In, Garbage-Out)")
            .size(17)
            .color(colors.amber),
        space().width(Length::Fill),
    ]
    .align_y(Alignment::Center);

    if let Some(report) = audio_report {
        let badge = if report.has_issues() {
            container(
                text(format!("{} flagged", report.flagged_cards))
                    .size(12)
                    .color(colors.amber),
            )
            .padding([4, 10])
            .style(theme::badge_style(colors.amber_soft, colors.amber))
        } else {
            container(text("Audio checks passed").size(12).color(colors.sage))
                .padding([4, 10])
                .style(theme::badge_style(colors.sage_soft, colors.sage))
        };
        header = header.push(badge);
    }

    let mut content = column![
        header,
        text("Kotodama grades your pronunciation and pitch contour directly against your Anki deck's audio. Missing clips, files shorter than 200 ms, or clipped recordings are flagged automatically before practice.")
            .size(14)
            .color(colors.text_primary),
    ]
    .spacing(8);

    if let Some(report) = audio_report {
        let report_color = if report.has_issues() {
            colors.amber
        } else {
            colors.sage
        };
        content = content.push(text(report.summary_line()).size(13).color(report_color));
    }

    container(content)
        .width(Length::Fill)
        .padding(16)
        .style(theme::gigo_banner_style)
        .into()
}

fn render_source_card<'a>(
    state: &'a SetupState,
    deck_config: Option<&'a DeckConfig>,
    loaded_card_count: usize,
    colors: &'static ThemeTokens,
) -> Element<'a, Message> {
    let mode_pills = SetupSourceMode::ALL.into_iter().fold(
        row![].spacing(10).align_y(Alignment::Center),
        |r, mode| {
            r.push(
                button(text(mode.label()).size(13))
                    .padding([8, 14])
                    .style(theme::pill_button_style(state.source_mode == mode))
                    .on_press(Message::SelectSetupSourceMode(mode)),
            )
        },
    );

    let mut header = row![
        text("1. Connect Anki Source")
            .size(19)
            .color(colors.text_primary),
        space().width(Length::Fill),
    ]
    .align_y(Alignment::Center);

    if let Some(config) = deck_config {
        let badge_text = if loaded_card_count > 0 {
            format!("Saved: {} ({loaded_card_count} learned)", config.deck_name)
        } else {
            format!("Saved: {}", config.deck_name)
        };
        header = header.push(
            container(text(badge_text).size(12).color(colors.sage))
                .padding([4, 10])
                .style(theme::badge_style(colors.sage_soft, colors.sage)),
        );
    }

    let source_inputs: Element<'a, Message> = match state.source_mode {
        SetupSourceMode::LocalCollection => {
            let collection_row = row![
                text_input(
                    "Path to collection.anki2...",
                    &state.collection_path_input,
                )
                .on_input(Message::CollectionPathChanged)
                .padding([10, 12])
                .style(theme::text_input_style)
                .width(Length::Fill),
                button(text("Browse...").size(13))
                    .padding([10, 16])
                    .style(theme::secondary_button_style)
                    .on_press_maybe((!state.is_busy).then_some(Message::BrowseCollectionFile)),
            ]
            .spacing(10)
            .align_y(Alignment::Center);

            let media_row = row![
                text_input(
                    "Path to collection.media folder...",
                    &state.media_dir_input,
                )
                .on_input(Message::MediaDirChanged)
                .padding([10, 12])
                .style(theme::text_input_style)
                .width(Length::Fill),
                button(text("Browse Folder...").size(13))
                    .padding([10, 16])
                    .style(theme::secondary_button_style)
                    .on_press_maybe((!state.is_busy).then_some(Message::BrowseMediaDir)),
            ]
            .spacing(10)
            .align_y(Alignment::Center);

            let load_btn = button(text("Load Collection (Temp Copy)").size(14))
                .padding([10, 18])
                .style(theme::primary_button_style)
                .on_press_maybe((!state.is_busy).then_some(Message::LoadCollection));

            column![
                text("collection.anki2 (read via safe temporary copy — never locks Anki)")
                    .size(13)
                    .color(colors.text_muted),
                collection_row,
                text("collection.media directory (auto-inferred next to collection.anki2)")
                    .size(13)
                    .color(colors.text_muted),
                media_row,
                load_btn,
            ]
            .spacing(10)
            .into()
        }
        SetupSourceMode::AnkiConnect => {
            let url_row = row![
                text_input("http://127.0.0.1:8765", &state.ankiconnect_url_input)
                    .on_input(Message::AnkiConnectUrlChanged)
                    .padding([10, 12])
                    .style(theme::text_input_style)
                    .width(Length::Fill),
                button(text("Connect to AnkiConnect").size(14))
                    .padding([10, 18])
                    .style(theme::primary_button_style)
                    .on_press_maybe((!state.is_busy).then_some(Message::LoadCollection)),
            ]
            .spacing(10)
            .align_y(Alignment::Center);

            column![
                text("Ensure Anki is running locally with the AnkiConnect add-on enabled.")
                    .size(13)
                    .color(colors.text_muted),
                url_row,
            ]
            .spacing(10)
            .into()
        }
    };

    let mut card_col = column![
        header,
        text("Only graduated cards in the Learned / Review state (queue = 2) are imported, leaving Anki's intervals untouched.")
            .size(14)
            .color(colors.text_muted),
        mode_pills,
        source_inputs,
    ]
    .spacing(12);

    if let Some(status) = &state.busy_label {
        card_col = card_col.push(text(status).size(13).color(colors.coral_text));
    }

    container(card_col)
        .width(Length::Fill)
        .padding(20)
        .style(theme::card_style)
        .into()
}

fn render_field_mapper_card<'a>(
    state: &'a SetupState,
    colors: &'static ThemeTokens,
) -> Element<'a, Message> {
    let deck_options: Vec<DeckPickerOption> = state
        .collection
        .as_ref()
        .map(|summary| {
            summary
                .decks
                .iter()
                .map(|deck| DeckPickerOption {
                    deck_name: deck.name.clone(),
                    learned_card_count: (deck.total_card_count > 0)
                        .then_some(deck.learned_card_count),
                })
                .collect()
        })
        .unwrap_or_default();

    let selected_deck_option = state.selected_deck_name.as_ref().and_then(|name| {
        deck_options
            .iter()
            .find(|option| &option.deck_name == name)
            .cloned()
    });

    let deck_picker_row = if deck_options.is_empty() {
        container(
            text("Load a collection above to choose a deck and map its note fields.")
                .size(13)
                .color(colors.text_muted),
        )
        .width(Length::Fill)
        .padding([10, 14])
        .style(theme::muted_card_style)
        .into()
    } else {
        Element::from(
            row![
                text("Deck")
                    .size(14)
                    .color(colors.text_primary)
                    .width(Length::FillPortion(2)),
                pick_list(deck_options, selected_deck_option, |option| {
                    Message::SelectDeck(option.deck_name)
                })
                .placeholder("Choose an Anki deck...")
                .padding([8, 12])
                .style(theme::pick_list_style)
                .menu_style(theme::pick_list_menu_style)
                .width(Length::FillPortion(3)),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        )
    };

    let required_fields = state.available_fields.clone();
    let optional_fields = state.optional_field_options();

    let selected_word = (!state.draft_mapping.word_field.is_empty())
        .then_some(state.draft_mapping.word_field.clone());
    let selected_kana = (!state.draft_mapping.kana_field.is_empty())
        .then_some(state.draft_mapping.kana_field.clone());
    let selected_sentence = Some(
        state
            .draft_mapping
            .sentence_field
            .clone()
            .unwrap_or_else(|| NONE_FIELD_OPTION.to_string()),
    );
    let selected_word_audio = (!state.draft_mapping.word_audio_field.is_empty())
        .then_some(state.draft_mapping.word_audio_field.clone());
    let selected_sentence_audio = Some(
        state
            .draft_mapping
            .sentence_audio_field
            .clone()
            .unwrap_or_else(|| NONE_FIELD_OPTION.to_string()),
    );

    let can_validate_and_save = !state.is_busy
        && state.collection.is_some()
        && state.selected_deck_name.is_some()
        && state.draft_mapping.validate().is_ok();

    let save_button = button(text("Validate Audio & Save Deck").size(14))
        .padding([10, 18])
        .style(theme::primary_button_style)
        .on_press_maybe(can_validate_and_save.then_some(Message::ValidateAndSaveDeck));

    container(
        column![
            text("2. Deck Picker & Note Field Mapping")
                .size(18)
                .color(colors.text_primary),
            text("Map your note fields once per deck. Changes update the sample preview below immediately.")
                .size(14)
                .color(colors.text_muted),
            deck_picker_row,
            field_picker_row(
                "Target Word / Expression",
                required_fields.clone(),
                selected_word,
                "Select word field...",
                Message::MapWordField,
                colors,
            ),
            field_picker_row(
                "Kana Reading",
                required_fields.clone(),
                selected_kana,
                "Select kana field...",
                Message::MapKanaField,
                colors,
            ),
            field_picker_row(
                "Example Sentence (Optional)",
                optional_fields.clone(),
                selected_sentence,
                NONE_FIELD_OPTION,
                Message::MapSentenceField,
                colors,
            ),
            field_picker_row(
                "Word Audio [sound:...]",
                required_fields,
                selected_word_audio,
                "Select word audio field...",
                Message::MapWordAudioField,
                colors,
            ),
            field_picker_row(
                "Sentence Audio [sound:...] (Optional)",
                optional_fields,
                selected_sentence_audio,
                NONE_FIELD_OPTION,
                Message::MapSentenceAudioField,
                colors,
            ),
            save_button,
        ]
        .spacing(12),
    )
    .width(Length::Fill)
    .padding(20)
    .style(theme::card_style)
    .into()
}

fn field_picker_row<'a, F>(
    label: &'a str,
    options: Vec<String>,
    selected: Option<String>,
    placeholder: &'a str,
    on_select: F,
    colors: &'static ThemeTokens,
) -> Element<'a, Message>
where
    F: Fn(String) -> Message + 'a,
{
    container(
        row![
            text(label)
                .size(14)
                .color(colors.text_primary)
                .width(Length::FillPortion(2)),
            pick_list(options, selected, on_select)
                .placeholder(placeholder)
                .padding([8, 12])
                .style(theme::pick_list_style)
                .menu_style(theme::pick_list_menu_style)
                .width(Length::FillPortion(3)),
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding([8, 12])
    .style(theme::muted_card_style)
    .into()
}

fn render_preview_card<'a>(
    state: &'a SetupState,
    colors: &'static ThemeTokens,
) -> Element<'a, Message> {
    let Some(card) = state.current_preview_card() else {
        return container(
            column![
                text("3. Live Sample Card Preview")
                    .size(18)
                    .color(colors.text_primary),
                text("Select a deck with learned (queue = 2) cards to inspect how your mapped fields and reference audio look before practice.")
                    .size(14)
                    .color(colors.text_muted),
            ]
            .spacing(8),
        )
        .width(Length::Fill)
        .padding(20)
        .style(theme::card_style)
        .into();
    };

    let sample_total = state.sample_notes.len();
    let sample_pos = state.sample_index + 1;

    let nav_row = row![
        text("3. Live Sample Card Preview")
            .size(18)
            .color(colors.text_primary),
        space().width(Length::Fill),
        text(format!("Sample {sample_pos} of {sample_total}"))
            .size(13)
            .color(colors.text_muted),
        button(text("Previous").size(13))
            .padding([6, 12])
            .style(theme::secondary_button_style)
            .on_press_maybe((state.sample_index > 0).then_some(Message::PrevPreviewCard)),
        button(text("Next").size(13))
            .padding([6, 12])
            .style(theme::secondary_button_style)
            .on_press_maybe(
                (state.sample_index + 1 < sample_total).then_some(Message::NextPreviewCard)
            ),
    ]
    .spacing(10)
    .align_y(Alignment::Center);

    let word_display = if card.word.is_empty() {
        "—".to_string()
    } else {
        card.word.clone()
    };
    let kana_display = if card.kana.is_empty() {
        "—".to_string()
    } else {
        card.kana.clone()
    };

    let mut preview_body = column![
        row![
            preview_kv("Target Word", word_display, 20, colors),
            preview_kv("Kana Reading", kana_display, 18, colors),
        ]
        .spacing(12),
    ]
    .spacing(12);

    if let Some(sentence) = &card.sentence {
        preview_body = preview_body.push(preview_kv(
            "Example Sentence",
            sentence.clone(),
            15,
            colors,
        ));
    }

    let word_audio_label = if card.word_audio.is_empty() {
        "No [sound:...] tag found in mapped field".to_string()
    } else {
        card.word_audio.clone()
    };
    let can_play_word = !state.is_playing_audio && !card.word_audio.is_empty();

    let mut audio_controls = row![
        container(
            row![
                column![
                    text("Word Audio").size(12).color(colors.text_muted),
                    text(word_audio_label).size(13).color(colors.text_primary),
                ]
                .spacing(2)
                .width(Length::Fill),
                button(text(if state.is_playing_audio {
                    "Playing..."
                } else {
                    "Play Test Audio"
                })
                .size(13))
                .padding([8, 14])
                .style(theme::primary_button_style)
                .on_press_maybe(
                    can_play_word.then_some(Message::PlayPreviewAudio { sentence: false })
                ),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .padding(12)
        .style(theme::muted_card_style),
    ]
    .spacing(12);

    if let Some(sentence_audio) = &card.sentence_audio {
        let can_play_sentence = !state.is_playing_audio && !sentence_audio.is_empty();
        audio_controls = audio_controls.push(
            container(
                row![
                    column![
                        text("Sentence Audio").size(12).color(colors.text_muted),
                        text(sentence_audio.clone())
                            .size(13)
                            .color(colors.text_primary),
                    ]
                    .spacing(2)
                    .width(Length::Fill),
                    button(text("Play Sentence").size(13))
                        .padding([8, 14])
                        .style(theme::secondary_button_style)
                        .on_press_maybe(
                            can_play_sentence
                                .then_some(Message::PlayPreviewAudio { sentence: true })
                        ),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            )
            .width(Length::Fill)
            .padding(12)
            .style(theme::muted_card_style),
        );
    }

    preview_body = preview_body.push(audio_controls);

    if card.has_audio_warnings() {
        let warnings = card
            .audio_issues
            .iter()
            .fold(column![].spacing(4), |col, issue| {
                col.push(
                    text(format!("• {}", issue.summary()))
                        .size(13)
                        .color(colors.amber),
                )
            });

        preview_body = preview_body.push(
            container(
                column![
                    text("Pre-Flight Audio Warnings").size(13).color(colors.amber),
                    warnings,
                ]
                .spacing(6),
            )
            .width(Length::Fill)
            .padding(12)
            .style(theme::gigo_banner_style),
        );
    } else if !card.word_audio.is_empty() && state.effective_media_dir().is_some() {
        preview_body = preview_body.push(
            container(
                text("Sample clip audio checks passed")
                    .size(12)
                    .color(colors.sage),
            )
            .padding([4, 10])
            .style(theme::badge_style(colors.sage_soft, colors.sage)),
        );
    }

    container(column![nav_row, preview_body].spacing(14))
        .width(Length::Fill)
        .padding(20)
        .style(theme::card_style)
        .into()
}

fn preview_kv<'a>(
    label: &'a str,
    value: String,
    value_size: u32,
    colors: &'static ThemeTokens,
) -> Element<'a, Message> {
    container(
        column![
            text(label).size(12).color(colors.text_muted),
            text(value).size(value_size).color(colors.text_primary),
        ]
        .spacing(4),
    )
    .width(Length::Fill)
    .padding(12)
    .style(theme::muted_card_style)
    .into()
}

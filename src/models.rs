// SPDX-License-Identifier: MPL-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use anyhow::{Result, bail};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

/// Top-level navigation tabs in the Kotodama desktop interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NavTab {
    /// Deck selection, field mapping, and audio quality pre-flight view.
    Setup,
    /// Active pronunciation practice modes (Mora, Pitch, and Shadowing).
    Practice,
    /// Daily streaks, accuracy trends, per-kana mistakes, and data exports.
    Dashboard,
    /// Audio, VAD, playback speed, daily goal, and FSRS threshold configuration.
    Settings,
}

impl NavTab {
    /// Ordered list of all primary navigation tabs.
    pub const ALL: [Self; 4] = [Self::Setup, Self::Practice, Self::Dashboard, Self::Settings];

    /// Short display label for navigation buttons.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Setup => "Setup",
            Self::Practice => "Practice",
            Self::Dashboard => "Dashboard",
            Self::Settings => "Settings",
        }
    }

    /// Brief contextual subtitle describing the active view.
    pub const fn subtitle(self) -> &'static str {
        match self {
            Self::Setup => "Connect your Anki deck, map note fields, and check reference audio.",
            Self::Practice => "Drill mora clarity, pitch contours, and sentence shadowing.",
            Self::Dashboard => "Track streaks, accuracy trends, and per-kana confusion pairs.",
            Self::Settings => "Tune VAD silence timing, playback speed, daily goals, and FSRS.",
        }
    }
}

/// Source location for reading graduated Anki cards and media files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeckSource {
    /// Local Anki SQLite collection copied to a temporary file before reading.
    LocalCollection {
        /// Path to the user's `collection.anki2` database file.
        collection_path: PathBuf,
        /// Path to the `collection.media` directory containing audio clips.
        media_dir: PathBuf,
    },
    /// Live AnkiConnect HTTP endpoint (typically `http://127.0.0.1:8765`).
    AnkiConnect {
        /// Base URL of the AnkiConnect server.
        endpoint_url: String,
    },
}

/// Maps Anki note field names to Kotodama's expected card fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldMapping {
    /// Note field containing the target word or expression.
    pub word_field: String,
    /// Note field containing the kana reading.
    pub kana_field: String,
    /// Optional note field containing an example sentence.
    pub sentence_field: Option<String>,
    /// Note field containing the `[sound:...]` tag for word audio.
    pub word_audio_field: String,
    /// Optional note field containing the `[sound:...]` tag for sentence audio.
    pub sentence_audio_field: Option<String>,
}

impl FieldMapping {
    /// Validates that required note field mappings are non-empty.
    pub fn validate(&self) -> Result<()> {
        if self.word_field.trim().is_empty() {
            bail!("Word / Expression field mapping cannot be empty");
        }
        if self.kana_field.trim().is_empty() {
            bail!("Kana Reading field mapping cannot be empty");
        }
        if self.word_audio_field.trim().is_empty() {
            bail!("Word Audio field mapping cannot be empty");
        }
        Ok(())
    }
}

/// Persisted configuration for a connected Anki deck.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeckConfig {
    /// Selected Anki deck name.
    pub deck_name: String,
    /// Underlying Anki data source.
    pub source: DeckSource,
    /// Field mapping used to extract words, kana, sentences, and audio.
    pub field_mapping: FieldMapping,
    /// Timestamp of the most recent successful sync.
    pub last_synced_at: Option<DateTime<Utc>>,
    /// Number of cards flagged by audio quality pre-flight validation.
    pub flagged_audio_count: usize,
}

impl DeckConfig {
    /// Validates deck selection, source configuration, and field mappings.
    pub fn validate(&self) -> Result<()> {
        if self.deck_name.trim().is_empty() {
            bail!("Deck name cannot be empty");
        }
        match &self.source {
            DeckSource::LocalCollection {
                collection_path,
                media_dir,
            } => {
                if collection_path.as_os_str().is_empty() {
                    bail!("Collection database path cannot be empty");
                }
                if media_dir.as_os_str().is_empty() {
                    bail!("Media directory path cannot be empty");
                }
            }
            DeckSource::AnkiConnect { endpoint_url } => {
                let trimmed = endpoint_url.trim();
                if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
                    bail!("AnkiConnect endpoint URL must start with http:// or https://");
                }
            }
        }
        self.field_mapping.validate()
    }
}

/// Summary metadata for a single deck discovered in an Anki collection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnkiDeckSummary {
    /// Numeric Anki deck identifier.
    pub deck_id: i64,
    /// Display name of the deck using `::` as the hierarchy separator.
    pub name: String,
    /// Number of unique graduated (`queue = 2`) notes in this deck and its subdecks.
    pub learned_card_count: usize,
    /// Total number of cards in this deck and its subdecks.
    pub total_card_count: usize,
}

/// Note type definition and ordered field list from an Anki collection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnkiNoteType {
    /// Numeric Anki note type (model) identifier.
    pub id: i64,
    /// Human-readable name of the note type.
    pub name: String,
    /// Ordered list of field names belonging to this note type.
    pub field_names: Vec<String>,
}

/// Raw graduated Anki note fields prior to applying a user [`FieldMapping`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnkiRawNote {
    /// Representative Anki card ID (`cards.id`).
    pub card_id: i64,
    /// Anki note ID (`notes.id`).
    pub note_id: i64,
    /// Name of the deck containing this card.
    pub deck_name: String,
    /// Associated note type (model) ID.
    pub note_type_id: i64,
    /// Anki card queue state (`2` indicates a graduated review card).
    pub anki_queue: i32,
    /// Raw field values keyed by note field name.
    pub fields: BTreeMap<String, String>,
}

/// Discovered decks and note types loaded from an Anki collection source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnkiCollectionSummary {
    /// Source connection details used to inspect the collection.
    pub source: DeckSource,
    /// Discovered decks in the collection.
    pub decks: Vec<AnkiDeckSummary>,
    /// Discovered note types and their field names.
    pub note_types: Vec<AnkiNoteType>,
}

/// Aggregated audio pre-flight quality report for a deck's reference clips.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeckAudioReport {
    /// Total graduated cards inspected.
    pub total_cards: usize,
    /// Total audio clip references checked across all cards.
    pub total_clips_checked: usize,
    /// Number of cards with at least one flagged audio quality issue.
    pub flagged_cards: usize,
    /// Number of missing or unreadable clip references.
    pub missing_files: usize,
    /// Number of clips shorter than the minimum 200 ms threshold.
    pub too_short_clips: usize,
    /// Number of clips reaching digital clipping levels.
    pub clipped_clips: usize,
    /// Number of clips that failed audio decoding.
    pub decode_failures: usize,
}

impl DeckAudioReport {
    /// Returns `true` when any audio quality warning or error was detected.
    pub const fn has_issues(&self) -> bool {
        self.flagged_cards > 0
    }

    /// Formats a concise one-line summary of the deck audio pre-flight check.
    pub fn summary_line(&self) -> String {
        if self.total_cards == 0 {
            return "No learned cards checked yet.".to_string();
        }
        if !self.has_issues() {
            return format!(
                "All {} clips across {} learned cards passed audio checks.",
                self.total_clips_checked, self.total_cards
            );
        }

        format!(
            "{} of {} cards flagged ({} missing, {} < 200ms, {} clipped, {} decode errors).",
            self.flagged_cards,
            self.total_cards,
            self.missing_files,
            self.too_short_clips,
            self.clipped_clips,
            self.decode_failures
        )
    }
}

/// Persisted deck workspace containing the active deck, field mappings, and card exclusions.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeckWorkspaceConfig {
    /// Most recently configured active Anki deck.
    pub active_deck: Option<DeckConfig>,
    /// Per-deck saved field mappings keyed by deck name.
    pub saved_mappings: BTreeMap<String, FieldMapping>,
    /// Per-deck sets of Anki card IDs permanently excluded from Kotodama practice.
    #[serde(default)]
    pub excluded_card_ids: BTreeMap<String, BTreeSet<i64>>,
    /// Cached display labels for excluded Anki cards keyed by card ID.
    #[serde(default)]
    pub excluded_card_labels: BTreeMap<i64, String>,
}

impl DeckWorkspaceConfig {
    /// Returns `true` when `card_id` is excluded from practice in `deck_name`.
    pub fn is_card_excluded(&self, deck_name: &str, card_id: i64) -> bool {
        self.excluded_card_ids
            .get(deck_name)
            .is_some_and(|ids| ids.contains(&card_id))
    }

    /// Returns the count of excluded cards recorded for `deck_name`.
    pub fn excluded_count_for_deck(&self, deck_name: &str) -> usize {
        self.excluded_card_ids
            .get(deck_name)
            .map_or(0, BTreeSet::len)
    }

    /// Excludes `card_id` from `deck_name` with an optional display label, returning `true` if newly added.
    pub fn exclude_card(&mut self, deck_name: &str, card_id: i64, label: Option<String>) -> bool {
        if let Some(text) = label
            && !text.trim().is_empty()
        {
            self.excluded_card_labels.insert(card_id, text);
        }
        self.excluded_card_ids
            .entry(deck_name.to_string())
            .or_default()
            .insert(card_id)
    }

    /// Restores a previously excluded `card_id` back into `deck_name`, returning `true` if it was excluded.
    pub fn restore_card(&mut self, deck_name: &str, card_id: i64) -> bool {
        let Some(ids) = self.excluded_card_ids.get_mut(deck_name) else {
            return false;
        };
        let removed = ids.remove(&card_id);
        if ids.is_empty() {
            self.excluded_card_ids.remove(deck_name);
        }
        if removed && !self.is_card_excluded_anywhere(card_id) {
            self.excluded_card_labels.remove(&card_id);
        }
        removed
    }

    /// Restores all excluded cards for `deck_name`, returning how many cards were restored.
    pub fn restore_all_for_deck(&mut self, deck_name: &str) -> usize {
        let Some(removed_ids) = self.excluded_card_ids.remove(deck_name) else {
            return 0;
        };
        let count = removed_ids.len();
        for card_id in removed_ids {
            if !self.is_card_excluded_anywhere(card_id) {
                self.excluded_card_labels.remove(&card_id);
            }
        }
        count
    }

    /// Excludes every card in `cards` that carries audio quality warnings, returning how many were newly excluded.
    pub fn exclude_invalid_audio_cards(&mut self, deck_name: &str, cards: &[AnkiCard]) -> usize {
        let mut added = 0_usize;
        for card in cards {
            if card.has_audio_warnings()
                && self.exclude_card(deck_name, card.card_id, Some(card.display_label()))
            {
                added = added.saturating_add(1);
            }
        }
        added
    }

    /// Filters `cards` to exclude any card IDs listed as excluded for `deck_name`.
    pub fn filter_active_cards<'a>(&self, deck_name: &str, cards: &'a [AnkiCard]) -> Vec<&'a AnkiCard> {
        let Some(excluded) = self.excluded_card_ids.get(deck_name) else {
            return cards.iter().collect();
        };
        cards
            .iter()
            .filter(|card| !excluded.contains(&card.card_id))
            .collect()
    }

    /// Returns sorted `(card_id, display_label)` entries for all excluded cards in `deck_name`.
    pub fn excluded_entries_for_deck(&self, deck_name: &str) -> Vec<(i64, String)> {
        let Some(excluded) = self.excluded_card_ids.get(deck_name) else {
            return Vec::new();
        };
        excluded
            .iter()
            .map(|&card_id| {
                let label = self
                    .excluded_card_labels
                    .get(&card_id)
                    .cloned()
                    .unwrap_or_else(|| format!("Card #{card_id}"));
                (card_id, label)
            })
            .collect()
    }

    fn is_card_excluded_anywhere(&self, card_id: i64) -> bool {
        self.excluded_card_ids
            .values()
            .any(|ids| ids.contains(&card_id))
    }
}

/// Decoded interleaved PCM audio buffer normalized to `[-1.0, 1.0]`.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedAudio {
    /// Interleaved normalized `f32` audio samples.
    pub samples: Vec<f32>,
    /// Sampling rate in Hertz.
    pub sample_rate: u32,
    /// Number of audio channels.
    pub channels: u16,
}

impl DecodedAudio {
    /// Returns the number of multi-channel sample frames in the buffer.
    pub fn frame_count(&self) -> usize {
        if self.channels == 0 {
            return 0;
        }
        self.samples.len() / usize::from(self.channels)
    }

    /// Returns the audio duration in milliseconds.
    pub fn duration_ms(&self) -> u64 {
        if self.sample_rate == 0 || self.channels == 0 {
            return 0;
        }
        let frames = self.frame_count() as u64;
        frames.saturating_mul(1000) / u64::from(self.sample_rate)
    }

    /// Mixes interleaved multi-channel samples down to a single mono `f32` buffer.
    pub fn to_mono(&self) -> Vec<f32> {
        if self.channels == 0 || self.samples.is_empty() {
            return Vec::new();
        }
        if self.channels == 1 {
            return self.samples.clone();
        }
        let ch = usize::from(self.channels);
        let scale = 1.0 / ch as f32;
        self.samples
            .chunks_exact(ch)
            .map(|frame| frame.iter().copied().sum::<f32>() * scale)
            .collect()
    }
}

/// Audio quality problems detected during pre-flight deck validation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AudioQualityIssue {
    /// Referenced media file could not be found in `collection.media`.
    MissingFile,
    /// Audio clip is shorter than the minimum usable threshold (200 ms).
    TooShort {
        /// Measured duration in milliseconds.
        duration_ms: u64,
    },
    /// Audio waveform reaches clipping levels near full scale.
    ClippedAudio {
        /// Normalized peak amplitude ratio (0.0 to 1.0+).
        peak_ratio: f32,
    },
    /// Audio file could not be decoded into PCM samples.
    DecodeFailed {
        /// Human-readable reason describing the decode failure.
        reason: String,
    },
}

impl AudioQualityIssue {
    /// Returns a concise, user-facing description of the audio quality issue.
    pub fn summary(&self) -> String {
        match self {
            Self::MissingFile => "Missing audio file".to_string(),
            Self::TooShort { duration_ms } => {
                format!("Too short ({duration_ms} ms < 200 ms)")
            }
            Self::ClippedAudio { peak_ratio } => {
                format!("Clipped audio (peak {peak_ratio:.2})")
            }
            Self::DecodeFailed { reason } => format!("Decode failed: {reason}"),
        }
    }
}

/// Graduated Anki card imported into Kotodama for pronunciation practice.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnkiCard {
    /// Anki card ID (`cards.id`).
    pub card_id: i64,
    /// Anki note ID (`notes.id`).
    pub note_id: i64,
    /// Owning deck name.
    pub deck_name: String,
    /// Target Japanese word or expression.
    pub word: String,
    /// Target kana reading.
    pub kana: String,
    /// Optional example sentence text.
    pub sentence: Option<String>,
    /// Extracted audio filename for the word.
    pub word_audio: String,
    /// Optional extracted audio filename for the sentence.
    pub sentence_audio: Option<String>,
    /// Anki card queue state (`2` indicates a graduated review card).
    pub anki_queue: i32,
    /// Any audio quality issues discovered for this card's reference clips.
    pub audio_issues: Vec<AudioQualityIssue>,
    /// Most recent pitch accent error percentage recorded for this card.
    pub last_pitch_error_percent: Option<f32>,
}

impl AnkiCard {
    /// Anki queue value representing graduated cards in the review state.
    pub const LEARNED_QUEUE: i32 = 2;

    /// Returns `true` when the card is in Anki's graduated/learned review queue.
    #[allow(dead_code)]
    pub const fn is_learned(&self) -> bool {
        self.anki_queue == Self::LEARNED_QUEUE
    }

    /// Returns `true` when one or more reference audio quality issues were flagged.
    pub fn has_audio_warnings(&self) -> bool {
        !self.audio_issues.is_empty()
    }

    /// Formats a concise human-readable label for preview badges and exclusion lists.
    pub fn display_label(&self) -> String {
        let word = self.word.trim();
        let kana = self.kana.trim();
        match (word.is_empty(), kana.is_empty()) {
            (false, false) if word != kana => format!("{word} ({kana})"),
            (false, _) => word.to_string(),
            (true, false) => kana.to_string(),
            (true, true) => format!("Card #{}", self.card_id),
        }
    }
}

/// Available pronunciation drill modes in Kotodama.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PracticeMode {
    /// Focuses on isolated kana/mora articulation clarity.
    MoraPurity,
    /// Compares normalized pitch contour against reference audio via DTW.
    PitchAccent,
    /// Combines countdown shadowing with simultaneous mora and pitch scoring.
    CombinedShadowing,
}

impl PracticeMode {
    /// Ordered list of all practice modes.
    pub const ALL: [Self; 3] = [
        Self::MoraPurity,
        Self::PitchAccent,
        Self::CombinedShadowing,
    ];

    /// Display name for the practice mode.
    pub const fn label(self) -> &'static str {
        match self {
            Self::MoraPurity => "Mora Purity",
            Self::PitchAccent => "Pitch Accent",
            Self::CombinedShadowing => "Combined Shadowing",
        }
    }

    /// Short description displayed in the practice mode selector card.
    pub const fn short_description(self) -> &'static str {
        match self {
            Self::MoraPurity => "Isolate individual kana clarity and spot subtle consonant/vowel swaps.",
            Self::PitchAccent => "Match normalized F0 pitch curves against native audio with DTW.",
            Self::CombinedShadowing => "Shadow full sentences after a countdown with combined mora + pitch grading.",
        }
    }

    /// Returns the canonical database string representation for this practice mode.
    #[allow(dead_code)]
    pub const fn as_db_str(self) -> &'static str {
        match self {
            Self::MoraPurity => "MoraPurity",
            Self::PitchAccent => "PitchAccent",
            Self::CombinedShadowing => "CombinedShadowing",
        }
    }

    /// Parses a [`PracticeMode`] from its canonical database string representation.
    #[allow(dead_code)]
    pub fn from_db_str(raw: &str) -> Result<Self> {
        match raw.trim() {
            "MoraPurity" => Ok(Self::MoraPurity),
            "PitchAccent" => Ok(Self::PitchAccent),
            "CombinedShadowing" => Ok(Self::CombinedShadowing),
            other => bail!("Unknown practice mode '{other}'"),
        }
    }
}

/// Result of comparing user pitch contour against reference audio.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PitchAnalysis {
    /// Reference F0 values normalized to semitones relative to reference median (`None` = unvoiced).
    pub reference_semitones: Vec<Option<f32>>,
    /// User F0 values normalized to semitones relative to user median (`None` = unvoiced).
    pub user_semitones: Vec<Option<f32>>,
    /// Dynamic Time Warping alignment distance between voiced contours.
    pub dtw_distance: f32,
    /// Overall pitch accent error percentage in the range `0.0..=100.0`.
    pub pitch_error_percent: f32,
    /// Reference speaker median F0 in Hertz.
    pub reference_median_hz: f32,
    /// User speaker median F0 in Hertz.
    pub user_median_hz: f32,
    /// Timestamp when the pitch analysis was computed.
    pub analyzed_at: DateTime<Utc>,
}

impl PitchAnalysis {
    /// Creates a validated [`PitchAnalysis`] record.
    pub fn new(
        reference_semitones: Vec<Option<f32>>,
        user_semitones: Vec<Option<f32>>,
        dtw_distance: f32,
        pitch_error_percent: f32,
        reference_median_hz: f32,
        user_median_hz: f32,
        analyzed_at: DateTime<Utc>,
    ) -> Result<Self> {
        if !dtw_distance.is_finite() || dtw_distance < 0.0 {
            bail!("DTW distance must be a finite non-negative number");
        }
        if !pitch_error_percent.is_finite() || !(0.0..=100.0).contains(&pitch_error_percent) {
            bail!("Pitch error percent must be within 0.0..=100.0");
        }
        if !reference_median_hz.is_finite() || reference_median_hz <= 0.0 {
            bail!("Reference median Hz must be positive and finite");
        }
        if !user_median_hz.is_finite() || user_median_hz <= 0.0 {
            bail!("User median Hz must be positive and finite");
        }

        Ok(Self {
            reference_semitones,
            user_semitones,
            dtw_distance,
            pitch_error_percent,
            reference_median_hz,
            user_median_hz,
            analyzed_at,
        })
    }

    /// Returns pitch accuracy percentage (`100.0 - pitch_error_percent`).
    #[allow(dead_code)]
    pub fn accuracy_percent(&self) -> f32 {
        (100.0 - self.pitch_error_percent).clamp(0.0, 100.0)
    }
}

/// Single mora pronunciation attempt evaluated against a target kana.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MoraAttempt {
    /// Identifier of the practiced Anki card.
    pub card_id: i64,
    /// Expected target kana/mora.
    pub expected_kana: String,
    /// Recognized kana/mora, or `None` when recognition confidence was inconclusive.
    pub heard_kana: Option<String>,
    /// Scorer confidence in `0.0..=1.0`.
    pub confidence: f32,
    /// Timestamp when the attempt was recorded.
    pub recorded_at: DateTime<Utc>,
}

#[allow(dead_code)]
impl MoraAttempt {
    /// Returns `true` when the recognized mora matches `expected_kana`.
    pub fn is_correct(&self) -> bool {
        self.heard_kana.as_deref() == Some(self.expected_kana.as_str())
    }

    /// Returns the recognized kana label or `"unknown"` when unclassifiable.
    pub fn heard_label(&self) -> &str {
        self.heard_kana.as_deref().unwrap_or("unknown")
    }
}

/// Aggregated confusion count and percentage for a specific mispronounced kana.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KanaMistakeEntry {
    /// Kana label (or `"unknown"`) heard instead of the target kana.
    pub heard_kana: String,
    /// Number of times this substitution occurred.
    pub count: u64,
    /// Share of total attempts for the target kana (`0.0..=100.0`).
    pub percentage: f32,
}

/// Cumulative pronunciation clarity statistics for a single target kana.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KanaStats {
    /// Target kana character or mora.
    pub kana: String,
    /// Total recorded attempts for this kana.
    pub total_attempts: u64,
    /// Number of attempts matching `kana`.
    pub correct_attempts: u64,
    /// Frequency map of confused outputs (`heard_kana` or `"unknown"`) to counts.
    pub mistake_counts: BTreeMap<String, u64>,
}

#[allow(dead_code)]
impl KanaStats {
    /// Initializes empty clarity statistics for a target kana.
    pub fn new(kana: impl Into<String>) -> Self {
        Self {
            kana: kana.into(),
            total_attempts: 0,
            correct_attempts: 0,
            mistake_counts: BTreeMap::new(),
        }
    }

    /// Incorporates a [`MoraAttempt`] into accuracy and confusion counters.
    pub fn record_attempt(&mut self, attempt: &MoraAttempt) {
        self.total_attempts = self.total_attempts.saturating_add(1);
        if attempt.is_correct() {
            self.correct_attempts = self.correct_attempts.saturating_add(1);
        } else {
            let label = attempt.heard_label().to_string();
            let entry = self.mistake_counts.entry(label).or_insert(0);
            *entry = entry.saturating_add(1);
        }
    }

    /// Returns accuracy percentage in `0.0..=100.0` (returns `0.0` when there are no attempts).
    pub fn accuracy_percent(&self) -> f32 {
        if self.total_attempts == 0 {
            return 0.0;
        }
        (self.correct_attempts as f32 / self.total_attempts as f32) * 100.0
    }

    /// Returns up to `limit` most frequent mistakes sorted descending by count.
    pub fn top_mistakes(&self, limit: usize) -> Vec<KanaMistakeEntry> {
        if self.total_attempts == 0 || limit == 0 {
            return Vec::new();
        }
        let mut entries: Vec<KanaMistakeEntry> = self
            .mistake_counts
            .iter()
            .map(|(heard_kana, &count)| KanaMistakeEntry {
                heard_kana: heard_kana.clone(),
                count,
                percentage: (count as f32 / self.total_attempts as f32) * 100.0,
            })
            .collect();

        entries.sort_by(|left, right| {
            right
                .count
                .cmp(&left.count)
                .then_with(|| left.heard_kana.cmp(&right.heard_kana))
        });
        entries.truncate(limit);
        entries
    }

    /// Formats a human-readable clarity and top-5 mistake summary for UI display.
    pub fn format_breakdown(&self) -> String {
        if self.total_attempts == 0 {
            return format!("{}: no attempts yet", self.kana);
        }

        let accuracy = self.accuracy_percent().round() as u64;
        let mistakes = self.top_mistakes(5);
        if mistakes.is_empty() {
            return format!("{}: {}% correct", self.kana, accuracy);
        }

        let formatted_mistakes: Vec<String> = mistakes
            .into_iter()
            .map(|entry| format!("{}% {}", entry.percentage.round() as u64, entry.heard_kana))
            .collect();

        format!(
            "{}: {}% correct — {}",
            self.kana,
            accuracy,
            formatted_mistakes.join(", ")
        )
    }
}

/// FSRS review ratings supported for manual override and automated score mapping.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FsrsRating {
    /// Pronunciation needs immediate re-drilling.
    Again,
    /// Pronunciation passed with notable mora or pitch errors.
    Hard,
    /// Pronunciation was clear and natural.
    Good,
    /// Pronunciation closely matched native clarity and pitch.
    Easy,
}

#[allow(dead_code)]
impl FsrsRating {
    /// All FSRS ratings in shortcut order (`1` through `4`).
    pub const ALL: [Self; 4] = [Self::Again, Self::Hard, Self::Good, Self::Easy];

    /// Keyboard shortcut key assigned to this rating.
    pub const fn shortcut_key(self) -> char {
        match self {
            Self::Again => '1',
            Self::Hard => '2',
            Self::Good => '3',
            Self::Easy => '4',
        }
    }

    /// Human-readable label for buttons and history logs.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Again => "Again",
            Self::Hard => "Hard",
            Self::Good => "Good",
            Self::Easy => "Easy",
        }
    }

    /// Returns the canonical database string representation for this FSRS rating.
    pub const fn as_db_str(self) -> &'static str {
        match self {
            Self::Again => "Again",
            Self::Hard => "Hard",
            Self::Good => "Good",
            Self::Easy => "Easy",
        }
    }

    /// Parses an [`FsrsRating`] from its canonical database string representation.
    pub fn from_db_str(raw: &str) -> Result<Self> {
        match raw.trim() {
            "Again" => Ok(Self::Again),
            "Hard" => Ok(Self::Hard),
            "Good" => Ok(Self::Good),
            "Easy" => Ok(Self::Easy),
            other => bail!("Unknown FSRS rating '{other}'"),
        }
    }
}

/// Configurable percentage thresholds mapping combined practice scores to FSRS ratings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoreThresholds {
    /// Minimum score percentage required for [`FsrsRating::Hard`].
    pub hard_min: f32,
    /// Minimum score percentage required for [`FsrsRating::Good`].
    pub good_min: f32,
    /// Minimum score percentage required for [`FsrsRating::Easy`].
    pub easy_min: f32,
}

impl Default for ScoreThresholds {
    fn default() -> Self {
        Self::balanced()
    }
}

#[allow(dead_code)]
impl ScoreThresholds {
    /// Returns lenient score thresholds (`40% / 65% / 85%`) suited for early shadowing practice.
    pub const fn lenient() -> Self {
        Self {
            hard_min: 40.0,
            good_min: 65.0,
            easy_min: 85.0,
        }
    }

    /// Returns balanced default score thresholds (`50% / 75% / 90%`).
    pub const fn balanced() -> Self {
        Self {
            hard_min: 50.0,
            good_min: 75.0,
            easy_min: 90.0,
        }
    }

    /// Returns strict score thresholds (`60% / 80% / 92%`) for precision drilling.
    pub const fn strict() -> Self {
        Self {
            hard_min: 60.0,
            good_min: 80.0,
            easy_min: 92.0,
        }
    }

    /// Validates that thresholds are finite, ordered, and within `0.0..=100.0`.
    pub fn validate(&self) -> Result<()> {
        if !self.hard_min.is_finite() || !self.good_min.is_finite() || !self.easy_min.is_finite() {
            bail!("Score thresholds must be finite percentages");
        }
        if self.hard_min < 0.0 || self.easy_min > 100.0 {
            bail!("Score thresholds must stay within 0.0..=100.0");
        }
        if !(self.hard_min < self.good_min && self.good_min < self.easy_min) {
            bail!("Score thresholds must strictly increase: hard_min < good_min < easy_min");
        }
        Ok(())
    }

    /// Maps a numeric score percentage (`0.0..=100.0`) to an [`FsrsRating`].
    pub fn rate_score(&self, score_percent: f32) -> FsrsRating {
        if score_percent >= self.easy_min {
            FsrsRating::Easy
        } else if score_percent >= self.good_min {
            FsrsRating::Good
        } else if score_percent >= self.hard_min {
            FsrsRating::Hard
        } else {
            FsrsRating::Again
        }
    }
}

/// Independent FSRS scheduling state stored locally per Anki card ID.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FsrsCardState {
    /// Associated Anki card ID.
    pub card_id: i64,
    /// FSRS memory stability estimate in days.
    pub stability: f64,
    /// FSRS-7 fast stability trace in days.
    #[serde(default)]
    pub stability_fast: f64,
    /// FSRS item difficulty estimate.
    pub difficulty: f64,
    /// Days elapsed since the previous pronunciation review.
    pub elapsed_days: u32,
    /// Scheduled interval in days for the next review.
    pub scheduled_days: u32,
    /// Total number of pronunciation reviews completed.
    pub reps: u32,
    /// Number of times the card was rated `Again`.
    pub lapses: u32,
    /// Next due timestamp in UTC.
    pub due_at: DateTime<Utc>,
    /// Timestamp of the most recent pronunciation review.
    pub last_reviewed_at: Option<DateTime<Utc>>,
    /// Most recent FSRS rating applied to this card.
    pub last_rating: Option<FsrsRating>,
}

#[allow(dead_code)]
impl FsrsCardState {
    /// Creates an unseen pronunciation scheduling state due immediately.
    pub fn new_unseen(card_id: i64, now: DateTime<Utc>) -> Self {
        Self {
            card_id,
            stability: 0.0,
            stability_fast: 0.0,
            difficulty: 0.0,
            elapsed_days: 0,
            scheduled_days: 0,
            reps: 0,
            lapses: 0,
            due_at: now,
            last_reviewed_at: None,
            last_rating: None,
        }
    }

    /// Returns `true` when the card is due for practice at `now`.
    pub fn is_due(&self, now: DateTime<Utc>) -> bool {
        self.due_at <= now
    }
}

/// Configurable daily practice target measured in cards or minutes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DailyGoal {
    /// Daily target number of reviewed cards.
    Cards(u32),
    /// Daily target active practice duration in minutes.
    Minutes(u32),
}

impl Default for DailyGoal {
    fn default() -> Self {
        Self::Cards(10)
    }
}

impl DailyGoal {
    /// Formats the daily goal for display in UI headers and settings.
    pub fn label(self) -> String {
        match self {
            Self::Cards(count) => format!("{count} cards / day"),
            Self::Minutes(minutes) => format!("{minutes} min / day"),
        }
    }

    /// Returns the canonical kind identifier for database storage (`"Cards"` or `"Minutes"`).
    pub const fn kind_str(self) -> &'static str {
        match self {
            Self::Cards(_) => "Cards",
            Self::Minutes(_) => "Minutes",
        }
    }

    /// Returns the numeric target value for this daily goal.
    pub const fn target_value(self) -> u32 {
        match self {
            Self::Cards(count) => count,
            Self::Minutes(minutes) => minutes,
        }
    }

    /// Reconstructs a [`DailyGoal`] from persisted database columns.
    pub fn from_parts(kind: &str, value: u32) -> Result<Self> {
        match kind.trim() {
            "Cards" => Ok(Self::Cards(value)),
            "Minutes" => Ok(Self::Minutes(value)),
            other => bail!("Unknown daily goal kind '{other}'"),
        }
    }
}

/// Tracks consecutive practice days, daily goal progress, and lifetime sessions.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DailyStreak {
    /// Current consecutive days practiced.
    pub current_streak_days: u32,
    /// All-time longest consecutive day streak.
    pub longest_streak_days: u32,
    /// Total completed practice sessions.
    pub total_sessions: u64,
    /// Cards practiced on `last_practice_date`.
    pub cards_practiced_today: u32,
    /// Minutes practiced on `last_practice_date`.
    pub minutes_practiced_today: u32,
    /// Calendar date of the most recent practice activity.
    pub last_practice_date: Option<NaiveDate>,
    /// Active daily target used for progress tracking.
    pub daily_goal: DailyGoal,
}

impl DailyStreak {
    /// Records a completed practice session for `date` and updates streak counters.
    #[allow(dead_code)]
    pub fn record_practice(
        &mut self,
        date: NaiveDate,
        cards_completed: u32,
        minutes_spent: u32,
    ) {
        self.total_sessions = self.total_sessions.saturating_add(1);

        match self.last_practice_date {
            Some(prev_date) if date == prev_date => {
                self.cards_practiced_today =
                    self.cards_practiced_today.saturating_add(cards_completed);
                self.minutes_practiced_today =
                    self.minutes_practiced_today.saturating_add(minutes_spent);
            }
            Some(prev_date) if date.signed_duration_since(prev_date).num_days() == 1 => {
                self.current_streak_days = self.current_streak_days.saturating_add(1);
                self.cards_practiced_today = cards_completed;
                self.minutes_practiced_today = minutes_spent;
                self.last_practice_date = Some(date);
            }
            Some(prev_date) if date < prev_date => {}
            _ => {
                self.current_streak_days = 1;
                self.cards_practiced_today = cards_completed;
                self.minutes_practiced_today = minutes_spent;
                self.last_practice_date = Some(date);
            }
        }

        if self.current_streak_days > self.longest_streak_days {
            self.longest_streak_days = self.current_streak_days;
        }
    }

    /// Returns normalized daily goal completion ratio clamped to `0.0..=1.0`.
    pub fn goal_progress_ratio(&self) -> f32 {
        let (done, target) = match self.daily_goal {
            DailyGoal::Cards(goal) => (self.cards_practiced_today, goal),
            DailyGoal::Minutes(goal) => (self.minutes_practiced_today, goal),
        };
        if target == 0 {
            return 1.0;
        }
        (done as f32 / target as f32).clamp(0.0, 1.0)
    }

    /// Returns `true` when today's practice meets or exceeds `daily_goal`.
    pub fn is_daily_goal_met(&self) -> bool {
        match self.daily_goal {
            DailyGoal::Cards(goal) => self.cards_practiced_today >= goal,
            DailyGoal::Minutes(goal) => self.minutes_practiced_today >= goal,
        }
    }
}

/// Reference audio playback speed options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PlaybackSpeed {
    /// Pitch-preserving 0.75x slow playback for shadowing fast native sentences.
    Slow,
    /// Original 1.0x reference playback speed.
    Normal,
}

impl PlaybackSpeed {
    /// Returns the numeric playback rate multiplier.
    pub const fn multiplier(self) -> f32 {
        match self {
            Self::Slow => 0.75,
            Self::Normal => 1.0,
        }
    }

    /// Returns a concise display label for UI controls.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Slow => "0.75x (Slow)",
            Self::Normal => "1.0x (Normal)",
        }
    }
}

/// User-selected theme preference controlling Light/Dark appearance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum ThemePreference {
    /// Follow the operating system's current light or dark appearance.
    #[default]
    System,
    /// Force Kotodama's warm cream Light palette.
    Light,
    /// Force Kotodama's warm charcoal/espresso Dark palette.
    Dark,
}

impl ThemePreference {
    /// Ordered list of all selectable theme preferences.
    pub const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];

    /// Short display label for UI controls.
    pub const fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }

    /// Cycles to the next preference in `System -> Light -> Dark -> System` order.
    pub const fn cycle_next(self) -> Self {
        match self {
            Self::System => Self::Light,
            Self::Light => Self::Dark,
            Self::Dark => Self::System,
        }
    }

    /// Resolves this preference into a concrete [`ResolvedTheme`] given the OS dark-mode flag.
    pub const fn resolve(self, system_is_dark: Option<bool>) -> ResolvedTheme {
        match self {
            Self::System => match system_is_dark {
                Some(true) => ResolvedTheme::Dark,
                Some(false) | None => ResolvedTheme::Light,
            },
            Self::Light => ResolvedTheme::Light,
            Self::Dark => ResolvedTheme::Dark,
        }
    }
}

/// Concrete Light or Dark color scheme currently rendered by the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ResolvedTheme {
    /// Warm cream and soft slate light appearance.
    Light,
    /// Warm charcoal and espresso dark appearance.
    Dark,
}

impl ResolvedTheme {
    /// Human-readable name of the resolved color scheme.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }

    /// Returns `true` when the active appearance is Dark mode.
    #[allow(dead_code)]
    pub const fn is_dark(self) -> bool {
        matches!(self, Self::Dark)
    }
}

/// Metadata describing an available system audio input device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputDeviceInfo {
    /// Human-readable device name reported by the audio host.
    pub name: String,
    /// Whether this device is currently the system's default input device.
    pub is_default: bool,
}

/// Configuration parameters controlling Voice Activity Detection and silence trimming.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VadConfig {
    /// Minimum frame RMS amplitude required to classify a frame as speech.
    pub rms_threshold: f32,
    /// Duration of trailing silence in milliseconds before auto-stopping capture.
    pub trailing_silence_ms: u64,
    /// Analysis frame window duration in milliseconds.
    pub frame_ms: u64,
    /// Leading and trailing padding in milliseconds retained around trimmed speech.
    pub padding_ms: u64,
    /// Whether capture automatically stops after trailing silence is detected.
    pub auto_stop: bool,
    /// Maximum recording length in milliseconds before capture automatically stops.
    pub max_duration_ms: u64,
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            rms_threshold: 0.025,
            trailing_silence_ms: 1000,
            frame_ms: 20,
            padding_ms: 60,
            auto_stop: true,
            max_duration_ms: 15_000,
        }
    }
}

impl VadConfig {
    /// Validates that VAD thresholds and timing parameters are finite and positive.
    pub fn validate(&self) -> Result<()> {
        if !self.rms_threshold.is_finite() || self.rms_threshold <= 0.0 || self.rms_threshold > 1.0 {
            bail!("VAD RMS threshold must be finite and within (0.0, 1.0]");
        }
        if self.frame_ms == 0 {
            bail!("VAD frame duration must be at least 1 ms");
        }
        if self.max_duration_ms < self.frame_ms {
            bail!("Maximum recording duration must be at least one VAD frame");
        }
        Ok(())
    }

    /// Builds a [`VadConfig`] from persisted [`AppSettings`].
    pub fn from_settings(settings: &AppSettings) -> Self {
        Self {
            rms_threshold: settings.vad_rms_threshold.clamp(0.005, 0.25),
            trailing_silence_ms: settings.vad_silence_ms.max(200),
            auto_stop: settings.auto_stop_vad,
            ..Self::default()
        }
    }
}

/// Instantaneous microphone signal telemetry used for live level meters and calibration.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MicLevelSnapshot {
    /// Root-mean-square amplitude over the recent capture window.
    pub rms: f32,
    /// Peak absolute sample amplitude over the recent capture window.
    pub peak: f32,
    /// Normalized meter level mapped to `0.0..=1.0` from `-60..=0 dBFS`.
    pub meter_ratio: f32,
    /// Whether the signal reached near-full-scale clipping (`peak >= 0.98`).
    pub is_clipping: bool,
    /// Whether the RMS level exceeds the active VAD speech threshold.
    pub is_speech: bool,
    /// Signal level in decibels relative to full scale (`-60.0..=0.0`).
    pub db_fs: f32,
}

impl Default for MicLevelSnapshot {
    fn default() -> Self {
        Self {
            rms: 0.0,
            peak: 0.0,
            meter_ratio: 0.0,
            is_clipping: false,
            is_speech: false,
            db_fs: -60.0,
        }
    }
}

impl MicLevelSnapshot {
    /// Computes a [`MicLevelSnapshot`] from a slice of mono `f32` audio samples.
    pub fn from_samples(samples: &[f32], vad_threshold: f32) -> Self {
        if samples.is_empty() {
            return Self::default();
        }
        let mut sum_sq = 0.0_f64;
        let mut peak = 0.0_f32;
        for &sample in samples {
            if sample.is_finite() {
                let abs = sample.abs();
                if abs > peak {
                    peak = abs;
                }
                let s = f64::from(sample);
                sum_sq += s * s;
            }
        }
        let rms = (sum_sq / samples.len() as f64).sqrt() as f32;
        let db_fs = (20.0 * rms.max(1e-6).log10()).clamp(-60.0, 0.0);
        let meter_ratio = ((db_fs + 60.0) / 60.0).clamp(0.0, 1.0);

        Self {
            rms,
            peak,
            meter_ratio,
            is_clipping: peak >= 0.98,
            is_speech: rms >= vad_threshold,
            db_fs,
        }
    }
}

/// Configuration parameters for YIN fundamental frequency (`F0`) pitch extraction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PitchExtractionConfig {
    /// Minimum expected fundamental frequency in Hertz.
    pub min_f0_hz: f32,
    /// Maximum expected fundamental frequency in Hertz.
    pub max_f0_hz: f32,
    /// Analysis hop step size in milliseconds.
    pub hop_ms: f32,
    /// YIN cumulative mean normalized difference threshold (`0.0..1.0`).
    pub yin_threshold: f32,
    /// Minimum frame RMS amplitude required to emit a voiced pitch estimate.
    pub min_voiced_rms: f32,
}

impl Default for PitchExtractionConfig {
    fn default() -> Self {
        Self {
            min_f0_hz: 65.0,
            max_f0_hz: 500.0,
            hop_ms: 10.0,
            yin_threshold: 0.15,
            min_voiced_rms: 0.01,
        }
    }
}

impl PitchExtractionConfig {
    /// Validates that pitch extraction bounds and thresholds are finite and well-ordered.
    pub fn validate(&self) -> Result<()> {
        if !self.min_f0_hz.is_finite() || self.min_f0_hz <= 0.0 {
            bail!("Minimum F0 must be finite and greater than 0 Hz");
        }
        if !self.max_f0_hz.is_finite() || self.max_f0_hz <= self.min_f0_hz {
            bail!("Maximum F0 must be finite and greater than minimum F0");
        }
        if !self.hop_ms.is_finite() || self.hop_ms <= 0.0 {
            bail!("Pitch hop duration must be finite and greater than 0 ms");
        }
        if !self.yin_threshold.is_finite() || self.yin_threshold <= 0.0 || self.yin_threshold >= 1.0
        {
            bail!("YIN threshold must be finite and strictly within (0.0, 1.0)");
        }
        if !self.min_voiced_rms.is_finite() || self.min_voiced_rms < 0.0 {
            bail!("Minimum voiced RMS must be finite and non-negative");
        }
        Ok(())
    }
}

/// Reason why an active microphone recording session ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CaptureStopReason {
    /// Recording was stopped explicitly by the user.
    Manual,
    /// Recording stopped automatically after trailing VAD silence.
    AutoStopSilence,
    /// Recording stopped because the maximum clip duration cap was reached.
    MaxDurationReached,
}

impl CaptureStopReason {
    /// Returns a concise human-readable label describing why capture stopped.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Manual => "Manual stop",
            Self::AutoStopSilence => "Auto-stopped (VAD silence)",
            Self::MaxDurationReached => "Max duration reached",
        }
    }
}

/// Recorded microphone audio paired with VAD-trimmed speech and signal metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordedTake {
    /// Unmodified mono audio captured from the microphone.
    pub raw_audio: DecodedAudio,
    /// Audio trimmed around detected speech boundaries via VAD.
    pub trimmed_audio: DecodedAudio,
    /// Peak absolute sample level observed across the raw recording.
    pub peak_level: f32,
    /// Whether speech energy exceeding the VAD threshold was detected.
    pub speech_detected: bool,
    /// Reason recording terminated.
    pub stop_reason: CaptureStopReason,
}

fn default_vad_rms_threshold() -> f32 {
    0.025
}

/// User-configurable ergonomics, audio, and scheduling settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppSettings {
    /// Color-scheme preference (`System`, `Light`, or `Dark`).
    #[serde(default)]
    pub theme_preference: ThemePreference,
    /// Automatically stop microphone capture after trailing VAD silence.
    pub auto_stop_vad: bool,
    /// Duration of trailing silence in milliseconds before auto-stopping recording.
    pub vad_silence_ms: u64,
    /// Minimum RMS speech threshold used by VAD silence detection and calibration.
    #[serde(default = "default_vad_rms_threshold")]
    pub vad_rms_threshold: f32,
    /// Optional preferred microphone device name (`None` uses the system default).
    #[serde(default)]
    pub preferred_input_device: Option<String>,
    /// Reference playback speed setting.
    pub playback_speed: PlaybackSpeed,
    /// Hide target text until after recording finishes.
    pub blind_mode: bool,
    /// Daily practice cap target.
    pub daily_goal: DailyGoal,
    /// Score-to-FSRS rating threshold configuration.
    pub score_thresholds: ScoreThresholds,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme_preference: ThemePreference::default(),
            auto_stop_vad: true,
            vad_silence_ms: 1000,
            vad_rms_threshold: default_vad_rms_threshold(),
            preferred_input_device: None,
            playback_speed: PlaybackSpeed::Normal,
            blind_mode: false,
            daily_goal: DailyGoal::default(),
            score_thresholds: ScoreThresholds::default(),
        }
    }
}

/// Visual severity level for non-fatal, dismissible user notifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ErrorSeverity {
    /// Non-blocking caution such as audio quality flags or preview notices.
    Warning,
    /// Recoverable operation failure such as a missing file or unplugged microphone.
    Error,
}

/// Dismissible notification toast surfaced in the UI for recoverable errors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppErrorToast {
    /// Unique identifier used to dismiss individual toasts.
    pub id: u64,
    /// Short headline summarizing what went wrong.
    pub title: String,
    /// Friendly explanation of the recoverable error.
    pub message: String,
    /// Optional actionable next step for the user.
    pub recovery_hint: Option<String>,
    /// Visual severity styling level.
    pub severity: ErrorSeverity,
    /// UTC timestamp when the toast was created.
    pub occurred_at: DateTime<Utc>,
}

impl AppErrorToast {
    /// Creates an error-severity dismissible toast.
    pub fn error(
        id: u64,
        title: impl Into<String>,
        message: impl Into<String>,
        recovery_hint: Option<String>,
        occurred_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            title: title.into(),
            message: message.into(),
            recovery_hint,
            severity: ErrorSeverity::Error,
            occurred_at,
        }
    }

    /// Creates a warning-severity dismissible toast.
    pub fn warning(
        id: u64,
        title: impl Into<String>,
        message: impl Into<String>,
        recovery_hint: Option<String>,
        occurred_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            title: title.into(),
            message: message.into(),
            recovery_hint,
            severity: ErrorSeverity::Warning,
            occurred_at,
        }
    }
}

/// Standard Japanese phonetic realization accepted for a target kana mora.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhoneticRealization {
    /// IPA transcription representing the accepted surface realization.
    pub ipa: String,
    /// Human-readable description of the allophonic or devoiced realization.
    pub label: String,
    /// Whether this realization involves standard high-vowel devoicing.
    pub is_devoiced: bool,
}

/// Equivalence and allophone acceptance rule for a single Japanese mora.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoraEquivalenceRule {
    /// Canonical normalized hiragana representation of the mora.
    pub canonical_kana: String,
    /// Surface kana spellings considered phonemically equivalent (e.g., `じ`/`ぢ`).
    pub equivalent_kana: Vec<String>,
    /// Standard phonetic realizations accepted for this mora.
    pub valid_realizations: Vec<PhoneticRealization>,
    /// Whether high-vowel devoicing is permitted in voiceless contexts.
    pub allows_vowel_devoicing: bool,
}

/// Input payload passed to a pluggable mora scorer implementation.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub struct MoraScoreRequest {
    /// Identifier of the Anki card being evaluated.
    pub card_id: i64,
    /// Expected kana reading sequence for the utterance.
    pub target_kana_sequence: String,
    /// Trimmed mono or stereo user microphone recording.
    pub user_audio: DecodedAudio,
    /// Optional native reference audio buffer for alignment and energy comparison.
    pub reference_audio: Option<DecodedAudio>,
    /// UTC timestamp when the recording was captured.
    pub recorded_at: DateTime<Utc>,
}

/// Aggregated output returned by a mora pronunciation scorer.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MoraEvaluation {
    /// Per-mora classification attempts in target sequence order.
    pub attempts: Vec<MoraAttempt>,
    /// Overall mora clarity percentage in `0.0..=100.0`.
    pub mora_accuracy_percent: f32,
    /// Identifier of the scorer backend that produced this evaluation.
    pub scorer_name: String,
    /// Informational notes describing accepted allophones or devoiced morae.
    pub accepted_allophone_notes: Vec<String>,
}

/// Combined pronunciation score breakdown and recommended FSRS rating for a review.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CombinedScoreBreakdown {
    /// Practice mode used to weight mora and pitch metrics.
    pub mode: PracticeMode,
    /// Mora clarity percentage (`0.0..=100.0`) when applicable.
    pub mora_accuracy_percent: Option<f32>,
    /// Pitch contour accuracy percentage (`0.0..=100.0`) when applicable.
    pub pitch_accuracy_percent: Option<f32>,
    /// Pitch accent error percentage (`0.0..=100.0`) when applicable.
    pub pitch_error_percent: Option<f32>,
    /// Weighted combined score percentage (`0.0..=100.0`).
    pub combined_score_percent: f32,
    /// Automated FSRS rating recommended from `combined_score_percent`.
    pub recommended_rating: FsrsRating,
}

/// Persisted review session record stored in Kotodama's local SQLite database.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewLogEntry {
    /// SQLite row identifier (`None` before insertion).
    pub id: Option<i64>,
    /// Associated Anki card ID.
    pub card_id: i64,
    /// Name of the Anki deck containing this card.
    pub deck_name: String,
    /// Target Japanese word or expression.
    pub word: String,
    /// Target kana reading.
    pub kana: String,
    /// Practice mode used during this review.
    pub practice_mode: PracticeMode,
    /// Mora clarity percentage (`0.0..=100.0`) when evaluated.
    pub mora_accuracy_percent: Option<f32>,
    /// Pitch accent error percentage (`0.0..=100.0`) when evaluated.
    pub pitch_error_percent: Option<f32>,
    /// Mean semitone DTW distance when pitch contour was analyzed.
    pub dtw_distance: Option<f32>,
    /// Combined numeric score percentage (`0.0..=100.0`).
    pub combined_score_percent: f32,
    /// Rating suggested automatically from score thresholds.
    pub recommended_rating: FsrsRating,
    /// Final FSRS rating persisted (either recommended or manual override).
    pub applied_rating: FsrsRating,
    /// Whether the user manually overrode the automated rating.
    pub was_manual_override: bool,
    /// Active drill duration in milliseconds.
    pub duration_ms: u64,
    /// UTC timestamp when the review was recorded.
    pub reviewed_at: DateTime<Utc>,
}

/// Per-review pitch accent error log record used for history views and exports.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PitchErrorLogEntry {
    /// Identifier of the parent [`ReviewLogEntry`].
    pub review_id: i64,
    /// Associated Anki card ID.
    pub card_id: i64,
    /// Name of the Anki deck containing this card.
    pub deck_name: String,
    /// Target Japanese word or expression.
    pub word: String,
    /// Target kana reading.
    pub kana: String,
    /// Pitch accent error percentage (`0.0..=100.0`).
    pub pitch_error_percent: f32,
    /// Mean semitone DTW distance between reference and user contours.
    pub dtw_distance: f32,
    /// UTC timestamp when the review was recorded.
    pub reviewed_at: DateTime<Utc>,
}

/// Daily aggregated accuracy summary point for analytics time-series charts.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DailyAccuracyPoint {
    /// UTC calendar date of the aggregated reviews.
    pub date: NaiveDate,
    /// Total reviews completed on `date`.
    pub review_count: u64,
    /// Average mora clarity percentage across reviews with mora scores.
    pub avg_mora_accuracy_percent: Option<f32>,
    /// Average pitch accent error percentage across reviews with pitch scores.
    pub avg_pitch_error_percent: Option<f32>,
    /// Average combined score percentage across all reviews on `date`.
    pub avg_combined_score_percent: f32,
}

/// Outcome returned after applying an FSRS review transition to a card.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FsrsSchedulingOutcome {
    /// Card FSRS state prior to this review.
    pub previous_state: FsrsCardState,
    /// Updated card FSRS state after applying `applied_rating`.
    pub next_state: FsrsCardState,
    /// Automated rating recommended by score thresholds.
    pub recommended_rating: FsrsRating,
    /// Final rating applied to the FSRS memory model.
    pub applied_rating: FsrsRating,
    /// Whether `applied_rating` differed from `recommended_rating` via manual override.
    pub was_manual_override: bool,
    /// Prospective scheduled intervals in days for `[Again, Hard, Good, Easy]`.
    pub preview_intervals_days: [u32; 4],
}

/// Complete JSON export snapshot of Kotodama's local pronunciation database.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExportBundle {
    /// UTC timestamp when this export snapshot was generated.
    pub exported_at: DateTime<Utc>,
    /// Kotodama application version that produced the export.
    pub app_version: String,
    /// Persisted daily streak and goal progress.
    pub streak: DailyStreak,
    /// Full chronological pronunciation review history.
    pub review_history: Vec<ReviewLogEntry>,
    /// Aggregated per-kana clarity and substitution matrix.
    pub kana_stats: Vec<KanaStats>,
    /// Pitch accent error log entries across reviewed cards.
    pub pitch_error_logs: Vec<PitchErrorLogEntry>,
    /// Independent FSRS scheduling states per Anki card ID.
    pub fsrs_states: Vec<FsrsCardState>,
}

/// Summary report describing the CSV files written to an export directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CsvExportReport {
    /// Output path of the review history CSV file.
    pub reviews_path: PathBuf,
    /// Output path of the per-kana confusion matrix CSV file.
    pub kana_confusion_path: PathBuf,
    /// Output path of the pitch accent error log CSV file.
    pub pitch_errors_path: PathBuf,
    /// Number of review log data rows exported.
    pub review_rows: usize,
    /// Number of kana confusion data rows exported.
    pub kana_rows: usize,
    /// Number of pitch error data rows exported.
    pub pitch_rows: usize,
}

/// Identifies which reference audio clip on a card is targeted during practice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PracticeTargetClip {
    /// Practice against the card's isolated word audio clip.
    Word,
    /// Practice against the card's example sentence audio clip.
    Sentence,
}

impl PracticeTargetClip {
    /// Human-readable label for the selected reference clip button.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Word => "Word Audio",
            Self::Sentence => "Sentence Audio",
        }
    }
}

/// Precomputed normalized reference pitch contour ready for canvas overlay rendering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReferenceContourPreview {
    /// Reference `F0` contour in semitones relative to `median_hz` (`None` = unvoiced).
    pub semitones: Vec<Option<f32>>,
    /// Reference speaker median `F0` in Hertz.
    pub median_hz: f32,
}

/// Complete pronunciation evaluation produced after recording a practice take.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PracticeEvaluationResult {
    /// Identifier of the evaluated Anki card.
    pub card_id: i64,
    /// Practice mode under which the recording was scored.
    pub mode: PracticeMode,
    /// Mora clarity evaluation when applicable to the mode.
    pub mora_evaluation: Option<MoraEvaluation>,
    /// Pitch contour DTW comparison when applicable to the mode.
    pub pitch_analysis: Option<PitchAnalysis>,
    /// Combined numeric score and automated FSRS rating recommendation.
    pub breakdown: CombinedScoreBreakdown,
    /// Active drill duration in milliseconds.
    pub duration_ms: u64,
    /// UTC timestamp when evaluation finished.
    pub evaluated_at: DateTime<Utc>,
}

/// Visual beat state during the pre-recording Combined Shadowing countdown.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShadowingCountdownState {
    /// Number of countdown beats remaining (`1..=total_beats`).
    pub beats_remaining: u8,
    /// Total number of countdown beats before recording begins.
    pub total_beats: u8,
    /// Progress across the entire countdown normalized to `0.0..=1.0`.
    pub beat_progress: f32,
}

impl ShadowingCountdownState {
    /// Computes the countdown beat state for `elapsed_ms`, or `None` once the countdown completes.
    pub fn from_elapsed_ms(
        elapsed_ms: u64,
        total_beats: u8,
        beat_duration_ms: u64,
    ) -> Option<Self> {
        if total_beats == 0 || beat_duration_ms == 0 {
            return None;
        }
        let total_ms = u64::from(total_beats).saturating_mul(beat_duration_ms);
        if elapsed_ms >= total_ms {
            return None;
        }
        let completed_beats = (elapsed_ms / beat_duration_ms) as u8;
        let beats_remaining = total_beats.saturating_sub(completed_beats).max(1);
        let beat_progress = (elapsed_ms as f32 / total_ms as f32).clamp(0.0, 1.0);
        Some(Self {
            beats_remaining,
            total_beats,
            beat_progress,
        })
    }
}

/// Aggregated metrics displayed at the end of a practice session.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PracticeSessionSummary {
    /// Total cards rated and persisted during this session.
    pub cards_completed: u32,
    /// Total active session duration in milliseconds.
    pub session_duration_ms: u64,
    /// Mean combined score percentage across evaluated cards in this session.
    pub average_score_percent: Option<f32>,
    /// Mean mora clarity percentage across mora-scored cards in this session.
    pub average_mora_accuracy_percent: Option<f32>,
    /// Mean pitch accent error percentage across pitch-scored cards in this session.
    pub average_pitch_error_percent: Option<f32>,
    /// Whether the user's configured daily goal cap was reached during this session.
    pub daily_cap_reached: bool,
    /// Number of cards whose FSRS rating was manually overridden.
    pub override_count: u32,
}

/// Logical keyboard shortcut action supported on the Practice screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PracticeShortcutAction {
    /// Play the current card's reference audio (`Space`).
    PlayReference,
    /// Start or stop microphone recording (`R`).
    ToggleRecord,
    /// Play back the user's most recent recorded take (`T`).
    PlayUserTake,
    /// Apply a manual FSRS rating override (`1`–`4`).
    RateCard(FsrsRating),
    /// Toggle Blind / Audio-Only card text hiding (`B`).
    ToggleBlindMode,
    /// Toggle between `0.75x` slow and `1.0x` normal playback speed (`S`).
    TogglePlaybackSpeed,
    /// Accept the automated FSRS rating recommendation (`Enter`).
    AcceptRecommended,
    /// Stop an active recording or cancel an active countdown (`Esc`).
    CancelActive,
}

/// Semantic visual state for mora chips in Mora Purity and Combined Shadowing views.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MoraChipTone {
    /// Neutral ungraded target mora chip.
    Neutral,
    /// Target mora matched a valid realization.
    Clear,
    /// Target mora was substituted or classified as unknown.
    Mistake,
}

/// Deck readiness counts passed into the Practice view.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PracticeDeckStatus {
    /// Whether an Anki deck is configured in the workspace.
    pub has_deck: bool,
    /// Number of active non-excluded learned cards ready for practice.
    pub active_card_count: usize,
    /// Number of excluded cards skipped for the active deck.
    pub excluded_card_count: usize,
}

/// Live microphone telemetry snapshot passed into the Practice view.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PracticeMicViewState {
    /// Whether Practice microphone capture is currently active.
    pub is_recording: bool,
    /// Most recent microphone level snapshot.
    pub mic_level: MicLevelSnapshot,
    /// Elapsed milliseconds in the active recording session.
    pub elapsed_ms: u64,
}

/// Time window used to filter trend charts on the Analytics Dashboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum DashboardTimeRange {
    /// Displays the most recent 7 calendar days including today.
    Last7Days,
    /// Displays the most recent 30 calendar days including today.
    #[default]
    Last30Days,
    /// Displays the complete recorded practice history.
    AllTime,
}

impl DashboardTimeRange {
    /// Ordered list of all dashboard time-range options.
    pub const ALL: [Self; 3] = [Self::Last7Days, Self::Last30Days, Self::AllTime];

    /// Short human-readable label for dashboard filter pills.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Last7Days => "7 Days",
            Self::Last30Days => "30 Days",
            Self::AllTime => "All Time",
        }
    }

    /// Maximum number of calendar days included in this range, or `None` for all history.
    pub const fn max_days(self) -> Option<i64> {
        match self {
            Self::Last7Days => Some(7),
            Self::Last30Days => Some(30),
            Self::AllTime => None,
        }
    }
}

/// Sort order applied to the per-kana clarity breakdown list on the Dashboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum KanaSortOrder {
    /// Orders kana with the lowest accuracy percentage first for targeted drilling.
    #[default]
    LowestAccuracy,
    /// Orders kana with the highest total attempt counts first.
    MostPracticed,
    /// Orders kana lexicographically by character.
    Alphabetical,
}

impl KanaSortOrder {
    /// Ordered list of all kana sorting options.
    pub const ALL: [Self; 3] = [
        Self::LowestAccuracy,
        Self::MostPracticed,
        Self::Alphabetical,
    ];

    /// Short human-readable label for kana sort pills.
    pub const fn label(self) -> &'static str {
        match self {
            Self::LowestAccuracy => "Lowest Accuracy",
            Self::MostPracticed => "Most Practiced",
            Self::Alphabetical => "Kana Order",
        }
    }
}

/// Aggregated per-card pitch accent error summary and recent trend samples.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardPitchSummary {
    /// Associated Anki card ID.
    pub card_id: i64,
    /// Name of the Anki deck containing this card.
    pub deck_name: String,
    /// Target Japanese word or expression.
    pub word: String,
    /// Target kana reading.
    pub kana: String,
    /// Number of pitch-scored reviews recorded for this card.
    pub review_count: u64,
    /// Most recent pitch accent error percentage (`0.0..=100.0`).
    pub latest_pitch_error_percent: f32,
    /// Mean pitch accent error percentage across all recorded reviews.
    pub average_pitch_error_percent: f32,
    /// Lowest (best) pitch accent error percentage achieved on this card.
    pub best_pitch_error_percent: f32,
    /// Mean semitone DTW distance from the most recent review.
    pub latest_dtw_distance: f32,
    /// UTC timestamp of the most recent pitch-scored review.
    pub last_reviewed_at: DateTime<Utc>,
    /// Chronological oldest-to-newest pitch error % samples (up to the last 12 reviews).
    pub recent_errors: Vec<f32>,
}

impl CardPitchSummary {
    /// Formats a concise human-readable label for pitch history lists.
    pub fn display_label(&self) -> String {
        let word = self.word.trim();
        let kana = self.kana.trim();
        match (word.is_empty(), kana.is_empty()) {
            (false, false) if word != kana => format!("{word} ({kana})"),
            (false, _) => word.to_string(),
            (true, false) => kana.to_string(),
            (true, true) => format!("Card #{}", self.card_id),
        }
    }

    /// Returns the net change in pitch error percentage across `recent_errors` (`latest - earliest`), or `None` with fewer than 2 samples.
    pub fn trend_delta_percent(&self) -> Option<f32> {
        let first = self.recent_errors.first().copied()?;
        let last = self.recent_errors.last().copied()?;
        if self.recent_errors.len() < 2 {
            return None;
        }
        Some(last - first)
    }
}

/// Complete analytics snapshot loaded from SQLite for the Dashboard view.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DashboardSnapshot {
    /// Persisted daily streak and goal completion counters.
    pub streak: DailyStreak,
    /// Total number of pronunciation reviews recorded in SQLite.
    pub total_reviews: u64,
    /// Mean mora clarity percentage across all reviews with mora scores.
    pub overall_mora_accuracy_percent: Option<f32>,
    /// Mean pitch accent error percentage across all reviews with pitch scores.
    pub overall_pitch_error_percent: Option<f32>,
    /// Number of distinct kana with recorded clarity statistics.
    pub tracked_kana_count: usize,
    /// Daily aggregated accuracy and pitch error time-series sorted chronologically.
    pub daily_series: Vec<DailyAccuracyPoint>,
    /// Per-kana clarity and confusion statistics across all recorded mora attempts.
    pub kana_stats: Vec<KanaStats>,
    /// Per-card pitch accent error summaries sorted by highest recent error first.
    pub card_pitch_summaries: Vec<CardPitchSummary>,
    /// Detailed per-review pitch error logs ordered newest first.
    pub pitch_error_logs: Vec<PitchErrorLogEntry>,
}

impl DashboardSnapshot {
    /// Returns `true` when no review, kana, or pitch history has been recorded yet.
    pub fn is_empty(&self) -> bool {
        self.total_reviews == 0
            && self.kana_stats.is_empty()
            && self.pitch_error_logs.is_empty()
    }

    /// Returns daily time-series points falling within `range` relative to `today`.
    pub fn filtered_series(
        &self,
        range: DashboardTimeRange,
        today: NaiveDate,
    ) -> Vec<&DailyAccuracyPoint> {
        match range.max_days() {
            Some(days) => {
                let cutoff = today - chrono::Duration::days(days.saturating_sub(1));
                self.daily_series
                    .iter()
                    .filter(|point| point.date >= cutoff)
                    .collect()
            }
            None => self.daily_series.iter().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn sample_time() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 26, 12, 0, 0)
            .single()
            .unwrap_or_else(Utc::now)
    }

    fn valid_mapping() -> FieldMapping {
        FieldMapping {
            word_field: "Expression".to_string(),
            kana_field: "Reading".to_string(),
            sentence_field: Some("Sentence".to_string()),
            word_audio_field: "WordAudio".to_string(),
            sentence_audio_field: Some("SentenceAudio".to_string()),
        }
    }

    #[test]
    fn field_mapping_validation_rejects_blank_required_fields() {
        let mut mapping = valid_mapping();
        assert!(mapping.validate().is_ok());

        mapping.word_field = "   ".to_string();
        assert!(mapping.validate().is_err());

        mapping.word_field = "Expression".to_string();
        mapping.kana_field.clear();
        assert!(mapping.validate().is_err());

        mapping.kana_field = "Reading".to_string();
        mapping.word_audio_field = "\t".to_string();
        assert!(mapping.validate().is_err());
    }

    #[test]
    fn deck_config_validation_checks_source_and_deck_name() {
        let config = DeckConfig {
            deck_name: "Core 2k".to_string(),
            source: DeckSource::LocalCollection {
                collection_path: PathBuf::from("/tmp/collection.anki2"),
                media_dir: PathBuf::from("/tmp/collection.media"),
            },
            field_mapping: valid_mapping(),
            last_synced_at: Some(sample_time()),
            flagged_audio_count: 2,
        };
        assert!(config.validate().is_ok());

        let bad_url = DeckConfig {
            deck_name: "Core 2k".to_string(),
            source: DeckSource::AnkiConnect {
                endpoint_url: "localhost:8765".to_string(),
            },
            field_mapping: valid_mapping(),
            last_synced_at: None,
            flagged_audio_count: 0,
        };
        assert!(bad_url.validate().is_err());
    }

    #[test]
    fn anki_card_learned_and_warning_flags() {
        let card = AnkiCard {
            card_id: 101,
            note_id: 202,
            deck_name: "Core 2k".to_string(),
            word: "靴".to_string(),
            kana: "くつ".to_string(),
            sentence: None,
            word_audio: "kutsu.mp3".to_string(),
            sentence_audio: None,
            anki_queue: AnkiCard::LEARNED_QUEUE,
            audio_issues: vec![AudioQualityIssue::TooShort { duration_ms: 140 }],
            last_pitch_error_percent: Some(12.5),
        };
        assert!(card.is_learned());
        assert!(card.has_audio_warnings());

        let new_card = AnkiCard {
            anki_queue: 0,
            audio_issues: Vec::new(),
            ..card
        };
        assert!(!new_card.is_learned());
        assert!(!new_card.has_audio_warnings());
    }

    #[test]
    fn pitch_analysis_validates_ranges_and_computes_accuracy() -> Result<()> {
        let analysis = PitchAnalysis::new(
            vec![Some(0.0), Some(3.0), None],
            vec![Some(-1.0), Some(2.5), None],
            1.4,
            14.5,
            135.0,
            210.0,
            sample_time(),
        )?;
        assert!((analysis.accuracy_percent() - 85.5).abs() < 0.001);

        assert!(
            PitchAnalysis::new(vec![], vec![], -0.1, 10.0, 120.0, 120.0, sample_time()).is_err()
        );
        assert!(
            PitchAnalysis::new(vec![], vec![], 0.5, 105.0, 120.0, 120.0, sample_time()).is_err()
        );
        assert!(
            PitchAnalysis::new(vec![], vec![], 0.5, 10.0, 0.0, 120.0, sample_time()).is_err()
        );
        Ok(())
    }

    #[test]
    fn kana_stats_computes_accuracy_and_top_five_breakdown() {
        let mut stats = KanaStats::new("つ");
        assert_eq!(stats.format_breakdown(), "つ: no attempts yet");

        let now = sample_time();
        for _ in 0..82 {
            stats.record_attempt(&MoraAttempt {
                card_id: 1,
                expected_kana: "つ".to_string(),
                heard_kana: Some("つ".to_string()),
                confidence: 0.95,
                recorded_at: now,
            });
        }
        for (heard, count) in [
            (None, 6),
            (Some("す"), 5),
            (Some("ず"), 4),
            (Some("すう"), 3),
        ] {
            for _ in 0..count {
                stats.record_attempt(&MoraAttempt {
                    card_id: 1,
                    expected_kana: "つ".to_string(),
                    heard_kana: heard.map(str::to_string),
                    confidence: 0.4,
                    recorded_at: now,
                });
            }
        }

        assert_eq!(stats.total_attempts, 100);
        assert!((stats.accuracy_percent() - 82.0).abs() < 0.01);
        assert_eq!(
            stats.format_breakdown(),
            "つ: 82% correct — 6% unknown, 5% す, 4% ず, 3% すう"
        );
    }

    #[test]
    fn kana_stats_top_mistakes_truncates_to_limit() {
        let mut stats = KanaStats::new("さ");
        let now = sample_time();
        for (idx, label) in ["ざ", "た", "しゃ", "ち", "か", "は"].iter().enumerate() {
            for _ in 0..=(idx + 1) {
                stats.record_attempt(&MoraAttempt {
                    card_id: 2,
                    expected_kana: "さ".to_string(),
                    heard_kana: Some((*label).to_string()),
                    confidence: 0.5,
                    recorded_at: now,
                });
            }
        }
        let top_five = stats.top_mistakes(5);
        assert_eq!(top_five.len(), 5);
        assert_eq!(top_five[0].heard_kana, "は");
        assert_eq!(top_five[4].heard_kana, "た");
    }

    #[test]
    fn score_thresholds_and_fsrs_due_state() -> Result<()> {
        let thresholds = ScoreThresholds::default();
        thresholds.validate()?;

        assert_eq!(thresholds.rate_score(95.0), FsrsRating::Easy);
        assert_eq!(thresholds.rate_score(78.0), FsrsRating::Good);
        assert_eq!(thresholds.rate_score(55.0), FsrsRating::Hard);
        assert_eq!(thresholds.rate_score(49.9), FsrsRating::Again);

        let invalid = ScoreThresholds {
            hard_min: 80.0,
            good_min: 75.0,
            easy_min: 90.0,
        };
        assert!(invalid.validate().is_err());

        let now = sample_time();
        let state = FsrsCardState::new_unseen(7, now);
        assert!(state.is_due(now));
        Ok(())
    }

    #[test]
    fn daily_streak_updates_consecutive_same_day_and_gap_resets() {
        let mut streak = DailyStreak::default();
        let day1 = NaiveDate::from_ymd_opt(2026, 11, 24).unwrap_or(NaiveDate::MIN);
        let day2 = NaiveDate::from_ymd_opt(2026, 11, 25).unwrap_or(NaiveDate::MIN);
        let day5 = NaiveDate::from_ymd_opt(2026, 11, 28).unwrap_or(NaiveDate::MIN);

        streak.record_practice(day1, 4, 3);
        assert_eq!(streak.current_streak_days, 1);
        assert_eq!(streak.cards_practiced_today, 4);
        assert!(!streak.is_daily_goal_met());

        streak.record_practice(day1, 6, 4);
        assert_eq!(streak.current_streak_days, 1);
        assert_eq!(streak.cards_practiced_today, 10);
        assert!(streak.is_daily_goal_met());
        assert!((streak.goal_progress_ratio() - 1.0).abs() < 0.001);

        streak.record_practice(day2, 5, 2);
        assert_eq!(streak.current_streak_days, 2);
        assert_eq!(streak.longest_streak_days, 2);
        assert_eq!(streak.cards_practiced_today, 5);

        streak.record_practice(day5, 3, 2);
        assert_eq!(streak.current_streak_days, 1);
        assert_eq!(streak.longest_streak_days, 2);
        assert_eq!(streak.total_sessions, 4);
    }

    #[test]
    fn theme_preference_resolves_and_cycles_deterministically() {
        assert_eq!(ThemePreference::default(), ThemePreference::System);
        assert_eq!(ThemePreference::System.resolve(Some(true)), ResolvedTheme::Dark);
        assert_eq!(ThemePreference::System.resolve(Some(false)), ResolvedTheme::Light);
        assert_eq!(ThemePreference::System.resolve(None), ResolvedTheme::Light);
        assert_eq!(ThemePreference::Light.resolve(Some(true)), ResolvedTheme::Light);
        assert_eq!(ThemePreference::Dark.resolve(Some(false)), ResolvedTheme::Dark);

        assert_eq!(ThemePreference::System.cycle_next(), ThemePreference::Light);
        assert_eq!(ThemePreference::Light.cycle_next(), ThemePreference::Dark);
        assert_eq!(ThemePreference::Dark.cycle_next(), ThemePreference::System);
        assert!(ResolvedTheme::Dark.is_dark());
        assert!(!ResolvedTheme::Light.is_dark());
    }

    #[test]
    fn serde_round_trip_preserves_settings_and_deck_config() -> Result<()> {
        let settings = AppSettings {
            theme_preference: ThemePreference::Dark,
            auto_stop_vad: false,
            vad_silence_ms: 1200,
            vad_rms_threshold: 0.04,
            preferred_input_device: Some("Studio USB Mic".to_string()),
            playback_speed: PlaybackSpeed::Slow,
            blind_mode: true,
            daily_goal: DailyGoal::Minutes(15),
            score_thresholds: ScoreThresholds::default(),
        };
        let encoded = serde_json::to_string(&settings)?;
        let decoded: AppSettings = serde_json::from_str(&encoded)?;
        assert_eq!(settings, decoded);
        assert!((decoded.playback_speed.multiplier() - 0.75).abs() < 0.001);

        let legacy_json = r#"{
            "auto_stop_vad": true,
            "vad_silence_ms": 1000,
            "playback_speed": "Normal",
            "blind_mode": false,
            "daily_goal": { "Cards": 10 },
            "score_thresholds": { "hard_min": 50.0, "good_min": 75.0, "easy_min": 90.0 }
        }"#;
        let migrated: AppSettings = serde_json::from_str(legacy_json)?;
        assert_eq!(migrated.theme_preference, ThemePreference::System);
        assert!((migrated.vad_rms_threshold - 0.025).abs() < 1e-6);
        assert_eq!(migrated.preferred_input_device, None);
        Ok(())
    }

    #[test]
    fn decoded_audio_to_mono_handles_mono_stereo_and_empty() {
        let stereo = DecodedAudio {
            samples: vec![0.4, -0.2, 0.8, 0.4],
            sample_rate: 16_000,
            channels: 2,
        };
        let mono = stereo.to_mono();
        assert_eq!(mono.len(), 2);
        assert!((mono[0] - 0.1).abs() < 1e-5);
        assert!((mono[1] - 0.6).abs() < 1e-5);

        let already_mono = DecodedAudio {
            samples: vec![0.25, -0.5],
            sample_rate: 16_000,
            channels: 1,
        };
        assert_eq!(already_mono.to_mono(), vec![0.25, -0.5]);

        let invalid = DecodedAudio {
            samples: vec![0.1, 0.2],
            sample_rate: 16_000,
            channels: 0,
        };
        assert!(invalid.to_mono().is_empty());
    }

    #[test]
    fn vad_and_pitch_configs_validate_and_mic_snapshot_computes_levels() -> Result<()> {
        let mut vad = VadConfig::default();
        vad.validate()?;
        vad.rms_threshold = 0.0;
        assert!(vad.validate().is_err());

        let settings = AppSettings {
            vad_rms_threshold: 0.06,
            vad_silence_ms: 800,
            auto_stop_vad: false,
            ..AppSettings::default()
        };
        let from_settings = VadConfig::from_settings(&settings);
        assert!((from_settings.rms_threshold - 0.06).abs() < 1e-6);
        assert_eq!(from_settings.trailing_silence_ms, 800);
        assert!(!from_settings.auto_stop);

        let mut pitch_cfg = PitchExtractionConfig::default();
        pitch_cfg.validate()?;
        pitch_cfg.max_f0_hz = 50.0;
        assert!(pitch_cfg.validate().is_err());

        let silent = MicLevelSnapshot::from_samples(&[0.0, 0.0, 0.0], 0.025);
        assert!(!silent.is_speech);
        assert!(!silent.is_clipping);
        assert!((silent.db_fs + 60.0).abs() < 1e-4);

        let loud = MicLevelSnapshot::from_samples(&[0.5, -0.99, 0.5], 0.025);
        assert!(loud.is_speech);
        assert!(loud.is_clipping);
        assert!(loud.meter_ratio > 0.7);
        assert_eq!(
            CaptureStopReason::AutoStopSilence.label(),
            "Auto-stopped (VAD silence)"
        );
        Ok(())
    }

    #[test]
    fn audio_quality_issue_summary_and_deck_report_format_concisely() {
        assert_eq!(AudioQualityIssue::MissingFile.summary(), "Missing audio file");
        assert_eq!(
            AudioQualityIssue::TooShort { duration_ms: 120 }.summary(),
            "Too short (120 ms < 200 ms)"
        );
        assert_eq!(
            AudioQualityIssue::ClippedAudio { peak_ratio: 1.0 }.summary(),
            "Clipped audio (peak 1.00)"
        );
        assert_eq!(
            AudioQualityIssue::DecodeFailed {
                reason: "bad header".to_string()
            }
            .summary(),
            "Decode failed: bad header"
        );

        let clean = DeckAudioReport {
            total_cards: 12,
            total_clips_checked: 20,
            ..DeckAudioReport::default()
        };
        assert!(!clean.has_issues());
        assert_eq!(
            clean.summary_line(),
            "All 20 clips across 12 learned cards passed audio checks."
        );

        let flagged = DeckAudioReport {
            total_cards: 12,
            total_clips_checked: 20,
            flagged_cards: 3,
            missing_files: 1,
            too_short_clips: 1,
            clipped_clips: 1,
            decode_failures: 0,
        };
        assert!(flagged.has_issues());
        assert_eq!(
            flagged.summary_line(),
            "3 of 12 cards flagged (1 missing, 1 < 200ms, 1 clipped, 0 decode errors)."
        );
    }

    #[test]
    fn decoded_audio_duration_and_workspace_config_round_trip() -> Result<()> {
        let audio = DecodedAudio {
            samples: vec![0.0; 44_100],
            sample_rate: 44_100,
            channels: 2,
        };
        assert_eq!(audio.frame_count(), 22_050);
        assert_eq!(audio.duration_ms(), 500);

        let zero_rate = DecodedAudio {
            samples: vec![0.0; 10],
            sample_rate: 0,
            channels: 1,
        };
        assert_eq!(zero_rate.duration_ms(), 0);

        let config = DeckConfig {
            deck_name: "Core 2k".to_string(),
            source: DeckSource::AnkiConnect {
                endpoint_url: "http://127.0.0.1:8765".to_string(),
            },
            field_mapping: valid_mapping(),
            last_synced_at: Some(sample_time()),
            flagged_audio_count: 1,
        };
        let mut saved_mappings = BTreeMap::new();
        saved_mappings.insert("Core 2k".to_string(), valid_mapping());
        let workspace = DeckWorkspaceConfig {
            active_deck: Some(config),
            saved_mappings,
            excluded_card_ids: BTreeMap::from([(
                "Core 2k".to_string(),
                BTreeSet::from([101_i64, 202_i64]),
            )]),
            excluded_card_labels: BTreeMap::from([
                (101_i64, "靴 (くつ)".to_string()),
                (202_i64, "橋 (はし)".to_string()),
            ]),
        };

        let encoded = serde_json::to_string(&workspace)?;
        let decoded: DeckWorkspaceConfig = serde_json::from_str(&encoded)?;
        assert_eq!(workspace, decoded);
        Ok(())
    }

    #[test]
    fn deck_workspace_exclusions_filter_active_queue_and_migrate_legacy_json() -> Result<()> {
        let legacy_json = r#"{
            "active_deck": null,
            "saved_mappings": {}
        }"#;
        let mut workspace: DeckWorkspaceConfig = serde_json::from_str(legacy_json)?;
        assert!(workspace.excluded_card_ids.is_empty());

        let clean_card = AnkiCard {
            card_id: 1,
            note_id: 10,
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
            card_id: 2,
            note_id: 20,
            deck_name: "Core 2k".to_string(),
            word: "靴".to_string(),
            kana: "くつ".to_string(),
            sentence: None,
            word_audio: "missing.mp3".to_string(),
            sentence_audio: None,
            anki_queue: AnkiCard::LEARNED_QUEUE,
            audio_issues: vec![AudioQualityIssue::MissingFile],
            last_pitch_error_percent: None,
        };
        let cards = vec![clean_card.clone(), bad_card.clone()];

        assert_eq!(workspace.filter_active_cards("Core 2k", &cards).len(), 2);
        assert_eq!(
            workspace.exclude_invalid_audio_cards("Core 2k", &cards),
            1
        );
        assert!(workspace.is_card_excluded("Core 2k", 2));
        assert!(!workspace.is_card_excluded("Core 2k", 1));
        assert_eq!(workspace.excluded_count_for_deck("Core 2k"), 1);
        assert_eq!(
            workspace.excluded_entries_for_deck("Core 2k"),
            vec![(2, "靴 (くつ)".to_string())]
        );

        let active = workspace.filter_active_cards("Core 2k", &cards);
        assert_eq!(active, vec![&clean_card]);

        assert!(workspace.restore_card("Core 2k", 2));
        assert!(!workspace.is_card_excluded("Core 2k", 2));
        assert_eq!(workspace.excluded_count_for_deck("Core 2k"), 0);
        assert_eq!(workspace.filter_active_cards("Core 2k", &cards).len(), 2);

        assert!(workspace.exclude_card("Core 2k", 1, Some(clean_card.display_label())));
        assert_eq!(workspace.restore_all_for_deck("Core 2k"), 1);
        assert_eq!(workspace.excluded_count_for_deck("Core 2k"), 0);
        Ok(())
    }

    #[test]
    fn db_enum_round_trips_and_threshold_presets_validate() -> Result<()> {
        for mode in PracticeMode::ALL {
            assert_eq!(PracticeMode::from_db_str(mode.as_db_str())?, mode);
        }
        assert!(PracticeMode::from_db_str("UnknownMode").is_err());

        for rating in FsrsRating::ALL {
            assert_eq!(FsrsRating::from_db_str(rating.as_db_str())?, rating);
        }
        assert!(FsrsRating::from_db_str("Perfect").is_err());

        for goal in [DailyGoal::Cards(15), DailyGoal::Minutes(10)] {
            assert_eq!(
                DailyGoal::from_parts(goal.kind_str(), goal.target_value())?,
                goal
            );
        }
        assert!(DailyGoal::from_parts("Hours", 1).is_err());

        for preset in [
            ScoreThresholds::lenient(),
            ScoreThresholds::balanced(),
            ScoreThresholds::strict(),
        ] {
            preset.validate()?;
        }
        assert_eq!(ScoreThresholds::default(), ScoreThresholds::balanced());
        Ok(())
    }

    #[test]
    fn phase_4_scoring_and_export_models_round_trip_json() -> Result<()> {
        let now = sample_time();
        let state = FsrsCardState::new_unseen(42, now);
        assert!((state.stability_fast - 0.0).abs() < f64::EPSILON);

        let bundle = ExportBundle {
            exported_at: now,
            app_version: "0.4.0".to_string(),
            streak: DailyStreak::default(),
            review_history: vec![ReviewLogEntry {
                id: Some(1),
                card_id: 42,
                deck_name: "Core 2k".to_string(),
                word: "靴".to_string(),
                kana: "くつ".to_string(),
                practice_mode: PracticeMode::CombinedShadowing,
                mora_accuracy_percent: Some(90.0),
                pitch_error_percent: Some(8.5),
                dtw_distance: Some(0.51),
                combined_score_percent: 90.75,
                recommended_rating: FsrsRating::Easy,
                applied_rating: FsrsRating::Good,
                was_manual_override: true,
                duration_ms: 1200,
                reviewed_at: now,
            }],
            kana_stats: vec![KanaStats::new("つ")],
            pitch_error_logs: vec![PitchErrorLogEntry {
                review_id: 1,
                card_id: 42,
                deck_name: "Core 2k".to_string(),
                word: "靴".to_string(),
                kana: "くつ".to_string(),
                pitch_error_percent: 8.5,
                dtw_distance: 0.51,
                reviewed_at: now,
            }],
            fsrs_states: vec![state],
        };

        let encoded = serde_json::to_string(&bundle)?;
        let decoded: ExportBundle = serde_json::from_str(&encoded)?;
        assert_eq!(bundle, decoded);

        let csv_report = CsvExportReport {
            reviews_path: PathBuf::from("/tmp/reviews.csv"),
            kana_confusion_path: PathBuf::from("/tmp/kana.csv"),
            pitch_errors_path: PathBuf::from("/tmp/pitch.csv"),
            review_rows: 1,
            kana_rows: 1,
            pitch_rows: 1,
        };
        assert_eq!(csv_report.review_rows, 1);
        Ok(())
    }

    #[test]
    fn phase_5_practice_models_validate_countdown_and_clip_labels() {
        assert_eq!(PracticeTargetClip::Word.label(), "Word Audio");
        assert_eq!(PracticeTargetClip::Sentence.label(), "Sentence Audio");

        assert_eq!(ShadowingCountdownState::from_elapsed_ms(0, 0, 500), None);
        assert_eq!(ShadowingCountdownState::from_elapsed_ms(0, 3, 0), None);

        let beat_3 = ShadowingCountdownState::from_elapsed_ms(0, 3, 500)
            .expect("0ms should start at beat 3");
        assert_eq!(beat_3.beats_remaining, 3);
        assert_eq!(beat_3.total_beats, 3);
        assert!((beat_3.beat_progress - 0.0).abs() < 1e-6);

        let beat_2 = ShadowingCountdownState::from_elapsed_ms(500, 3, 500)
            .expect("500ms should transition to beat 2");
        assert_eq!(beat_2.beats_remaining, 2);
        assert!((beat_2.beat_progress - (1.0 / 3.0)).abs() < 1e-4);

        let beat_1 = ShadowingCountdownState::from_elapsed_ms(1499, 3, 500)
            .expect("1499ms should remain on beat 1");
        assert_eq!(beat_1.beats_remaining, 1);

        assert_eq!(ShadowingCountdownState::from_elapsed_ms(1500, 3, 500), None);
        assert_eq!(ShadowingCountdownState::from_elapsed_ms(2200, 3, 500), None);
    }

    #[test]
    fn phase_6_dashboard_models_filter_series_and_compute_pitch_trends() {
        assert_eq!(DashboardTimeRange::default(), DashboardTimeRange::Last30Days);
        assert_eq!(DashboardTimeRange::Last7Days.label(), "7 Days");
        assert_eq!(DashboardTimeRange::Last7Days.max_days(), Some(7));
        assert_eq!(DashboardTimeRange::Last30Days.max_days(), Some(30));
        assert_eq!(DashboardTimeRange::AllTime.max_days(), None);

        assert_eq!(KanaSortOrder::default(), KanaSortOrder::LowestAccuracy);
        assert_eq!(KanaSortOrder::MostPracticed.label(), "Most Practiced");
        assert_eq!(KanaSortOrder::Alphabetical.label(), "Kana Order");

        let mut summary = CardPitchSummary {
            card_id: 7,
            deck_name: "Core 2k".to_string(),
            word: "橋".to_string(),
            kana: "はし".to_string(),
            review_count: 1,
            latest_pitch_error_percent: 24.0,
            average_pitch_error_percent: 24.0,
            best_pitch_error_percent: 24.0,
            latest_dtw_distance: 1.2,
            last_reviewed_at: sample_time(),
            recent_errors: vec![],
        };
        assert_eq!(summary.display_label(), "橋 (はし)");
        assert_eq!(summary.trend_delta_percent(), None);

        summary.recent_errors = vec![24.0];
        assert_eq!(summary.trend_delta_percent(), None);

        summary.recent_errors = vec![30.0, 22.0, 14.5];
        let delta = summary
            .trend_delta_percent()
            .expect("multi-sample pitch history should yield delta");
        assert!((delta - (-15.5)).abs() < 1e-5);

        let same_kana = CardPitchSummary {
            word: "みず".to_string(),
            kana: "みず".to_string(),
            ..summary.clone()
        };
        assert_eq!(same_kana.display_label(), "みず");

        let blank_card = CardPitchSummary {
            word: "  ".to_string(),
            kana: "".to_string(),
            ..summary.clone()
        };
        assert_eq!(blank_card.display_label(), "Card #7");

        let empty_snap = DashboardSnapshot::default();
        assert!(empty_snap.is_empty());

        let today = NaiveDate::from_ymd_opt(2026, 11, 27).unwrap_or(NaiveDate::MIN);
        let snap = DashboardSnapshot {
            total_reviews: 3,
            daily_series: vec![
                DailyAccuracyPoint {
                    date: today - chrono::Duration::days(40),
                    review_count: 1,
                    avg_mora_accuracy_percent: Some(70.0),
                    avg_pitch_error_percent: Some(25.0),
                    avg_combined_score_percent: 72.5,
                },
                DailyAccuracyPoint {
                    date: today - chrono::Duration::days(10),
                    review_count: 2,
                    avg_mora_accuracy_percent: Some(80.0),
                    avg_pitch_error_percent: Some(18.0),
                    avg_combined_score_percent: 81.0,
                },
                DailyAccuracyPoint {
                    date: today - chrono::Duration::days(2),
                    review_count: 3,
                    avg_mora_accuracy_percent: Some(88.0),
                    avg_pitch_error_percent: Some(12.0),
                    avg_combined_score_percent: 88.0,
                },
            ],
            ..DashboardSnapshot::default()
        };
        assert!(!snap.is_empty());
        assert_eq!(snap.filtered_series(DashboardTimeRange::Last7Days, today).len(), 1);
        assert_eq!(snap.filtered_series(DashboardTimeRange::Last30Days, today).len(), 2);
        assert_eq!(snap.filtered_series(DashboardTimeRange::AllTime, today).len(), 3);
    }
}

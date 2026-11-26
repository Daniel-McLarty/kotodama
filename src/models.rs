// SPDX-License-Identifier: MPL-2.0

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Result, bail};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

/// Top-level navigation tabs in the Kotodama desktop interface.
#[allow(dead_code)]
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

#[allow(dead_code)]
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
#[allow(dead_code)]
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
#[allow(dead_code)]
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

#[allow(dead_code)]
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
#[allow(dead_code)]
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

#[allow(dead_code)]
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

/// Audio quality problems detected during pre-flight deck validation.
#[allow(dead_code)]
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

/// Graduated Anki card imported into Kotodama for pronunciation practice.
#[allow(dead_code)]
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

#[allow(dead_code)]
impl AnkiCard {
    /// Anki queue value representing graduated cards in the review state.
    pub const LEARNED_QUEUE: i32 = 2;

    /// Returns `true` when the card is in Anki's graduated/learned review queue.
    pub const fn is_learned(&self) -> bool {
        self.anki_queue == Self::LEARNED_QUEUE
    }

    /// Returns `true` when one or more reference audio quality issues were flagged.
    pub fn has_audio_warnings(&self) -> bool {
        !self.audio_issues.is_empty()
    }
}

/// Available pronunciation drill modes in Kotodama.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PracticeMode {
    /// Focuses on isolated kana/mora articulation clarity.
    MoraPurity,
    /// Compares normalized pitch contour against reference audio via DTW.
    PitchAccent,
    /// Combines countdown shadowing with simultaneous mora and pitch scoring.
    CombinedShadowing,
}

#[allow(dead_code)]
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
}

/// Result of comparing user pitch contour against reference audio.
#[allow(dead_code)]
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

#[allow(dead_code)]
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
#[allow(dead_code)]
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
}

/// Configurable percentage thresholds mapping combined practice scores to FSRS ratings.
#[allow(dead_code)]
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
        Self {
            hard_min: 50.0,
            good_min: 75.0,
            easy_min: 90.0,
        }
    }
}

#[allow(dead_code)]
impl ScoreThresholds {
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
#[allow(dead_code)]
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

#[allow(dead_code)]
impl DailyGoal {
    /// Formats the daily goal for display in UI headers and settings.
    pub fn label(self) -> String {
        match self {
            Self::Cards(count) => format!("{count} cards / day"),
            Self::Minutes(minutes) => format!("{minutes} min / day"),
        }
    }
}

/// Tracks consecutive practice days, daily goal progress, and lifetime sessions.
#[allow(dead_code)]
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

#[allow(dead_code)]
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
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PlaybackSpeed {
    /// Pitch-preserving 0.75x slow playback for shadowing fast native sentences.
    Slow,
    /// Original 1.0x reference playback speed.
    Normal,
}

#[allow(dead_code)]
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

/// User-configurable ergonomics, audio, and scheduling settings.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppSettings {
    /// Automatically stop microphone capture after trailing VAD silence.
    pub auto_stop_vad: bool,
    /// Duration of trailing silence in milliseconds before auto-stopping recording.
    pub vad_silence_ms: u64,
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
            auto_stop_vad: true,
            vad_silence_ms: 1000,
            playback_speed: PlaybackSpeed::Normal,
            blind_mode: false,
            daily_goal: DailyGoal::default(),
            score_thresholds: ScoreThresholds::default(),
        }
    }
}

/// Visual severity level for non-fatal, dismissible user notifications.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ErrorSeverity {
    /// Non-blocking caution such as audio quality flags or preview notices.
    Warning,
    /// Recoverable operation failure such as a missing file or unplugged microphone.
    Error,
}

/// Dismissible notification toast surfaced in the UI for recoverable errors.
#[allow(dead_code)]
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

#[allow(dead_code)]
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
    fn serde_round_trip_preserves_settings_and_deck_config() -> Result<()> {
        let settings = AppSettings {
            auto_stop_vad: false,
            vad_silence_ms: 1200,
            playback_speed: PlaybackSpeed::Slow,
            blind_mode: true,
            daily_goal: DailyGoal::Minutes(15),
            score_thresholds: ScoreThresholds::default(),
        };
        let encoded = serde_json::to_string(&settings)?;
        let decoded: AppSettings = serde_json::from_str(&encoded)?;
        assert_eq!(settings, decoded);
        assert!((decoded.playback_speed.multiplier() - 0.75).abs() < 0.001);
        Ok(())
    }
}

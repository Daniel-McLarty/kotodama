// SPDX-License-Identifier: MPL-2.0

use std::collections::BTreeSet;
use std::time::Instant;

use iced::alignment::Vertical;
use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers, key};
use iced::widget::text::Alignment as TextAlignment;
use iced::widget::{
    button, canvas, column, container, progress_bar, row, space, text, toggler,
};
use iced::{Alignment, Color, Element, Length, Pixels, Point, Rectangle, Renderer, Size, Theme, mouse};

use crate::models::{
    AnkiCard, AppSettings, DailyGoal, DailyStreak, DecodedAudio, FsrsCardState, FsrsRating,
    MoraAttempt, MoraChipTone, NavTab, PlaybackSpeed, PracticeDeckStatus, PracticeEvaluationResult,
    PracticeMicViewState, PracticeMode, PracticeSessionSummary, PracticeShortcutAction,
    PracticeTargetClip, RecordedTake, ReferenceContourPreview, ResolvedTheme,
    ShadowingCountdownState,
};
use crate::scoring::mora::{mora_equivalence_rule, segment_kana_into_morae};
use crate::ui::theme::{self, ThemeTokens};
use crate::ui::{Message, MicMeterProgram};

const COUNTDOWN_TOTAL_BEATS: u8 = 3;
const COUNTDOWN_BEAT_MS: u64 = 500;

/// High-level screen phase within the Practice tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PracticePhase {
    /// Mode selection and readiness overview prior to starting a drill session.
    Lobby,
    /// Asynchronous deck/due-queue preparation indicator.
    Loading {
        /// Human-readable loading status text.
        label: String,
    },
    /// Active flashcard pronunciation drill view.
    ActiveCard,
    /// End-of-session summary screen after finishing due cards or hitting the daily goal.
    SessionComplete {
        /// Whether the session paused specifically because the daily goal cap was met.
        cap_reached: bool,
    },
}

/// Mutable state backing Kotodama's active Practice session, audio buffers, and evaluations.
#[derive(Debug, Clone)]
pub struct PracticeState {
    /// Current screen phase inside the Practice tab.
    pub phase: PracticePhase,
    /// Ordered card queue for the active session.
    pub queue: Vec<AnkiCard>,
    /// Index of the active card inside `queue`.
    pub current_index: usize,
    /// Count of due/unseen cards discovered when the session started.
    pub due_count_hint: Option<usize>,
    /// Whether the session is running in Extra Study mode across all active cards.
    pub extra_study_mode: bool,
    /// Whether the user chose to continue past the daily session goal cap.
    pub cap_override_active: bool,
    /// Which reference clip (`Word` or `Sentence`) is selected for the active card.
    pub target_clip: PracticeTargetClip,
    /// Whether card text is visible (always `true` outside Blind Mode, or after speaking).
    pub text_revealed: bool,
    /// Selected mora index in Mora Purity mode for viewing allophone/devoicing guidance.
    pub selected_mora_index: Option<usize>,
    /// Whether Combined Shadowing automatically starts the 3-2-1 countdown after reference playback.
    pub shadow_auto_record: bool,
    /// Whether the app is waiting for reference playback to finish before starting the countdown.
    pub awaiting_shadow_countdown: bool,
    /// Active pre-recording 3-2-1 countdown state in Combined Shadowing.
    pub countdown: Option<ShadowingCountdownState>,
    /// Timestamp when the active shadowing countdown began.
    pub countdown_started_at: Option<Instant>,
    /// Monotonic token incremented whenever the active card or target clip changes.
    pub card_generation: u64,
    /// Monotonic token incremented whenever a new recording or reset invalidates prior takes.
    pub take_generation: u64,
    /// Cached decoded PCM buffer for the active card's selected reference clip.
    pub reference_audio: Option<DecodedAudio>,
    /// Cached normalized `F0` contour for the active card's selected reference clip.
    pub reference_contour: Option<ReferenceContourPreview>,
    /// Whether reference decoding and `F0` extraction are running in the background.
    pub is_decoding_reference: bool,
    /// Non-fatal reference audio decode error for the active card, if any.
    pub reference_audio_error: Option<String>,
    /// Whether reference audio playback is currently running.
    pub is_playing_reference: bool,
    /// Whether playback of the user's recorded take is currently running.
    pub is_playing_user_take: bool,
    /// Whether background mora/pitch scoring is currently running.
    pub is_scoring: bool,
    /// Whether an FSRS review commit is currently running in SQLite.
    pub is_committing: bool,
    /// Most recent captured microphone take for the active card.
    pub last_take: Option<RecordedTake>,
    /// Most recent mora and/or pitch evaluation result for the active card.
    pub last_evaluation: Option<PracticeEvaluationResult>,
    /// Persisted FSRS state for the active card prior to rating.
    pub current_fsrs_state: Option<FsrsCardState>,
    /// Prospective `[Again, Hard, Good, Easy]` intervals in days for the active card.
    pub preview_intervals_days: Option<[u32; 4]>,
    /// Timestamp when the active card was presented.
    pub card_started_at: Option<Instant>,
    /// Total active practice time accumulated in the current session in milliseconds.
    pub session_elapsed_ms: u64,
    /// Whole session minutes already flushed into `DailyStreak.minutes_practiced_today`.
    pub flushed_session_minutes: u32,
    /// Aggregated end-of-session metrics.
    pub summary: PracticeSessionSummary,
    score_sum: f64,
    score_count: u32,
    mora_sum: f64,
    mora_count: u32,
    pitch_err_sum: f64,
    pitch_err_count: u32,
    requeued_card_ids: BTreeSet<i64>,
}

impl Default for PracticeState {
    fn default() -> Self {
        Self {
            phase: PracticePhase::Lobby,
            queue: Vec::new(),
            current_index: 0,
            due_count_hint: None,
            extra_study_mode: false,
            cap_override_active: false,
            target_clip: PracticeTargetClip::Word,
            text_revealed: true,
            selected_mora_index: Some(0),
            shadow_auto_record: true,
            awaiting_shadow_countdown: false,
            countdown: None,
            countdown_started_at: None,
            card_generation: 0,
            take_generation: 0,
            reference_audio: None,
            reference_contour: None,
            is_decoding_reference: false,
            reference_audio_error: None,
            is_playing_reference: false,
            is_playing_user_take: false,
            is_scoring: false,
            is_committing: false,
            last_take: None,
            last_evaluation: None,
            current_fsrs_state: None,
            preview_intervals_days: None,
            card_started_at: None,
            session_elapsed_ms: 0,
            flushed_session_minutes: 0,
            summary: PracticeSessionSummary::default(),
            score_sum: 0.0,
            score_count: 0,
            mora_sum: 0.0,
            mora_count: 0,
            pitch_err_sum: 0.0,
            pitch_err_count: 0,
            requeued_card_ids: BTreeSet::new(),
        }
    }
}

impl PracticeState {
    /// Returns the currently active [`AnkiCard`] in the practice queue, if any.
    pub fn current_card(&self) -> Option<&AnkiCard> {
        self.queue.get(self.current_index)
    }

    /// Initializes a new practice session queue and prepares the first card.
    pub fn begin_session(
        &mut self,
        queue: Vec<AnkiCard>,
        mode: PracticeMode,
        blind_mode: bool,
        extra_study: bool,
        due_count_hint: Option<usize>,
    ) {
        self.queue = queue;
        self.current_index = 0;
        self.due_count_hint = due_count_hint;
        self.extra_study_mode = extra_study;
        self.cap_override_active = false;
        self.session_elapsed_ms = 0;
        self.flushed_session_minutes = 0;
        self.summary = PracticeSessionSummary::default();
        self.score_sum = 0.0;
        self.score_count = 0;
        self.mora_sum = 0.0;
        self.mora_count = 0;
        self.pitch_err_sum = 0.0;
        self.pitch_err_count = 0;
        self.requeued_card_ids.clear();

        if self.queue.is_empty() {
            self.phase = PracticePhase::SessionComplete { cap_reached: false };
            self.card_started_at = None;
        } else {
            self.phase = PracticePhase::ActiveCard;
            self.prepare_current_card(mode, blind_mode);
        }
    }

    /// Resets per-card audio, contour, evaluation, and visibility state for the current queue item.
    pub fn prepare_current_card(&mut self, mode: PracticeMode, blind_mode: bool) {
        self.card_generation = self.card_generation.saturating_add(1);
        self.take_generation = self.take_generation.saturating_add(1);
        self.target_clip = preferred_clip_for_card(self.current_card(), mode);
        self.text_revealed = !blind_mode;
        self.selected_mora_index = self
            .current_card()
            .filter(|card| !segment_kana_into_morae(&card.kana).is_empty())
            .map(|_| 0);
        self.awaiting_shadow_countdown = false;
        self.countdown = None;
        self.countdown_started_at = None;
        self.reference_audio = None;
        self.reference_contour = None;
        self.is_decoding_reference = false;
        self.reference_audio_error = None;
        self.is_playing_reference = false;
        self.is_playing_user_take = false;
        self.is_scoring = false;
        self.is_committing = false;
        self.last_take = None;
        self.last_evaluation = None;
        self.current_fsrs_state = None;
        self.preview_intervals_days = None;
        self.card_started_at = Some(Instant::now());
    }

    /// Switches target clip (`Word` vs `Sentence`) on the active card and clears stale reference buffers.
    pub fn switch_target_clip(&mut self, clip: PracticeTargetClip) {
        self.card_generation = self.card_generation.saturating_add(1);
        self.take_generation = self.take_generation.saturating_add(1);
        self.target_clip = clip;
        self.awaiting_shadow_countdown = false;
        self.countdown = None;
        self.countdown_started_at = None;
        self.reference_audio = None;
        self.reference_contour = None;
        self.is_decoding_reference = false;
        self.reference_audio_error = None;
        self.is_playing_reference = false;
        self.is_scoring = false;
        self.last_evaluation = None;
    }

    /// Starts the 3-2-1 Combined Shadowing pre-recording visual countdown.
    pub fn start_countdown(&mut self, now: Instant) {
        self.awaiting_shadow_countdown = false;
        self.countdown_started_at = Some(now);
        self.countdown = ShadowingCountdownState::from_elapsed_ms(
            0,
            COUNTDOWN_TOTAL_BEATS,
            COUNTDOWN_BEAT_MS,
        );
    }

    /// Cancels any pending or active Combined Shadowing countdown.
    pub fn cancel_countdown(&mut self) {
        self.awaiting_shadow_countdown = false;
        self.countdown = None;
        self.countdown_started_at = None;
    }

    /// Advances the active countdown against `now`, returning `true` on the tick it completes.
    pub fn advance_countdown(&mut self, now: Instant) -> bool {
        if self.countdown.is_none() {
            return false;
        }
        let Some(started_at) = self.countdown_started_at else {
            self.countdown = None;
            return false;
        };
        let elapsed_ms = now.saturating_duration_since(started_at).as_millis() as u64;
        match ShadowingCountdownState::from_elapsed_ms(
            elapsed_ms,
            COUNTDOWN_TOTAL_BEATS,
            COUNTDOWN_BEAT_MS,
        ) {
            Some(next) => {
                self.countdown = Some(next);
                false
            }
            None => {
                self.countdown = None;
                self.countdown_started_at = None;
                true
            }
        }
    }

    /// Returns newly crossed whole session minutes since the previous DB flush and advances the marker.
    pub fn take_newly_elapsed_minutes(&mut self) -> u32 {
        let total_whole = (self.session_elapsed_ms / 60_000) as u32;
        let delta = total_whole.saturating_sub(self.flushed_session_minutes);
        self.flushed_session_minutes = total_whole;
        delta
    }

    /// Returns today's practiced minutes including unflushed active session time for live progress display.
    pub fn effective_today_minutes(&self, persisted_minutes: u32) -> u32 {
        let total_whole = (self.session_elapsed_ms / 60_000) as u32;
        let unflushed = total_whole.saturating_sub(self.flushed_session_minutes);
        persisted_minutes.saturating_add(unflushed)
    }

    /// Records a committed review into session summary stats and re-queues `Again` cards once.
    pub fn record_committed_card(
        &mut self,
        eval: Option<&PracticeEvaluationResult>,
        was_manual_override: bool,
        applied_rating: FsrsRating,
    ) {
        self.summary.cards_completed = self.summary.cards_completed.saturating_add(1);
        self.summary.session_duration_ms = self.session_elapsed_ms;
        if was_manual_override {
            self.summary.override_count = self.summary.override_count.saturating_add(1);
        }

        if let Some(result) = eval {
            self.score_sum += f64::from(result.breakdown.combined_score_percent);
            self.score_count = self.score_count.saturating_add(1);
            self.summary.average_score_percent =
                Some((self.score_sum / f64::from(self.score_count)) as f32);

            if let Some(mora) = result.breakdown.mora_accuracy_percent {
                self.mora_sum += f64::from(mora);
                self.mora_count = self.mora_count.saturating_add(1);
                self.summary.average_mora_accuracy_percent =
                    Some((self.mora_sum / f64::from(self.mora_count)) as f32);
            }
            if let Some(pitch_err) = result.breakdown.pitch_error_percent {
                self.pitch_err_sum += f64::from(pitch_err);
                self.pitch_err_count = self.pitch_err_count.saturating_add(1);
                self.summary.average_pitch_error_percent =
                    Some((self.pitch_err_sum / f64::from(self.pitch_err_count)) as f32);
            }
        }

        if applied_rating == FsrsRating::Again
            && let Some(card) = self.current_card().cloned()
            && self.requeued_card_ids.insert(card.card_id)
        {
            self.queue.push(card);
        }
    }

    /// Resets session state back to the Practice Lobby.
    pub fn reset_to_lobby(&mut self) {
        *self = Self {
            shadow_auto_record: self.shadow_auto_record,
            ..Self::default()
        };
    }
}

/// Chooses the default target clip (`Sentence` when available in Combined Shadowing, else `Word`).
pub fn preferred_clip_for_card(card: Option<&AnkiCard>, mode: PracticeMode) -> PracticeTargetClip {
    if mode == PracticeMode::CombinedShadowing
        && card.is_some_and(|c| {
            c.sentence_audio
                .as_deref()
                .is_some_and(|name| !name.trim().is_empty())
        })
    {
        PracticeTargetClip::Sentence
    } else {
        PracticeTargetClip::Word
    }
}

/// Maps an Iced keyboard press to a logical [`PracticeShortcutAction`], supporting active IME layouts via physical keys.
pub fn map_key_to_shortcut(
    key: &Key,
    physical_key: key::Physical,
    modifiers: Modifiers,
) -> Option<PracticeShortcutAction> {
    if modifiers.control() || modifiers.alt() || modifiers.logo() {
        return None;
    }

    match key {
        Key::Named(Named::Space) => Some(PracticeShortcutAction::PlayReference),
        Key::Named(Named::Enter) => Some(PracticeShortcutAction::AcceptRecommended),
        Key::Named(Named::Escape) => Some(PracticeShortcutAction::CancelActive),
        _ => match key.to_latin(physical_key).map(|ch| ch.to_ascii_lowercase()) {
            Some('r') => Some(PracticeShortcutAction::ToggleRecord),
            Some('t') => Some(PracticeShortcutAction::PlayUserTake),
            Some('b') => Some(PracticeShortcutAction::ToggleBlindMode),
            Some('s') => Some(PracticeShortcutAction::TogglePlaybackSpeed),
            Some('1') => Some(PracticeShortcutAction::RateCard(FsrsRating::Again)),
            Some('2') => Some(PracticeShortcutAction::RateCard(FsrsRating::Hard)),
            Some('3') => Some(PracticeShortcutAction::RateCard(FsrsRating::Good)),
            Some('4') => Some(PracticeShortcutAction::RateCard(FsrsRating::Easy)),
            _ => None,
        },
    }
}

/// Computes padded semitone vertical bounds `(min_st, max_st)` spanning both contours and `0 st`.
pub fn compute_semitone_bounds(reference: &[Option<f32>], user: &[Option<f32>]) -> (f32, f32) {
    let mut min_val = f32::INFINITY;
    let mut max_val = f32::NEG_INFINITY;

    for val in reference.iter().chain(user.iter()).flatten() {
        if val.is_finite() {
            min_val = min_val.min(*val);
            max_val = max_val.max(*val);
        }
    }

    if !min_val.is_finite() || !max_val.is_finite() {
        return (-6.0, 6.0);
    }

    let min_st = (min_val - 1.0).floor().clamp(-24.0, -6.0);
    let max_st = (max_val + 1.0).ceil().clamp(6.0, 24.0);
    (min_st, max_st)
}

/// Maps voiced contour samples onto `plot` canvas coordinates, splitting segments at unvoiced (`None`) gaps.
pub fn map_contour_points(
    contour: &[Option<f32>],
    plot: Rectangle,
    min_st: f32,
    max_st: f32,
) -> Vec<Vec<Point>> {
    let span = max_st - min_st;
    if contour.is_empty() || plot.width <= 0.0 || plot.height <= 0.0 || !span.is_finite() || span <= 0.0 {
        return Vec::new();
    }

    let mut segments: Vec<Vec<Point>> = Vec::new();
    let mut current: Vec<Point> = Vec::new();
    let last_idx = contour.len().saturating_sub(1);

    for (idx, value) in contour.iter().enumerate() {
        match value.filter(|v| v.is_finite()) {
            Some(st) => {
                let x = if contour.len() == 1 {
                    plot.x + plot.width * 0.5
                } else {
                    plot.x + (idx as f32 / last_idx as f32) * plot.width
                };
                let norm_y = ((st - min_st) / span).clamp(0.0, 1.0);
                let y = plot.y + plot.height * (1.0 - norm_y);
                current.push(Point::new(x, y));
            }
            None => {
                if !current.is_empty() {
                    segments.push(std::mem::take(&mut current));
                }
            }
        }
    }

    if !current.is_empty() {
        segments.push(current);
    }

    segments
}

/// Iced `Canvas` program rendering overlaid normalized reference and user `F0` pitch contours.
pub struct PitchContourProgram<'a> {
    /// Reference normalized semitone contour (`None` = unvoiced).
    pub reference_semitones: &'a [Option<f32>],
    /// User normalized semitone contour (`None` = unvoiced).
    pub user_semitones: &'a [Option<f32>],
    /// Active Light or Dark appearance.
    pub resolved_theme: ResolvedTheme,
}

impl canvas::Program<Message> for PitchContourProgram<'_> {
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
        let top_pad = 16.0_f32;
        let bottom_pad = 16.0_f32;
        let plot = Rectangle {
            x: left_pad,
            y: top_pad,
            width: (width - left_pad - right_pad).max(1.0),
            height: (height - top_pad - bottom_pad).max(1.0),
        };

        let (min_st, max_st) =
            compute_semitone_bounds(self.reference_semitones, self.user_semitones);
        let span = (max_st - min_st).max(1.0);

        for &tick in &[-12.0_f32, -6.0, -3.0, 0.0, 3.0, 6.0, 12.0] {
            if tick < min_st - 0.01 || tick > max_st + 0.01 {
                continue;
            }
            let norm_y = ((tick - min_st) / span).clamp(0.0, 1.0);
            let y = plot.y + plot.height * (1.0 - norm_y);
            let is_zero = tick.abs() < 0.01;
            let line_color = if is_zero {
                Color::from_rgba(
                    colors.text_muted.r,
                    colors.text_muted.g,
                    colors.text_muted.b,
                    0.50,
                )
            } else {
                Color::from_rgba(colors.border.r, colors.border.g, colors.border.b, 0.85)
            };

            let grid_line =
                canvas::Path::line(Point::new(plot.x, y), Point::new(plot.x + plot.width, y));
            frame.stroke(
                &grid_line,
                canvas::Stroke::default()
                    .with_color(line_color)
                    .with_width(if is_zero { 1.4 } else { 1.0 }),
            );

            let label = if tick > 0.0 {
                format!("+{tick:.0}st")
            } else {
                format!("{tick:.0}st")
            };
            frame.fill_text(canvas::Text {
                content: label,
                position: Point::new(left_pad - 8.0, y),
                color: colors.text_muted,
                size: Pixels(11.0),
                align_x: TextAlignment::Right,
                align_y: Vertical::Center,
                ..canvas::Text::default()
            });
        }

        let ref_segments = map_contour_points(self.reference_semitones, plot, min_st, max_st);
        let user_segments = map_contour_points(self.user_semitones, plot, min_st, max_st);

        if ref_segments.is_empty() && user_segments.is_empty() {
            frame.fill_text(canvas::Text {
                content: "Play reference or record a take to plot F0 pitch contours".to_string(),
                position: Point::new(plot.x + plot.width * 0.5, plot.y + plot.height * 0.5),
                color: colors.text_muted,
                size: Pixels(13.0),
                align_x: TextAlignment::Center,
                align_y: Vertical::Center,
                ..canvas::Text::default()
            });
            return vec![frame.into_geometry()];
        }

        draw_contour_segments(&mut frame, &ref_segments, colors.sage, 2.8);
        draw_contour_segments(&mut frame, &user_segments, colors.coral, 3.2);

        vec![frame.into_geometry()]
    }
}

fn draw_contour_segments(
    frame: &mut canvas::Frame,
    segments: &[Vec<Point>],
    color: Color,
    stroke_width: f32,
) {
    for segment in segments {
        if segment.len() >= 2 {
            let path = canvas::Path::new(|builder| {
                builder.move_to(segment[0]);
                for &pt in &segment[1..] {
                    builder.line_to(pt);
                }
            });
            frame.stroke(
                &path,
                canvas::Stroke::default()
                    .with_color(color)
                    .with_width(stroke_width)
                    .with_line_cap(canvas::LineCap::Round)
                    .with_line_join(canvas::LineJoin::Round),
            );
        }

        let dot_radius = if segment.len() == 1 { 3.5 } else { 2.0 };
        let stride = if segment.len() > 60 { 2 } else { 1 };
        for (idx, &pt) in segment.iter().enumerate() {
            if idx % stride == 0 || idx + 1 == segment.len() {
                let dot = canvas::Path::circle(pt, dot_radius);
                frame.fill(&dot, color);
            }
        }
    }
}

/// Renders the pronunciation Practice screen across Lobby, Loading, Active Card, and Session Summary phases.
pub fn view<'a>(
    state: &'a PracticeState,
    active_mode: PracticeMode,
    settings: &'a AppSettings,
    streak: &'a DailyStreak,
    deck_info: PracticeDeckStatus,
    mic_state: PracticeMicViewState,
    resolved_theme: ResolvedTheme,
) -> Element<'a, Message> {
    let colors = theme::tokens(resolved_theme);

    match &state.phase {
        PracticePhase::Lobby => render_lobby(
            active_mode,
            settings,
            streak,
            deck_info,
            resolved_theme,
            colors,
        ),
        PracticePhase::Loading { label } => render_loading(label, colors),
        PracticePhase::ActiveCard => render_active_card(
            state,
            active_mode,
            settings,
            streak,
            mic_state,
            resolved_theme,
            colors,
        ),
        PracticePhase::SessionComplete { cap_reached } => render_session_complete(
            state,
            streak,
            *cap_reached,
            colors,
        ),
    }
}

fn render_lobby<'a>(
    active_mode: PracticeMode,
    settings: &'a AppSettings,
    streak: &'a DailyStreak,
    deck_info: PracticeDeckStatus,
    _resolved_theme: ResolvedTheme,
    colors: &'static ThemeTokens,
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

    let deck_badge = if deck_info.has_deck {
        let label = if deck_info.active_card_count > 0 || deck_info.excluded_card_count > 0 {
            format!("Deck Ready ({} active)", deck_info.active_card_count)
        } else {
            "Deck Ready".to_string()
        };
        container(text(label).size(12).color(colors.sage))
            .padding([4, 10])
            .style(theme::badge_style(colors.sage_soft, colors.sage))
    } else {
        container(text("No Deck Connected Yet").size(12).color(colors.amber))
            .padding([4, 10])
            .style(theme::badge_style(colors.amber_soft, colors.amber))
    };

    let next_speed = if settings.playback_speed == PlaybackSpeed::Slow {
        PlaybackSpeed::Normal
    } else {
        PlaybackSpeed::Slow
    };
    let blind_label = if settings.blind_mode {
        "Blind Mode: On"
    } else {
        "Blind Mode: Off"
    };

    let mut status_badges = row![
        deck_badge,
        button(text(settings.playback_speed.label()).size(12))
            .padding([5, 10])
            .style(theme::pill_button_style(
                settings.playback_speed == PlaybackSpeed::Slow
            ))
            .on_press(Message::SelectPlaybackSpeed(next_speed)),
        button(text(blind_label).size(12))
            .padding([5, 10])
            .style(theme::pill_button_style(settings.blind_mode))
            .on_press(Message::ToggleBlindMode(!settings.blind_mode)),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    if deck_info.excluded_card_count > 0 {
        status_badges = status_badges.push(
            container(
                text(format!("{} excluded", deck_info.excluded_card_count))
                    .size(12)
                    .color(colors.text_muted),
            )
            .padding([4, 10])
            .style(theme::badge_style(colors.card_muted, colors.border)),
        );
    }

    let action_row = if deck_info.has_deck {
        row![
            button(text("Start Practice Session").size(14))
                .padding([10, 18])
                .style(theme::primary_button_style)
                .on_press(Message::PracticeStartSession { extra_study: false }),
            button(text("Extra Study (All Active Cards)").size(14))
                .padding([10, 16])
                .style(theme::secondary_button_style)
                .on_press(Message::PracticeStartSession { extra_study: true }),
        ]
        .spacing(10)
        .align_y(Alignment::Center)
    } else {
        row![
            button(text("Start Practice Session").size(14))
                .padding([10, 18])
                .style(theme::primary_button_style)
                .on_press(Message::PracticeStartSession { extra_study: false }),
            button(text("Open Deck Setup").size(14))
                .padding([10, 16])
                .style(theme::secondary_button_style)
                .on_press(Message::SelectTab(NavTab::Setup)),
        ]
        .spacing(10)
        .align_y(Alignment::Center)
    };

    let goal_progress_text = format_daily_goal_progress(streak, streak.minutes_practiced_today);

    let mode_card = container(
        column![
            row![
                text(active_mode.label()).size(20).color(colors.text_primary),
                space().width(Length::Fill),
                status_badges,
            ]
            .spacing(12)
            .align_y(Alignment::Center),
            text(active_mode.short_description())
                .size(14)
                .color(colors.text_muted),
            container(
                column![
                    row![
                        text("Daily Goal Progress").size(13).color(colors.text_primary),
                        space().width(Length::Fill),
                        text(goal_progress_text).size(13).color(colors.sage),
                    ]
                    .spacing(12),
                    progress_bar(0.0..=1.0, streak.goal_progress_ratio()),
                ]
                .spacing(8),
            )
            .width(Length::Fill)
            .padding(14)
            .style(theme::muted_card_style),
            action_row,
        ]
        .spacing(14),
    )
    .width(Length::Fill)
    .padding(20)
    .style(theme::card_style);

    let shortcuts_card = container(
        column![
            text("Keyboard-First Controls")
                .size(17)
                .color(colors.text_primary),
            row![
                shortcut_chip("Space", "Play reference", colors),
                shortcut_chip("R", "Start / stop mic", colors),
                shortcut_chip("T", "Play your take", colors),
                shortcut_chip("1 – 4", "FSRS override", colors),
            ]
            .spacing(10),
            row![
                shortcut_chip("B", "Toggle Blind Mode", colors),
                shortcut_chip("S", "0.75x / 1.0x speed", colors),
                shortcut_chip("Enter", "Accept score", colors),
                shortcut_chip("Esc", "Stop / cancel", colors),
            ]
            .spacing(10),
        ]
        .spacing(10),
    )
    .width(Length::Fill)
    .padding(18)
    .style(theme::card_style);

    column![mode_pills, mode_card, shortcuts_card]
        .spacing(18)
        .width(Length::Fill)
        .into()
}

fn render_loading<'a>(label: &'a str, colors: &'static ThemeTokens) -> Element<'a, Message> {
    container(
        column![
            text("Preparing Practice Queue")
                .size(20)
                .color(colors.text_primary),
            text(label).size(14).color(colors.text_muted),
            button(text("Back to Lobby").size(13))
                .padding([8, 14])
                .style(theme::secondary_button_style)
                .on_press(Message::PracticeEndSession),
        ]
        .spacing(12),
    )
    .width(Length::Fill)
    .padding(24)
    .style(theme::card_style)
    .into()
}

fn render_active_card<'a>(
    state: &'a PracticeState,
    active_mode: PracticeMode,
    settings: &'a AppSettings,
    streak: &'a DailyStreak,
    mic_state: PracticeMicViewState,
    resolved_theme: ResolvedTheme,
    colors: &'static ThemeTokens,
) -> Element<'a, Message> {
    let Some(card) = state.current_card() else {
        return render_session_complete(state, streak, false, colors);
    };

    let header_bar = render_session_header(state, active_mode, settings, streak, colors);
    let prompt_card = render_prompt_card(state, card, settings, colors);
    let mode_panel = match active_mode {
        PracticeMode::MoraPurity => render_mora_panel(state, card, colors),
        PracticeMode::PitchAccent => render_pitch_panel(state, resolved_theme, colors),
        PracticeMode::CombinedShadowing => {
            render_shadowing_panel(state, card, resolved_theme, colors)
        }
    };
    let capture_bar = render_capture_bar(state, settings, mic_state, resolved_theme, colors);
    let rating_bar = render_rating_bar(state, colors);
    let shortcut_strip = container(
        text("Shortcuts: Space Play Ref · R Record · T Play Take · 1–4 Rate · Enter Accept · B Blind · S 0.75x/1.0x · Esc Cancel")
            .size(12)
            .color(colors.text_muted),
    )
    .width(Length::Fill)
    .padding([8, 14])
    .style(theme::muted_card_style);

    column![
        header_bar,
        prompt_card,
        mode_panel,
        capture_bar,
        rating_bar,
        shortcut_strip,
    ]
    .spacing(16)
    .width(Length::Fill)
    .into()
}

fn render_session_header<'a>(
    state: &'a PracticeState,
    active_mode: PracticeMode,
    settings: &'a AppSettings,
    streak: &'a DailyStreak,
    colors: &'static ThemeTokens,
) -> Element<'a, Message> {
    let mode_pills = PracticeMode::ALL
        .iter()
        .fold(row![].spacing(6).align_y(Alignment::Center), |r, &mode| {
            r.push(
                button(text(mode.label()).size(12))
                    .padding([6, 12])
                    .style(theme::pill_button_style(mode == active_mode))
                    .on_press(Message::SelectPracticeMode(mode)),
            )
        });

    let next_speed = if settings.playback_speed == PlaybackSpeed::Slow {
        PlaybackSpeed::Normal
    } else {
        PlaybackSpeed::Slow
    };
    let speed_short = if settings.playback_speed == PlaybackSpeed::Slow {
        "0.75x"
    } else {
        "1.0x"
    };

    let card_pos = format!(
        "Card {} / {}",
        state.current_index.saturating_add(1),
        state.queue.len().max(1)
    );
    let elapsed_label = format_elapsed_mm_ss(state.session_elapsed_ms);

    let effective_minutes = state.effective_today_minutes(streak.minutes_practiced_today);
    let goal_ratio = match streak.daily_goal {
        DailyGoal::Cards(goal) => {
            if goal == 0 {
                1.0
            } else {
                (streak.cards_practiced_today as f32 / goal as f32).clamp(0.0, 1.0)
            }
        }
        DailyGoal::Minutes(goal) => {
            if goal == 0 {
                1.0
            } else {
                (effective_minutes as f32 / goal as f32).clamp(0.0, 1.0)
            }
        }
    };
    let goal_text = format_daily_goal_progress(streak, effective_minutes);

    container(
        column![
            row![
                container(text(card_pos).size(13).color(colors.coral_text))
                    .padding([5, 12])
                    .style(theme::badge_style(colors.coral_soft, colors.coral)),
                container(text(format!("Timer {elapsed_label}")).size(12).color(colors.text_muted))
                    .padding([5, 10])
                    .style(theme::badge_style(colors.card_muted, colors.border)),
                space().width(Length::Fill),
                mode_pills,
                button(text(speed_short).size(12))
                    .padding([6, 12])
                    .style(theme::pill_button_style(
                        settings.playback_speed == PlaybackSpeed::Slow
                    ))
                    .on_press(Message::SelectPlaybackSpeed(next_speed)),
                button(text(if settings.blind_mode { "Blind: On" } else { "Blind: Off" }).size(12))
                    .padding([6, 12])
                    .style(theme::pill_button_style(settings.blind_mode))
                    .on_press(Message::ToggleBlindMode(!settings.blind_mode)),
                button(text("End Session").size(12))
                    .padding([6, 12])
                    .style(theme::secondary_button_style)
                    .on_press(Message::PracticeEndSession),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            row![
                text("Daily Goal").size(12).color(colors.text_muted),
                container(progress_bar(0.0..=1.0, goal_ratio))
                    .width(Length::Fill),
                text(goal_text).size(12).color(colors.sage),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        ]
        .spacing(10),
    )
    .width(Length::Fill)
    .padding(14)
    .style(theme::card_style)
    .into()
}

fn render_prompt_card<'a>(
    state: &'a PracticeState,
    card: &'a AnkiCard,
    settings: &'a AppSettings,
    colors: &'static ThemeTokens,
) -> Element<'a, Message> {
    let fsrs_label = match &state.current_fsrs_state {
        Some(fsrs) if fsrs.reps > 0 => format!(
            "Reps: {} · Stability {:.1}d",
            fsrs.reps, fsrs.stability
        ),
        _ => "New Pronunciation Card".to_string(),
    };

    let mut badges = row![
        container(text(&card.deck_name).size(12).color(colors.text_primary))
            .padding([4, 10])
            .style(theme::badge_style(colors.card_muted, colors.border)),
        container(text(fsrs_label).size(12).color(colors.sage))
            .padding([4, 10])
            .style(theme::badge_style(colors.sage_soft, colors.sage)),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    if let Some(pitch_err) = card.last_pitch_error_percent {
        badges = badges.push(
            container(
                text(format!("Last pitch error: {pitch_err:.1}%"))
                    .size(12)
                    .color(colors.coral_text),
            )
            .padding([4, 10])
            .style(theme::badge_style(colors.coral_soft, colors.coral)),
        );
    }

    if card.has_audio_warnings() {
        badges = badges.push(
            container(text("Audio Warning").size(12).color(colors.amber))
                .padding([4, 10])
                .style(theme::badge_style(colors.amber_soft, colors.amber)),
        );
    }

    let has_sentence_audio = card
        .sentence_audio
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty());
    if has_sentence_audio {
        badges = badges.push(space().width(Length::Fill));
        for clip in [PracticeTargetClip::Word, PracticeTargetClip::Sentence] {
            let selected = state.target_clip == clip;
            badges = badges.push(
                button(text(clip.label()).size(12))
                    .padding([5, 12])
                    .style(theme::pill_button_style(selected))
                    .on_press(Message::PracticeSelectTargetClip(clip)),
            );
        }
    }

    let mut body = column![badges].spacing(12);

    if settings.blind_mode && !state.text_revealed {
        body = body.push(
            container(
                column![
                    text("Blind / Audio-Only Mode")
                        .size(18)
                        .color(colors.coral_text),
                    text("Listen with Space and speak your take with R — text reveals automatically once your recording finishes.")
                        .size(13)
                        .color(colors.text_muted),
                    button(text("Reveal Text Now").size(13))
                        .padding([8, 14])
                        .style(theme::secondary_button_style)
                        .on_press(Message::PracticeRevealText),
                ]
                .spacing(8),
            )
            .width(Length::Fill)
            .padding(18)
            .style(theme::muted_card_style),
        );
    } else {
        let word_display = if card.word.trim().is_empty() {
            card.kana.as_str()
        } else {
            card.word.as_str()
        };
        body = body.push(text(word_display).size(34).color(colors.text_primary));
        if !card.kana.trim().is_empty() && card.kana.trim() != word_display.trim() {
            body = body.push(text(&card.kana).size(18).color(colors.coral_text));
        }
        if let Some(sentence) = card.sentence.as_deref().filter(|s| !s.trim().is_empty()) {
            body = body.push(
                container(text(sentence).size(15).color(colors.text_primary))
                    .width(Length::Fill)
                    .padding(12)
                    .style(theme::muted_card_style),
            );
        }
    }

    if let Some(ref_err) = &state.reference_audio_error {
        body = body.push(
            container(text(format!("Reference audio note: {ref_err}")).size(12).color(colors.amber))
                .width(Length::Fill)
                .padding([8, 12])
                .style(theme::gigo_banner_style),
        );
    } else if state.is_decoding_reference {
        body = body.push(
            text("Loading reference audio and pitch contour...")
                .size(12)
                .color(colors.text_muted),
        );
    }

    container(body)
        .width(Length::Fill)
        .padding(20)
        .style(theme::card_style)
        .into()
}

fn render_mora_panel<'a>(
    state: &'a PracticeState,
    card: &'a AnkiCard,
    colors: &'static ThemeTokens,
) -> Element<'a, Message> {
    let morae = segment_kana_into_morae(&card.kana);
    let attempts = state
        .last_evaluation
        .as_ref()
        .and_then(|eval| eval.mora_evaluation.as_ref())
        .map(|m| m.attempts.as_slice());

    let mut content = column![
        row![
            text("Mora Purity Breakdown")
                .size(17)
                .color(colors.text_primary),
            space().width(Length::Fill),
            text(if let Some(eval) = state.last_evaluation.as_ref().and_then(|e| e.mora_evaluation.as_ref()) {
                format!("Mora Clarity: {:.0}%", eval.mora_accuracy_percent)
            } else {
                format!("{} morae · Click a mora to inspect allophone rules", morae.len())
            })
            .size(13)
            .color(colors.sage),
        ]
        .align_y(Alignment::Center),
    ]
    .spacing(12);

    content = content.push(render_mora_chips_row(
        &morae,
        attempts,
        state.selected_mora_index,
        colors,
    ));

    if let Some(selected_idx) = state.selected_mora_index
        && let Some(kana) = morae.get(selected_idx)
    {
        let rule = mora_equivalence_rule(kana);
        let realization_text = if rule.valid_realizations.is_empty() {
            "Standard Japanese mora realization.".to_string()
        } else {
            rule.valid_realizations
                .iter()
                .map(|r| format!("{} ({})", r.ipa, r.label))
                .collect::<Vec<_>>()
                .join(" · ")
        };
        let devoicing_note = if rule.allows_vowel_devoicing {
            " · High-vowel devoicing accepted when voiceless consonant holds the mora beat."
        } else {
            ""
        };
        let equiv_note = if rule.equivalent_kana.len() > 1 {
            format!(" · Equivalent: {}", rule.equivalent_kana.join(" / "))
        } else {
            String::new()
        };

        content = content.push(
            container(
                column![
                    text(format!("Mora Guide: {kana}{equiv_note}"))
                        .size(13)
                        .color(colors.coral_text),
                    text(format!("{realization_text}{devoicing_note}"))
                        .size(12)
                        .color(colors.text_muted),
                ]
                .spacing(4),
            )
            .width(Length::Fill)
            .padding(12)
            .style(theme::muted_card_style),
        );
    }

    if let Some(mora_eval) = state
        .last_evaluation
        .as_ref()
        .and_then(|eval| eval.mora_evaluation.as_ref())
        && !mora_eval.accepted_allophone_notes.is_empty()
    {
        content = content.push(
            container(
                text(format!(
                    "Accepted Japanese realizations: {}",
                    mora_eval.accepted_allophone_notes.join(" · ")
                ))
                .size(12)
                .color(colors.sage),
            )
            .width(Length::Fill)
            .padding([8, 12])
            .style(theme::muted_card_style),
        );
    }

    container(content)
        .width(Length::Fill)
        .padding(18)
        .style(theme::card_style)
        .into()
}

fn render_mora_chips_row<'a>(
    morae: &[String],
    attempts: Option<&[MoraAttempt]>,
    selected_idx: Option<usize>,
    colors: &'static ThemeTokens,
) -> Element<'a, Message> {
    if morae.is_empty() {
        return text("No scorable kana reading found on this card.")
            .size(13)
            .color(colors.text_muted)
            .into();
    }

    let mut chips = row![].spacing(8).align_y(Alignment::Center);
    for (idx, kana) in morae.iter().enumerate() {
        let attempt = attempts.and_then(|list| list.get(idx));
        let tone = match attempt {
            Some(att) if att.is_correct() => MoraChipTone::Clear,
            Some(_) => MoraChipTone::Mistake,
            None => MoraChipTone::Neutral,
        };
        let label = match attempt {
            Some(att) if att.is_correct() => {
                format!("{kana} ✓ {:.0}%", att.confidence * 100.0)
            }
            Some(att) => format!("{kana} → {}", att.heard_label()),
            None => kana.clone(),
        };
        chips = chips.push(
            button(text(label).size(14))
                .padding([8, 12])
                .style(theme::mora_chip_style(tone, selected_idx == Some(idx)))
                .on_press(Message::PracticeSelectMora(idx)),
        );
    }
    chips.into()
}

fn render_pitch_panel<'a>(
    state: &'a PracticeState,
    resolved_theme: ResolvedTheme,
    colors: &'static ThemeTokens,
) -> Element<'a, Message> {
    let pitch_analysis = state
        .last_evaluation
        .as_ref()
        .and_then(|eval| eval.pitch_analysis.as_ref());

    let empty_slice: &[Option<f32>] = &[];
    let (ref_semitones, ref_median_label) = match pitch_analysis {
        Some(analysis) => (
            analysis.reference_semitones.as_slice(),
            format!("Reference ({:.0} Hz median)", analysis.reference_median_hz),
        ),
        None => match &state.reference_contour {
            Some(preview) => (
                preview.semitones.as_slice(),
                format!("Reference ({:.0} Hz median)", preview.median_hz),
            ),
            None => (empty_slice, "Reference".to_string()),
        },
    };
    let (user_semitones, user_median_label) = match pitch_analysis {
        Some(analysis) => (
            analysis.user_semitones.as_slice(),
            format!("Your Take ({:.0} Hz median)", analysis.user_median_hz),
        ),
        None => (empty_slice, "Your Take (pending)".to_string()),
    };

    let mut content = column![
        row![
            text("Pitch Contour Overlay (Semitones)")
                .size(17)
                .color(colors.text_primary),
            space().width(Length::Fill),
            container(text(ref_median_label).size(12).color(colors.sage))
                .padding([4, 10])
                .style(theme::badge_style(colors.sage_soft, colors.sage)),
            container(text(user_median_label).size(12).color(colors.coral_text))
                .padding([4, 10])
                .style(theme::badge_style(colors.coral_soft, colors.coral)),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        canvas(PitchContourProgram {
            reference_semitones: ref_semitones,
            user_semitones,
            resolved_theme,
        })
        .width(Length::Fill)
        .height(210),
    ]
    .spacing(12);

    if let Some(analysis) = pitch_analysis {
        content = content.push(
            row![
                metric_pill(
                    "Pitch Error",
                    format!("{:.1}%", analysis.pitch_error_percent),
                    colors.coral_text,
                    colors,
                ),
                metric_pill(
                    "Pitch Accuracy",
                    format!("{:.1}%", analysis.accuracy_percent()),
                    colors.sage,
                    colors,
                ),
                metric_pill(
                    "DTW Distance",
                    format!("{:.2} st", analysis.dtw_distance),
                    colors.text_primary,
                    colors,
                ),
            ]
            .spacing(10),
        );
    }

    container(content)
        .width(Length::Fill)
        .padding(18)
        .style(theme::card_style)
        .into()
}

fn render_shadowing_panel<'a>(
    state: &'a PracticeState,
    card: &'a AnkiCard,
    resolved_theme: ResolvedTheme,
    colors: &'static ThemeTokens,
) -> Element<'a, Message> {
    let mut controls_col = column![
        row![
            text("Combined Shadowing")
                .size(17)
                .color(colors.text_primary),
            space().width(Length::Fill),
            toggler(state.shadow_auto_record)
                .label("Auto-record after countdown")
                .on_toggle(Message::PracticeToggleShadowAutoRecord),
            button(text("Shadow Now (Play → 3-2-1 → Record)").size(13))
                .padding([8, 14])
                .style(theme::primary_button_style)
                .on_press(Message::PracticeStartShadowFlow),
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    ]
    .spacing(12);

    if let Some(cd) = state.countdown {
        controls_col = controls_col.push(
            container(
                column![
                    row![
                        text(format!("Get ready to shadow… {}", cd.beats_remaining))
                            .size(18)
                            .color(colors.coral_text),
                        space().width(Length::Fill),
                        button(text("Cancel (Esc)").size(12))
                            .padding([4, 10])
                            .style(theme::secondary_button_style)
                            .on_press(Message::PracticeKeyShortcut(
                                PracticeShortcutAction::CancelActive
                            )),
                    ]
                    .align_y(Alignment::Center),
                    progress_bar(0.0..=1.0, cd.beat_progress),
                ]
                .spacing(8),
            )
            .width(Length::Fill)
            .padding(14)
            .style(theme::muted_card_style),
        );
    } else if state.awaiting_shadow_countdown {
        controls_col = controls_col.push(
            container(
                text("Playing reference clip — countdown begins automatically when audio finishes...")
                    .size(13)
                    .color(colors.coral_text),
            )
            .width(Length::Fill)
            .padding([10, 14])
            .style(theme::muted_card_style),
        );
    }

    let morae = segment_kana_into_morae(&card.kana);
    let attempts = state
        .last_evaluation
        .as_ref()
        .and_then(|eval| eval.mora_evaluation.as_ref())
        .map(|m| m.attempts.as_slice());

    let pitch_view = render_pitch_panel(state, resolved_theme, colors);
    let mora_card = container(
        column![
            row![
                text("Shadowing Mora Clarity")
                    .size(16)
                    .color(colors.text_primary),
                space().width(Length::Fill),
                text(
                    state
                        .last_evaluation
                        .as_ref()
                        .and_then(|e| e.mora_evaluation.as_ref())
                        .map_or_else(
                            || format!("{} morae", morae.len()),
                            |m| format!("Mora {:.0}% · 50/50 Combined", m.mora_accuracy_percent)
                        )
                )
                .size(13)
                .color(colors.sage),
            ]
            .align_y(Alignment::Center),
            render_mora_chips_row(&morae, attempts, state.selected_mora_index, colors),
        ]
        .spacing(10),
    )
    .width(Length::Fill)
    .padding(16)
    .style(theme::card_style);

    column![
        container(controls_col)
            .width(Length::Fill)
            .padding(16)
            .style(theme::card_style),
        pitch_view,
        mora_card,
    ]
    .spacing(14)
    .width(Length::Fill)
    .into()
}

fn render_capture_bar<'a>(
    state: &'a PracticeState,
    settings: &'a AppSettings,
    mic_state: PracticeMicViewState,
    resolved_theme: ResolvedTheme,
    colors: &'static ThemeTokens,
) -> Element<'a, Message> {
    let mut body = column![].spacing(12);

    if mic_state.is_recording {
        body = body.push(
            column![
                row![
                    text(format!(
                        "Recording live · {:.1}s",
                        mic_state.elapsed_ms as f32 / 1000.0
                    ))
                    .size(13)
                    .color(colors.danger),
                    space().width(Length::Fill),
                    text(format!("{:.1} dBFS", mic_state.mic_level.db_fs))
                        .size(12)
                        .color(colors.text_muted),
                ]
                .align_y(Alignment::Center),
                canvas(MicMeterProgram {
                    meter_ratio: mic_state.mic_level.meter_ratio,
                    vad_threshold_ratio: crate::audio::capture::vad_threshold_to_meter_ratio(
                        settings.vad_rms_threshold,
                    ),
                    is_speech: mic_state.mic_level.is_speech,
                    is_clipping: mic_state.mic_level.is_clipping,
                    is_active: true,
                    resolved_theme,
                })
                .width(Length::Fill)
                .height(18),
            ]
            .spacing(6),
        );
    }

    let play_ref_label = if state.is_playing_reference {
        "Playing Reference..."
    } else {
        "Play Reference (Space)"
    };
    let mut play_ref_btn = button(text(play_ref_label).size(14))
        .padding([10, 16])
        .style(theme::secondary_button_style);
    if !state.is_playing_reference && !mic_state.is_recording {
        play_ref_btn = play_ref_btn.on_press(Message::PracticePlayReference);
    }

    let record_btn = if mic_state.is_recording {
        button(text("Stop Recording (R)").size(14))
            .padding([10, 18])
            .style(theme::recording_button_style)
            .on_press(Message::PracticeToggleRecording)
    } else {
        button(text("Record Take (R)").size(14))
            .padding([10, 18])
            .style(theme::primary_button_style)
            .on_press(Message::PracticeToggleRecording)
    };

    let take_label = if state.is_playing_user_take {
        "Playing Take..."
    } else {
        "Play Your Take (T)"
    };
    let mut play_take_btn = button(text(take_label).size(14))
        .padding([10, 16])
        .style(theme::secondary_button_style);
    if state.last_take.is_some() && !state.is_playing_user_take && !mic_state.is_recording {
        play_take_btn = play_take_btn.on_press(Message::PracticePlayUserTake);
    }

    let mut skip_btn = button(text("Skip Card").size(13))
        .padding([10, 14])
        .style(theme::secondary_button_style);
    if !state.is_committing {
        skip_btn = skip_btn.on_press(Message::PracticeSkipCard);
    }

    let mut status_text = String::new();
    if state.is_scoring {
        status_text = "Analyzing mora clarity and pitch contour...".to_string();
    } else if let Some(take) = &state.last_take {
        status_text = format!(
            "Last take: {} ms VAD-trimmed · {}",
            take.trimmed_audio.duration_ms(),
            take.stop_reason.label()
        );
    }

    body = body.push(
        row![
            play_ref_btn,
            record_btn,
            play_take_btn,
            space().width(Length::Fill),
            skip_btn,
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    );

    if !status_text.is_empty() {
        body = body.push(text(status_text).size(12).color(colors.text_muted));
    }

    container(body)
        .width(Length::Fill)
        .padding(16)
        .style(theme::card_style)
        .into()
}

fn render_rating_bar<'a>(
    state: &'a PracticeState,
    colors: &'static ThemeTokens,
) -> Element<'a, Message> {
    let recommended = state
        .last_evaluation
        .as_ref()
        .map(|eval| eval.breakdown.recommended_rating);

    let mut body = column![].spacing(12);

    if let Some(eval) = &state.last_evaluation {
        let copy = encouraging_micro_copy(eval.breakdown.recommended_rating);
        let mut accept_btn = button(
            text(format!(
                "Accept {} (Enter)",
                eval.breakdown.recommended_rating.label()
            ))
            .size(13),
        )
        .padding([8, 14])
        .style(theme::primary_button_style);
        if !state.is_committing {
            accept_btn = accept_btn.on_press(Message::PracticeSubmitRating {
                rating: eval.breakdown.recommended_rating,
                is_manual: false,
            });
        }

        body = body.push(
            row![
                column![
                    text(format!(
                        "Combined Score: {:.1}% · Recommended: {}",
                        eval.breakdown.combined_score_percent,
                        eval.breakdown.recommended_rating.label()
                    ))
                    .size(15)
                    .color(colors.text_primary),
                    text(copy).size(12).color(colors.sage),
                ]
                .spacing(2),
                space().width(Length::Fill),
                accept_btn,
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        );
    } else {
        body = body.push(
            text("Record a take for automated scoring, or press 1–4 anytime to rate manually.")
                .size(13)
                .color(colors.text_muted),
        );
    }

    let mut rating_row = row![].spacing(10).align_y(Alignment::Center);
    for (idx, rating) in FsrsRating::ALL.into_iter().enumerate() {
        let is_rec = recommended == Some(rating);
        let interval_suffix = state
            .preview_intervals_days
            .and_then(|intervals| intervals.get(idx).copied())
            .map(|days| format!(" · {}", format_interval_days(rating, days)))
            .unwrap_or_default();
        let label = format!(
            "[{}] {}{}",
            rating.shortcut_key(),
            rating.label(),
            interval_suffix
        );
        let mut btn = button(text(label).size(13))
            .width(Length::Fill)
            .padding([10, 12])
            .style(theme::rating_button_style(rating, is_rec));
        if !state.is_committing {
            btn = btn.on_press(Message::PracticeSubmitRating {
                rating,
                is_manual: recommended != Some(rating),
            });
        }
        rating_row = rating_row.push(btn);
    }

    body = body.push(rating_row);

    container(body)
        .width(Length::Fill)
        .padding(16)
        .style(theme::card_style)
        .into()
}

fn render_session_complete<'a>(
    state: &'a PracticeState,
    streak: &'a DailyStreak,
    cap_reached: bool,
    colors: &'static ThemeTokens,
) -> Element<'a, Message> {
    let heading = if cap_reached {
        "Daily Goal Complete!"
    } else if state.summary.cards_completed == 0 {
        "All Due Cards Caught Up!"
    } else {
        "Due Queue Complete!"
    };

    let subtitle = if cap_reached {
        format!(
            "You hit today's {} target — great consistency! Keep your voice fresh or take a well-earned break.",
            streak.daily_goal.label()
        )
    } else if state.summary.cards_completed == 0 {
        "No FSRS pronunciation cards are due right now. You can practice all active deck cards in Extra Study."
            .to_string()
    } else {
        "Every due pronunciation card in this session has been reviewed and scheduled with FSRS."
            .to_string()
    };

    let avg_score = state
        .summary
        .average_score_percent
        .map_or_else(|| "—".to_string(), |v| format!("{v:.1}%"));
    let avg_mora = state
        .summary
        .average_mora_accuracy_percent
        .map_or_else(|| "—".to_string(), |v| format!("{v:.1}%"));
    let avg_pitch = state
        .summary
        .average_pitch_error_percent
        .map_or_else(|| "—".to_string(), |v| format!("{v:.1}%"));

    let stats_row = row![
        metric_pill(
            "Cards Practiced",
            state.summary.cards_completed.to_string(),
            colors.sage,
            colors,
        ),
        metric_pill(
            "Session Time",
            format_elapsed_mm_ss(state.summary.session_duration_ms),
            colors.text_primary,
            colors,
        ),
        metric_pill("Avg Score", avg_score, colors.coral_text, colors),
        metric_pill("Avg Mora", avg_mora, colors.sage, colors),
        metric_pill("Avg Pitch Error", avg_pitch, colors.amber, colors),
    ]
    .spacing(10);

    container(
        column![
            text(heading).size(24).color(colors.sage),
            text(subtitle).size(14).color(colors.text_muted),
            stats_row,
            text(format!(
                "Current streak: {} day(s) · Manual overrides: {}",
                streak.current_streak_days, state.summary.override_count
            ))
            .size(13)
            .color(colors.text_primary),
            row![
                button(text("Continue Extra Practice").size(14))
                    .padding([10, 16])
                    .style(theme::primary_button_style)
                    .on_press(Message::PracticeContinueExtra),
                button(text("Back to Practice Lobby").size(14))
                    .padding([10, 16])
                    .style(theme::secondary_button_style)
                    .on_press(Message::PracticeEndSession),
                button(text("View Dashboard").size(14))
                    .padding([10, 16])
                    .style(theme::secondary_button_style)
                    .on_press(Message::SelectTab(NavTab::Dashboard)),
            ]
            .spacing(10),
        ]
        .spacing(16),
    )
    .width(Length::Fill)
    .padding(24)
    .style(theme::card_style)
    .into()
}

fn metric_pill<'a>(
    label: &'a str,
    value: String,
    value_color: Color,
    colors: &'static ThemeTokens,
) -> Element<'a, Message> {
    container(
        column![
            text(label).size(12).color(colors.text_muted),
            text(value).size(18).color(value_color),
        ]
        .spacing(4),
    )
    .width(Length::Fill)
    .padding(12)
    .style(theme::muted_card_style)
    .into()
}

fn shortcut_chip<'a>(
    key: &'a str,
    action: &'a str,
    colors: &'static ThemeTokens,
) -> Element<'a, Message> {
    container(
        column![
            text(key).size(14).color(colors.coral_text),
            text(action).size(12).color(colors.text_muted),
        ]
        .spacing(4),
    )
    .width(Length::Fill)
    .padding(12)
    .style(theme::muted_card_style)
    .into()
}

fn encouraging_micro_copy(rating: FsrsRating) -> &'static str {
    match rating {
        FsrsRating::Easy => "Crisp mora timing and natural pitch contour!",
        FsrsRating::Good => "Solid pronunciation — clear and well-paced.",
        FsrsRating::Hard => "Close! Listen once more at 0.75x and watch the pitch curve.",
        FsrsRating::Again => "Keep going — we'll bring this card back once more this session.",
    }
}

fn format_interval_days(rating: FsrsRating, days: u32) -> String {
    if rating == FsrsRating::Again && days == 0 {
        "10m".to_string()
    } else {
        format!("{days}d")
    }
}

fn format_elapsed_mm_ss(elapsed_ms: u64) -> String {
    let total_secs = elapsed_ms / 1000;
    let mins = total_secs / 60;
    let secs = total_secs % 60;
    format!("{mins:02}:{secs:02}")
}

fn format_daily_goal_progress(streak: &DailyStreak, effective_minutes: u32) -> String {
    match streak.daily_goal {
        DailyGoal::Cards(goal) => format!("{} / {} cards", streak.cards_practiced_today, goal),
        DailyGoal::Minutes(goal) => format!("{effective_minutes} / {goal} min"),
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use iced::keyboard::key::{Code, Physical};

    use super::*;
    use crate::models::{CombinedScoreBreakdown, MoraEvaluation};

    fn sample_card(card_id: i64, sentence_audio: Option<&str>) -> AnkiCard {
        AnkiCard {
            card_id,
            note_id: card_id * 10,
            deck_name: "Core 2k".to_string(),
            word: "橋".to_string(),
            kana: "はし".to_string(),
            sentence: Some("橋を渡る。".to_string()),
            word_audio: "hashi.mp3".to_string(),
            sentence_audio: sentence_audio.map(str::to_string),
            anki_queue: AnkiCard::LEARNED_QUEUE,
            audio_issues: Vec::new(),
            last_pitch_error_percent: None,
        }
    }

    #[test]
    fn keyboard_shortcut_mapper_handles_keys_ime_fallbacks_and_modifiers() {
        let none = Modifiers::empty();
        assert_eq!(
            map_key_to_shortcut(&Key::Named(Named::Space), Physical::Code(Code::Space), none),
            Some(PracticeShortcutAction::PlayReference)
        );
        assert_eq!(
            map_key_to_shortcut(&Key::Named(Named::Enter), Physical::Code(Code::Enter), none),
            Some(PracticeShortcutAction::AcceptRecommended)
        );
        assert_eq!(
            map_key_to_shortcut(&Key::Named(Named::Escape), Physical::Code(Code::Escape), none),
            Some(PracticeShortcutAction::CancelActive)
        );
        assert_eq!(
            map_key_to_shortcut(
                &Key::Character("R".into()),
                Physical::Code(Code::KeyR),
                Modifiers::SHIFT
            ),
            Some(PracticeShortcutAction::ToggleRecord)
        );
        assert_eq!(
            map_key_to_shortcut(
                &Key::Character("あ".into()),
                Physical::Code(Code::KeyT),
                none
            ),
            Some(PracticeShortcutAction::PlayUserTake)
        );
        assert_eq!(
            map_key_to_shortcut(
                &Key::Character("b".into()),
                Physical::Code(Code::KeyB),
                none
            ),
            Some(PracticeShortcutAction::ToggleBlindMode)
        );
        assert_eq!(
            map_key_to_shortcut(
                &Key::Character("s".into()),
                Physical::Code(Code::KeyS),
                none
            ),
            Some(PracticeShortcutAction::TogglePlaybackSpeed)
        );
        assert_eq!(
            map_key_to_shortcut(
                &Key::Character("1".into()),
                Physical::Code(Code::Digit1),
                none
            ),
            Some(PracticeShortcutAction::RateCard(FsrsRating::Again))
        );
        assert_eq!(
            map_key_to_shortcut(
                &Key::Character("4".into()),
                Physical::Code(Code::Digit4),
                none
            ),
            Some(PracticeShortcutAction::RateCard(FsrsRating::Easy))
        );
        assert_eq!(
            map_key_to_shortcut(
                &Key::Character("r".into()),
                Physical::Code(Code::KeyR),
                Modifiers::CTRL
            ),
            None
        );
    }

    #[test]
    fn semitone_bounds_and_contour_point_mapping_split_unvoiced_gaps() {
        assert_eq!(compute_semitone_bounds(&[], &[]), (-6.0, 6.0));

        let reference = vec![Some(-2.0), Some(4.0), None, Some(8.2)];
        let user = vec![Some(-8.3), None, Some(1.5)];
        let (min_st, max_st) = compute_semitone_bounds(&reference, &user);
        assert!(min_st <= -9.0);
        assert!(max_st >= 9.0);

        let plot = Rectangle {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 80.0,
        };
        let segments = map_contour_points(&reference, plot, -10.0, 10.0);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].len(), 2);
        assert_eq!(segments[1].len(), 1);
        assert!((segments[0][0].x - 10.0).abs() < 1e-4);
        assert!((segments[1][0].x - 110.0).abs() < 1e-4);
        assert!(segments[0][1].y < segments[0][0].y);
    }

    #[test]
    fn practice_state_handles_blind_mode_clip_selection_countdown_and_again_requeue() {
        let mut state = PracticeState::default();
        let card_with_sentence = sample_card(1, Some("sent.mp3"));
        let card_word_only = sample_card(2, None);

        state.begin_session(
            vec![card_with_sentence, card_word_only],
            PracticeMode::CombinedShadowing,
            true,
            false,
            Some(2),
        );
        assert_eq!(state.phase, PracticePhase::ActiveCard);
        assert_eq!(state.target_clip, PracticeTargetClip::Sentence);
        assert!(!state.text_revealed);

        let t0 = Instant::now();
        state.start_countdown(t0);
        assert!(!state.advance_countdown(t0 + std::time::Duration::from_millis(600)));
        assert_eq!(
            state.countdown.map(|c| c.beats_remaining),
            Some(2)
        );
        assert!(state.advance_countdown(t0 + std::time::Duration::from_millis(1600)));
        assert!(state.countdown.is_none());

        let eval = PracticeEvaluationResult {
            card_id: 1,
            mode: PracticeMode::CombinedShadowing,
            mora_evaluation: Some(MoraEvaluation {
                attempts: Vec::new(),
                mora_accuracy_percent: 60.0,
                scorer_name: "test".to_string(),
                accepted_allophone_notes: Vec::new(),
            }),
            pitch_analysis: None,
            breakdown: CombinedScoreBreakdown {
                mode: PracticeMode::CombinedShadowing,
                mora_accuracy_percent: Some(60.0),
                pitch_accuracy_percent: Some(80.0),
                pitch_error_percent: Some(20.0),
                combined_score_percent: 70.0,
                recommended_rating: FsrsRating::Hard,
            },
            duration_ms: 4000,
            evaluated_at: Utc::now(),
        };

        state.session_elapsed_ms = 125_000;
        assert_eq!(state.take_newly_elapsed_minutes(), 2);
        assert_eq!(state.take_newly_elapsed_minutes(), 0);

        state.record_committed_card(Some(&eval), true, FsrsRating::Again);
        assert_eq!(state.summary.cards_completed, 1);
        assert_eq!(state.summary.override_count, 1);
        assert_eq!(state.queue.len(), 3);

        state.current_index = 2;
        state.record_committed_card(Some(&eval), false, FsrsRating::Again);
        assert_eq!(state.queue.len(), 3);
    }
}

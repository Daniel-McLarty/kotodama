// SPDX-License-Identifier: MPL-2.0

use anyhow::{Result, bail};
use chrono::{DateTime, Duration, Utc};
use fsrs::{FSRS, MemoryState};

use crate::models::{
    AnkiCard, CombinedScoreBreakdown, DailyGoal, DailyStreak, FsrsCardState, FsrsRating,
    FsrsSchedulingOutcome, MoraAttempt, ReviewLogEntry, ScoreThresholds,
};
use crate::storage::db::KotodamaDb;

/// Default target recall probability used by Kotodama's independent FSRS scheduler.
#[allow(dead_code)]
pub const DEFAULT_DESIRED_RETENTION: f32 = 0.90;

/// Independent FSRS scheduler translating pronunciation scores into local spaced-repetition intervals.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct FsrsScheduler {
    engine: FSRS,
    desired_retention: f32,
}

#[allow(dead_code)]
impl FsrsScheduler {
    /// Creates an [`FsrsScheduler`] configured with `desired_retention` in `0.70..=0.99`.
    pub fn new(desired_retention: f32) -> Result<Self> {
        if !desired_retention.is_finite() || !(0.70..=0.99).contains(&desired_retention) {
            bail!("Desired FSRS retention must be finite and within 0.70..=0.99");
        }
        Ok(Self {
            engine: FSRS::default(),
            desired_retention,
        })
    }

    /// Creates an [`FsrsScheduler`] with [`DEFAULT_DESIRED_RETENTION`] (`0.90`).
    pub fn default_scheduler() -> Result<Self> {
        Self::new(DEFAULT_DESIRED_RETENTION)
    }

    /// Computes prospective review intervals in days for `[Again, Hard, Good, Easy]`.
    pub fn preview_next_intervals(
        &self,
        current: Option<&FsrsCardState>,
        now: DateTime<Utc>,
    ) -> Result<[u32; 4]> {
        let memory = current.and_then(to_memory_state);
        let elapsed_days = current.map_or(0, |state| compute_elapsed_days(state, now));
        let next_states = self
            .engine
            .next_states(memory, self.desired_retention, elapsed_days)
            .map_err(|err| anyhow::anyhow!("FSRS interval preview failed: {err}"))?;

        Ok([
            scheduled_days_for_item(FsrsRating::Again, next_states.again.interval),
            scheduled_days_for_item(FsrsRating::Hard, next_states.hard.interval),
            scheduled_days_for_item(FsrsRating::Good, next_states.good.interval),
            scheduled_days_for_item(FsrsRating::Easy, next_states.easy.interval),
        ])
    }

    /// Advances a card's FSRS memory state using `score_percent`, `thresholds`, and optional `manual_override`.
    pub fn schedule_card(
        &self,
        card_id: i64,
        current: Option<&FsrsCardState>,
        score_percent: f32,
        thresholds: &ScoreThresholds,
        manual_override: Option<FsrsRating>,
        now: DateTime<Utc>,
    ) -> Result<FsrsSchedulingOutcome> {
        if !score_percent.is_finite() || !(0.0..=100.0).contains(&score_percent) {
            bail!("Practice score percentage must be finite and within 0.0..=100.0");
        }
        thresholds.validate()?;

        let previous_state = current
            .cloned()
            .unwrap_or_else(|| FsrsCardState::new_unseen(card_id, now));
        let memory = to_memory_state(&previous_state);
        let elapsed_days = compute_elapsed_days(&previous_state, now);

        let next_states = self
            .engine
            .next_states(memory, self.desired_retention, elapsed_days)
            .map_err(|err| anyhow::anyhow!("FSRS state transition failed: {err}"))?;

        let preview_intervals_days = [
            scheduled_days_for_item(FsrsRating::Again, next_states.again.interval),
            scheduled_days_for_item(FsrsRating::Hard, next_states.hard.interval),
            scheduled_days_for_item(FsrsRating::Good, next_states.good.interval),
            scheduled_days_for_item(FsrsRating::Easy, next_states.easy.interval),
        ];

        let recommended_rating = thresholds.rate_score(score_percent);
        let applied_rating = manual_override.unwrap_or(recommended_rating);
        let was_manual_override =
            manual_override.is_some_and(|override_rating| override_rating != recommended_rating);

        let chosen = match applied_rating {
            FsrsRating::Again => next_states.again,
            FsrsRating::Hard => next_states.hard,
            FsrsRating::Good => next_states.good,
            FsrsRating::Easy => next_states.easy,
        };

        let scheduled_days = scheduled_days_for_item(applied_rating, chosen.interval);
        let due_at = if applied_rating == FsrsRating::Again && chosen.interval < 1.0 {
            now + Duration::minutes(10)
        } else {
            now + Duration::days(i64::from(scheduled_days))
        };

        let next_lapses = if applied_rating == FsrsRating::Again && previous_state.reps > 0 {
            previous_state.lapses.saturating_add(1)
        } else {
            previous_state.lapses
        };

        let next_state = FsrsCardState {
            card_id,
            stability: f64::from(chosen.memory.stability),
            stability_fast: f64::from(chosen.memory.stability_fast),
            difficulty: f64::from(chosen.memory.difficulty),
            elapsed_days,
            scheduled_days,
            reps: previous_state.reps.saturating_add(1),
            lapses: next_lapses,
            due_at,
            last_reviewed_at: Some(now),
            last_rating: Some(applied_rating),
        };

        Ok(FsrsSchedulingOutcome {
            previous_state,
            next_state,
            recommended_rating,
            applied_rating,
            was_manual_override,
            preview_intervals_days,
        })
    }

    /// Evaluates FSRS scheduling for `card` and commits review log, mora stats, FSRS state, and streak atomically.
    #[allow(clippy::too_many_arguments)]
    pub fn record_review_and_update_fsrs(
        &self,
        db: &mut KotodamaDb,
        card: &AnkiCard,
        breakdown: &CombinedScoreBreakdown,
        mora_attempts: &[MoraAttempt],
        thresholds: &ScoreThresholds,
        manual_override: Option<FsrsRating>,
        dtw_distance: Option<f32>,
        duration_ms: u64,
        daily_goal: DailyGoal,
        now: DateTime<Utc>,
    ) -> Result<(i64, FsrsSchedulingOutcome, DailyStreak)> {
        let streak_minutes_to_add = (duration_ms / 60_000) as u32;
        self.record_review_with_streak_minutes(
            db,
            card,
            breakdown,
            mora_attempts,
            thresholds,
            manual_override,
            dtw_distance,
            duration_ms,
            streak_minutes_to_add,
            daily_goal,
            now,
        )
    }

    /// Evaluates FSRS scheduling for `card`, recording exact `duration_ms` while adding `streak_minutes_to_add` to daily streak minutes.
    #[allow(clippy::too_many_arguments)]
    pub fn record_review_with_streak_minutes(
        &self,
        db: &mut KotodamaDb,
        card: &AnkiCard,
        breakdown: &CombinedScoreBreakdown,
        mora_attempts: &[MoraAttempt],
        thresholds: &ScoreThresholds,
        manual_override: Option<FsrsRating>,
        dtw_distance: Option<f32>,
        duration_ms: u64,
        streak_minutes_to_add: u32,
        daily_goal: DailyGoal,
        now: DateTime<Utc>,
    ) -> Result<(i64, FsrsSchedulingOutcome, DailyStreak)> {
        let existing = db.get_fsrs_state(card.card_id)?;
        let outcome = self.schedule_card(
            card.card_id,
            existing.as_ref(),
            breakdown.combined_score_percent,
            thresholds,
            manual_override,
            now,
        )?;

        let review = ReviewLogEntry {
            id: None,
            card_id: card.card_id,
            deck_name: card.deck_name.clone(),
            word: card.word.clone(),
            kana: card.kana.clone(),
            practice_mode: breakdown.mode,
            mora_accuracy_percent: breakdown.mora_accuracy_percent,
            pitch_error_percent: breakdown.pitch_error_percent,
            dtw_distance,
            combined_score_percent: breakdown.combined_score_percent,
            recommended_rating: outcome.recommended_rating,
            applied_rating: outcome.applied_rating,
            was_manual_override: outcome.was_manual_override,
            duration_ms,
            reviewed_at: now,
        };

        let (review_id, streak) = db.commit_review_transaction(
            &review,
            mora_attempts,
            &outcome.next_state,
            now.date_naive(),
            streak_minutes_to_add,
            daily_goal,
        )?;

        Ok((review_id, outcome, streak))
    }
}

fn to_memory_state(state: &FsrsCardState) -> Option<MemoryState> {
    if state.reps == 0 || state.stability <= 0.0 || state.difficulty <= 0.0 {
        return None;
    }
    let stability = state.stability as f32;
    let stability_fast = if state.stability_fast > 0.0 {
        state.stability_fast as f32
    } else {
        stability
    };
    Some(MemoryState {
        stability,
        stability_fast,
        difficulty: state.difficulty as f32,
    })
}

fn compute_elapsed_days(state: &FsrsCardState, now: DateTime<Utc>) -> u32 {
    if state.reps == 0 {
        return 0;
    }
    state
        .last_reviewed_at
        .map(|prev| now.signed_duration_since(prev).num_days().max(0) as u32)
        .unwrap_or(0)
}

fn scheduled_days_for_item(rating: FsrsRating, interval: f32) -> u32 {
    if !interval.is_finite() || interval <= 0.0 {
        return usize::from(rating != FsrsRating::Again) as u32;
    }
    if rating == FsrsRating::Again && interval < 1.0 {
        0
    } else {
        interval.round().max(1.0) as u32
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;

    use chrono::TimeZone;

    use super::*;
    use crate::models::{DecodedAudio, PitchExtractionConfig, PracticeMode};
    use crate::scoring::mora::{HeuristicAcousticMoraScorer, MoraScorer};
    use crate::scoring::pitch::{combine_practice_score, score_pitch};

    fn ts(year: i32, month: u32, day: u32, hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, hour, 0, 0)
            .single()
            .unwrap_or_else(Utc::now)
    }

    fn sine_audio(freq_hz: f32, duration_ms: u64) -> DecodedAudio {
        let sample_rate = 16_000_u32;
        let len = (u64::from(sample_rate) * duration_ms / 1000) as usize;
        let samples = (0..len)
            .map(|i| (TAU * freq_hz * i as f32 / sample_rate as f32).sin() * 0.45)
            .collect();
        DecodedAudio {
            samples,
            sample_rate,
            channels: 1,
        }
    }

    #[test]
    fn score_mapping_and_manual_override_preserve_recommended_rating() -> Result<()> {
        let scheduler = FsrsScheduler::default_scheduler()?;
        let thresholds = ScoreThresholds::strict();
        let now = ts(2026, 11, 26, 10);

        let auto_hard = scheduler.schedule_card(1, None, 65.0, &thresholds, None, now)?;
        assert_eq!(auto_hard.recommended_rating, FsrsRating::Hard);
        assert_eq!(auto_hard.applied_rating, FsrsRating::Hard);
        assert!(!auto_hard.was_manual_override);
        assert_eq!(auto_hard.next_state.reps, 1);

        let overridden = scheduler.schedule_card(
            2,
            None,
            95.0,
            &thresholds,
            Some(FsrsRating::Again),
            now,
        )?;
        assert_eq!(overridden.recommended_rating, FsrsRating::Easy);
        assert_eq!(overridden.applied_rating, FsrsRating::Again);
        assert!(overridden.was_manual_override);
        assert_eq!(overridden.next_state.lapses, 0);
        assert!(overridden.next_state.due_at >= now);
        Ok(())
    }

    #[test]
    fn multi_review_progression_grows_intervals_and_tracks_lapses() -> Result<()> {
        let scheduler = FsrsScheduler::default_scheduler()?;
        let thresholds = ScoreThresholds::default();
        let t0 = ts(2026, 11, 20, 9);

        let previews = scheduler.preview_next_intervals(None, t0)?;
        assert!(previews[3] >= previews[2]);

        let first = scheduler.schedule_card(10, None, 80.0, &thresholds, None, t0)?;
        assert_eq!(first.applied_rating, FsrsRating::Good);
        assert!(first.next_state.stability > 0.0);

        let t1 = t0 + Duration::days(i64::from(first.next_state.scheduled_days.max(1)));
        let second = scheduler.schedule_card(
            10,
            Some(&first.next_state),
            96.0,
            &thresholds,
            None,
            t1,
        )?;
        assert_eq!(second.applied_rating, FsrsRating::Easy);
        assert_eq!(second.next_state.reps, 2);
        assert!(second.next_state.scheduled_days >= first.next_state.scheduled_days);

        let t2 = t1 + Duration::days(i64::from(second.next_state.scheduled_days.max(1)));
        let lapsed = scheduler.schedule_card(
            10,
            Some(&second.next_state),
            20.0,
            &thresholds,
            None,
            t2,
        )?;
        assert_eq!(lapsed.applied_rating, FsrsRating::Again);
        assert_eq!(lapsed.next_state.reps, 3);
        assert_eq!(lapsed.next_state.lapses, 1);
        Ok(())
    }

    #[test]
    fn end_to_end_scoring_updates_local_db_and_fsrs_without_anki_mutation() -> Result<()> {
        let mut db = KotodamaDb::open_in_memory()?;
        let scheduler = FsrsScheduler::default_scheduler()?;
        let thresholds = ScoreThresholds::default();
        let now = ts(2026, 11, 26, 14);

        let card = AnkiCard {
            card_id: 501,
            note_id: 601,
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

        let ref_audio = sine_audio(180.0, 500);
        let user_audio = sine_audio(220.0, 500);

        let mora_eval = HeuristicAcousticMoraScorer.score_morae(&crate::models::MoraScoreRequest {
            card_id: card.card_id,
            target_kana_sequence: card.kana.clone(),
            user_audio: user_audio.clone(),
            reference_audio: Some(ref_audio.clone()),
            recorded_at: now,
        })?;

        let pitch = score_pitch(&ref_audio, &user_audio, &PitchExtractionConfig::default())?;
        let breakdown = combine_practice_score(
            PracticeMode::CombinedShadowing,
            Some(mora_eval.mora_accuracy_percent),
            Some(pitch.pitch_error_percent),
            &thresholds,
        )?;

        let (review_id, outcome, streak) = scheduler.record_review_and_update_fsrs(
            &mut db,
            &card,
            &breakdown,
            &mora_eval.attempts,
            &thresholds,
            None,
            Some(pitch.dtw_distance),
            62_000,
            DailyGoal::Cards(10),
            now,
        )?;

        assert!(review_id > 0);
        assert_eq!(outcome.next_state.reps, 1);
        assert_eq!(streak.cards_practiced_today, 1);
        assert_eq!(streak.minutes_practiced_today, 1);

        let persisted_fsrs = db
            .get_fsrs_state(card.card_id)?
            .expect("FSRS state should be saved");
        assert_eq!(persisted_fsrs, outcome.next_state);
        assert_eq!(db.list_review_logs(None)?.len(), 1);
        assert!(db.get_kana_stats("み")?.is_some());
        assert!(db.get_kana_stats("ず")?.is_some());
        Ok(())
    }

    #[test]
    fn record_review_with_streak_minutes_separates_card_duration_from_accumulated_minutes() -> Result<()> {
        let mut db = KotodamaDb::open_in_memory()?;
        let scheduler = FsrsScheduler::default_scheduler()?;
        let thresholds = ScoreThresholds::default();
        let now = ts(2026, 11, 26, 16);

        let card = AnkiCard {
            card_id: 777,
            note_id: 888,
            deck_name: "Core 2k".to_string(),
            word: "猫".to_string(),
            kana: "ねこ".to_string(),
            sentence: None,
            word_audio: "neko.mp3".to_string(),
            sentence_audio: None,
            anki_queue: AnkiCard::LEARNED_QUEUE,
            audio_issues: Vec::new(),
            last_pitch_error_percent: None,
        };
        let breakdown = combine_practice_score(
            PracticeMode::MoraPurity,
            Some(92.0),
            None,
            &thresholds,
        )?;

        let (review_id, _outcome, streak) = scheduler.record_review_with_streak_minutes(
            &mut db,
            &card,
            &breakdown,
            &[],
            &thresholds,
            None,
            None,
            12_500,
            2,
            DailyGoal::Minutes(5),
            now,
        )?;

        assert_eq!(streak.minutes_practiced_today, 2);
        let logs = db.list_review_logs(None)?;
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].id, Some(review_id));
        assert_eq!(logs[0].duration_ms, 12_500);
        Ok(())
    }
}

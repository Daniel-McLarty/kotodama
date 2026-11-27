// SPDX-License-Identifier: MPL-2.0

use anyhow::{Result, bail};

use crate::audio::dsp::analyze_pitch_contour;
use crate::models::{
    CombinedScoreBreakdown, DecodedAudio, PitchAnalysis, PitchErrorLogEntry, PitchExtractionConfig,
    PracticeMode, ScoreThresholds,
};

/// Evaluates user pitch contour against reference audio using YIN `F0` and semitone DTW.
#[allow(dead_code)]
pub fn score_pitch(
    reference: &DecodedAudio,
    user: &DecodedAudio,
    config: &PitchExtractionConfig,
) -> Result<PitchAnalysis> {
    analyze_pitch_contour(reference, user, config)
}

/// Returns pitch contour accuracy (`100.0 - pitch_error_percent`) clamped to `0.0..=100.0`.
#[allow(dead_code)]
pub fn pitch_accuracy_percent(analysis: &PitchAnalysis) -> f32 {
    analysis.accuracy_percent()
}

/// Computes the mean pitch accent error percentage across `logs`, or `None` when empty.
#[allow(dead_code)]
pub fn average_pitch_error_percent(logs: &[PitchErrorLogEntry]) -> Option<f32> {
    if logs.is_empty() {
        return None;
    }
    let sum: f64 = logs
        .iter()
        .map(|entry| f64::from(entry.pitch_error_percent))
        .sum();
    Some((sum / logs.len() as f64) as f32)
}

/// Combines mora clarity and pitch contour metrics into a weighted score and FSRS rating.
#[allow(dead_code)]
pub fn combine_practice_score(
    mode: PracticeMode,
    mora_accuracy_percent: Option<f32>,
    pitch_error_percent: Option<f32>,
    thresholds: &ScoreThresholds,
) -> Result<CombinedScoreBreakdown> {
    thresholds.validate()?;

    if let Some(mora) = mora_accuracy_percent
        && (!mora.is_finite() || !(0.0..=100.0).contains(&mora))
    {
        bail!("Mora accuracy percentage must be within 0.0..=100.0");
    }
    if let Some(pitch_err) = pitch_error_percent
        && (!pitch_err.is_finite() || !(0.0..=100.0).contains(&pitch_err))
    {
        bail!("Pitch error percentage must be within 0.0..=100.0");
    }

    let pitch_accuracy_percent = pitch_error_percent.map(|err| (100.0 - err).clamp(0.0, 100.0));

    let combined_score_percent = match mode {
        PracticeMode::MoraPurity => mora_accuracy_percent
            .ok_or_else(|| anyhow::anyhow!("Mora accuracy is required for Mora Purity mode"))?,
        PracticeMode::PitchAccent => pitch_accuracy_percent
            .ok_or_else(|| anyhow::anyhow!("Pitch error is required for Pitch Accent mode"))?,
        PracticeMode::CombinedShadowing => {
            let mora = mora_accuracy_percent.ok_or_else(|| {
                anyhow::anyhow!("Mora accuracy is required for Combined Shadowing mode")
            })?;
            let pitch_acc = pitch_accuracy_percent.ok_or_else(|| {
                anyhow::anyhow!("Pitch error is required for Combined Shadowing mode")
            })?;
            (0.5 * mora + 0.5 * pitch_acc).clamp(0.0, 100.0)
        }
    };

    let recommended_rating = thresholds.rate_score(combined_score_percent);
    Ok(CombinedScoreBreakdown {
        mode,
        mora_accuracy_percent,
        pitch_accuracy_percent,
        pitch_error_percent,
        combined_score_percent,
        recommended_rating,
    })
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::models::FsrsRating;

    #[test]
    fn combine_practice_score_handles_all_three_modes_and_validates_inputs() -> Result<()> {
        let thresholds = ScoreThresholds::default();

        let mora_only = combine_practice_score(
            PracticeMode::MoraPurity,
            Some(82.0),
            None,
            &thresholds,
        )?;
        assert!((mora_only.combined_score_percent - 82.0).abs() < 1e-4);
        assert_eq!(mora_only.recommended_rating, FsrsRating::Good);

        let pitch_only = combine_practice_score(
            PracticeMode::PitchAccent,
            None,
            Some(8.0),
            &thresholds,
        )?;
        assert!((pitch_only.combined_score_percent - 92.0).abs() < 1e-4);
        assert_eq!(pitch_only.pitch_accuracy_percent, Some(92.0));
        assert_eq!(pitch_only.recommended_rating, FsrsRating::Easy);

        let shadowing = combine_practice_score(
            PracticeMode::CombinedShadowing,
            Some(60.0),
            Some(40.0),
            &thresholds,
        )?;
        assert!((shadowing.combined_score_percent - 60.0).abs() < 1e-4);
        assert_eq!(shadowing.recommended_rating, FsrsRating::Hard);

        assert!(
            combine_practice_score(PracticeMode::MoraPurity, None, Some(10.0), &thresholds)
                .is_err()
        );
        assert!(
            combine_practice_score(PracticeMode::PitchAccent, Some(90.0), None, &thresholds)
                .is_err()
        );
        assert!(
            combine_practice_score(PracticeMode::CombinedShadowing, Some(90.0), None, &thresholds)
                .is_err()
        );
        assert!(
            combine_practice_score(PracticeMode::MoraPurity, Some(105.0), None, &thresholds)
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn average_pitch_error_percent_computes_mean_or_none() {
        assert_eq!(average_pitch_error_percent(&[]), None);

        let now = Utc::now();
        let logs = vec![
            PitchErrorLogEntry {
                review_id: 1,
                card_id: 10,
                deck_name: "Core 2k".to_string(),
                word: "橋".to_string(),
                kana: "はし".to_string(),
                pitch_error_percent: 12.0,
                dtw_distance: 0.72,
                reviewed_at: now,
            },
            PitchErrorLogEntry {
                review_id: 2,
                card_id: 10,
                deck_name: "Core 2k".to_string(),
                word: "橋".to_string(),
                kana: "はし".to_string(),
                pitch_error_percent: 18.0,
                dtw_distance: 1.08,
                reviewed_at: now,
            },
        ];
        assert_eq!(average_pitch_error_percent(&logs), Some(15.0));
    }
}

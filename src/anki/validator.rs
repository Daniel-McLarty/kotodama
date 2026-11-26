// SPDX-License-Identifier: MPL-2.0

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use crate::audio::playback::decode_audio_file;
use crate::models::{AnkiCard, AudioQualityIssue, DeckAudioReport};

/// Minimum usable clip duration in milliseconds before flagging GIGO short-audio warnings.
pub const MIN_AUDIO_DURATION_MS: u64 = 200;

/// Peak sample magnitude threshold considered digital clipping.
pub const CLIPPING_PEAK_THRESHOLD: f32 = 0.99;

/// Minimum count of samples at or above [`CLIPPING_PEAK_THRESHOLD`] to flag clipping.
pub const MIN_CLIPPED_SAMPLES: usize = 4;

/// Safely resolves an Anki media filename within `media_dir`, rejecting empty or traversal paths.
pub fn resolve_media_path(media_dir: &Path, filename: &str) -> Option<PathBuf> {
    let trimmed = filename.trim();
    if trimmed.is_empty() {
        return None;
    }

    let candidate = Path::new(trimmed);
    if candidate.is_absolute()
        || !candidate
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return None;
    }

    Some(media_dir.join(candidate))
}

/// Validates a single reference audio clip for missing files, ultra-short duration, clipping, and decode errors.
pub fn validate_audio_clip(media_dir: &Path, filename: &str) -> Vec<AudioQualityIssue> {
    let Some(path) = resolve_media_path(media_dir, filename) else {
        return vec![AudioQualityIssue::MissingFile];
    };

    if !path.is_file() {
        return vec![AudioQualityIssue::MissingFile];
    }

    let decoded = match decode_audio_file(&path) {
        Ok(audio) => audio,
        Err(err) => {
            return vec![AudioQualityIssue::DecodeFailed {
                reason: format!("{err:#}"),
            }];
        }
    };

    let mut issues = Vec::new();
    let duration_ms = decoded.duration_ms();
    if duration_ms < MIN_AUDIO_DURATION_MS {
        issues.push(AudioQualityIssue::TooShort { duration_ms });
    }

    let mut peak_ratio = 0.0_f32;
    let mut clipped_samples = 0_usize;
    for &sample in &decoded.samples {
        let magnitude = sample.abs();
        if magnitude > peak_ratio {
            peak_ratio = magnitude;
        }
        if magnitude >= CLIPPING_PEAK_THRESHOLD {
            clipped_samples = clipped_samples.saturating_add(1);
        }
    }

    if peak_ratio >= CLIPPING_PEAK_THRESHOLD && clipped_samples >= MIN_CLIPPED_SAMPLES {
        issues.push(AudioQualityIssue::ClippedAudio { peak_ratio });
    }

    issues
}

/// Validates a card's word and optional sentence audio clips, storing discovered issues on `card`.
#[allow(dead_code)]
pub fn validate_card_audio(card: &mut AnkiCard, media_dir: &Path) -> usize {
    let mut cache = HashMap::new();
    validate_card_audio_with_cache(card, media_dir, &mut cache)
}

/// Validates all cards in a deck against `media_dir` and returns an aggregated [`DeckAudioReport`].
#[allow(dead_code)]
pub fn validate_deck_cards(cards: &mut [AnkiCard], media_dir: &Path) -> DeckAudioReport {
    let mut cache: HashMap<String, Vec<AudioQualityIssue>> = HashMap::new();
    let mut report = DeckAudioReport {
        total_cards: cards.len(),
        ..DeckAudioReport::default()
    };

    for card in cards {
        let clips_checked = validate_card_audio_with_cache(card, media_dir, &mut cache);
        report.total_clips_checked = report.total_clips_checked.saturating_add(clips_checked);

        if card.has_audio_warnings() {
            report.flagged_cards = report.flagged_cards.saturating_add(1);
            for issue in &card.audio_issues {
                match issue {
                    AudioQualityIssue::MissingFile => {
                        report.missing_files = report.missing_files.saturating_add(1);
                    }
                    AudioQualityIssue::TooShort { .. } => {
                        report.too_short_clips = report.too_short_clips.saturating_add(1);
                    }
                    AudioQualityIssue::ClippedAudio { .. } => {
                        report.clipped_clips = report.clipped_clips.saturating_add(1);
                    }
                    AudioQualityIssue::DecodeFailed { .. } => {
                        report.decode_failures = report.decode_failures.saturating_add(1);
                    }
                }
            }
        }
    }

    report
}

fn validate_card_audio_with_cache(
    card: &mut AnkiCard,
    media_dir: &Path,
    cache: &mut HashMap<String, Vec<AudioQualityIssue>>,
) -> usize {
    let mut issues = lookup_or_validate(media_dir, &card.word_audio, cache);
    let mut clips_checked = 1_usize;

    if let Some(sentence_audio) = &card.sentence_audio {
        clips_checked = clips_checked.saturating_add(1);
        issues.extend(lookup_or_validate(media_dir, sentence_audio, cache));
    }

    card.audio_issues = issues;
    clips_checked
}

fn lookup_or_validate(
    media_dir: &Path,
    filename: &str,
    cache: &mut HashMap<String, Vec<AudioQualityIssue>>,
) -> Vec<AudioQualityIssue> {
    if let Some(cached) = cache.get(filename) {
        return cached.clone();
    }
    let issues = validate_audio_clip(media_dir, filename);
    cache.insert(filename.to_string(), issues.clone());
    issues
}

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;
    use std::fs;

    use anyhow::Result;
    use tempfile::TempDir;

    use super::*;

    fn write_wav(path: &Path, sample_rate: u32, duration_ms: u64, amplitude: f32) -> Result<()> {
        let total_frames = (u64::from(sample_rate) * duration_ms / 1000) as usize;
        let data_bytes_len = (total_frames * 2) as u32;
        let riff_size = 36_u32 + data_bytes_len;

        let mut bytes = Vec::with_capacity(44 + data_bytes_len as usize);
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&riff_size.to_le_bytes());
        bytes.extend_from_slice(b"WAVE");
        bytes.extend_from_slice(b"fmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&sample_rate.to_le_bytes());
        bytes.extend_from_slice(&(sample_rate * 2).to_le_bytes());
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_bytes_len.to_le_bytes());

        for frame in 0..total_frames {
            let t = frame as f32 / sample_rate as f32;
            let sample = (TAU * 330.0 * t).sin() * amplitude;
            let pcm = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16;
            bytes.extend_from_slice(&pcm.to_le_bytes());
        }

        fs::write(path, bytes)?;
        Ok(())
    }

    fn sample_card(card_id: i64, word_audio: &str, sentence_audio: Option<&str>) -> AnkiCard {
        AnkiCard {
            card_id,
            note_id: card_id + 1000,
            deck_name: "Core 2k".to_string(),
            word: "水".to_string(),
            kana: "みず".to_string(),
            sentence: None,
            word_audio: word_audio.to_string(),
            sentence_audio: sentence_audio.map(str::to_string),
            anki_queue: AnkiCard::LEARNED_QUEUE,
            audio_issues: Vec::new(),
            last_pitch_error_percent: None,
        }
    }

    #[test]
    fn resolve_media_path_rejects_empty_and_parent_traversal() {
        let dir = Path::new("/tmp/collection.media");
        assert_eq!(resolve_media_path(dir, ""), None);
        assert_eq!(resolve_media_path(dir, "../secret.wav"), None);
        assert_eq!(resolve_media_path(dir, "/etc/passwd"), None);
        assert_eq!(
            resolve_media_path(dir, "kutsu.mp3"),
            Some(dir.join("kutsu.mp3"))
        );
    }

    #[test]
    fn validator_flags_short_clipped_missing_and_corrupt_audio() -> Result<()> {
        let dir = TempDir::new()?;
        let clean_path = dir.path().join("clean.wav");
        let short_path = dir.path().join("short.wav");
        let clipped_path = dir.path().join("clipped.wav");
        let corrupt_path = dir.path().join("corrupt.mp3");

        write_wav(&clean_path, 16_000, 400, 0.5)?;
        write_wav(&short_path, 16_000, 100, 0.5)?;
        write_wav(&clipped_path, 16_000, 350, 2.0)?;
        fs::write(&corrupt_path, b"corrupt header")?;

        assert!(validate_audio_clip(dir.path(), "clean.wav").is_empty());
        assert_eq!(
            validate_audio_clip(dir.path(), "missing.wav"),
            vec![AudioQualityIssue::MissingFile]
        );
        assert_eq!(
            validate_audio_clip(dir.path(), ""),
            vec![AudioQualityIssue::MissingFile]
        );

        let short_issues = validate_audio_clip(dir.path(), "short.wav");
        assert!(matches!(
            short_issues.as_slice(),
            [AudioQualityIssue::TooShort { duration_ms }] if *duration_ms < MIN_AUDIO_DURATION_MS
        ));

        let clipped_issues = validate_audio_clip(dir.path(), "clipped.wav");
        assert!(matches!(
            clipped_issues.as_slice(),
            [AudioQualityIssue::ClippedAudio { peak_ratio }] if *peak_ratio >= CLIPPING_PEAK_THRESHOLD
        ));

        let corrupt_issues = validate_audio_clip(dir.path(), "corrupt.mp3");
        assert!(matches!(
            corrupt_issues.as_slice(),
            [AudioQualityIssue::DecodeFailed { .. }]
        ));
        Ok(())
    }

    #[test]
    fn validate_deck_cards_aggregates_counts_across_word_and_sentence_clips() -> Result<()> {
        let dir = TempDir::new()?;
        write_wav(&dir.path().join("clean.wav"), 16_000, 400, 0.4)?;
        write_wav(&dir.path().join("short.wav"), 16_000, 80, 0.4)?;
        write_wav(&dir.path().join("clipped.wav"), 16_000, 350, 2.0)?;

        let mut cards = vec![
            sample_card(1, "clean.wav", Some("clean.wav")),
            sample_card(2, "short.wav", Some("missing.wav")),
            sample_card(3, "clipped.wav", None),
        ];

        let report = validate_deck_cards(&mut cards, dir.path());
        assert_eq!(report.total_cards, 3);
        assert_eq!(report.total_clips_checked, 5);
        assert_eq!(report.flagged_cards, 2);
        assert_eq!(report.missing_files, 1);
        assert_eq!(report.too_short_clips, 1);
        assert_eq!(report.clipped_clips, 1);
        assert_eq!(report.decode_failures, 0);
        assert!(cards[0].audio_issues.is_empty());
        assert_eq!(cards[1].audio_issues.len(), 2);
        assert_eq!(cards[2].audio_issues.len(), 1);
        Ok(())
    }
}

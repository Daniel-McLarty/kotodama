// SPDX-License-Identifier: MPL-2.0

use anyhow::{Result, bail};
use chrono::Utc;

use crate::audio::capture::compute_rms_and_peak;
use crate::models::{DecodedAudio, PitchAnalysis, PitchExtractionConfig};

/// Extracts a frame-wise fundamental frequency (`F0` in Hz) contour using the YIN algorithm.
pub fn extract_f0_contour(
    audio: &DecodedAudio,
    config: &PitchExtractionConfig,
) -> Result<Vec<Option<f32>>> {
    config.validate()?;
    if audio.sample_rate == 0 || audio.channels == 0 {
        bail!("Audio buffer must have positive sample rate and channel count");
    }

    let mono = audio.to_mono();
    let sample_rate = audio.sample_rate as f32;
    let max_tau = (sample_rate / config.min_f0_hz).ceil() as usize;
    let min_tau = ((sample_rate / config.max_f0_hz).floor() as usize).max(2);
    if max_tau <= min_tau {
        bail!("Pitch extraction lag bounds are invalid for sample rate {}", audio.sample_rate);
    }

    let half_frame = max_tau;
    let frame_len = half_frame.saturating_mul(2);
    let hop = ((sample_rate * config.hop_ms / 1000.0).round() as usize).max(1);
    if mono.len() < frame_len {
        return Ok(Vec::new());
    }

    let mut diff = vec![0.0_f32; max_tau + 1];
    let mut cmndf = vec![0.0_f32; max_tau + 1];
    let mut raw_f0 = Vec::with_capacity((mono.len() - frame_len) / hop + 1);

    let mut start = 0_usize;
    while start + frame_len <= mono.len() {
        let frame = &mono[start..start + frame_len];
        let (rms, _) = compute_rms_and_peak(&frame[..half_frame]);
        if rms < config.min_voiced_rms {
            raw_f0.push(None);
            start += hop;
            continue;
        }

        diff[0] = 0.0;
        for (tau, slot) in diff.iter_mut().enumerate().skip(1) {
            let mut sum = 0.0_f32;
            for j in 0..half_frame {
                let delta = frame[j] - frame[j + tau];
                sum += delta * delta;
            }
            *slot = sum;
        }

        cmndf[0] = 1.0;
        let mut running_sum = 0.0_f32;
        for tau in 1..=max_tau {
            running_sum += diff[tau];
            cmndf[tau] = if running_sum > 1e-12 {
                diff[tau] * (tau as f32) / running_sum
            } else {
                1.0
            };
        }

        let mut selected_tau: Option<usize> = None;
        let mut tau = min_tau;
        while tau <= max_tau {
            if cmndf[tau] < config.yin_threshold {
                while tau + 1 <= max_tau && cmndf[tau + 1] < cmndf[tau] {
                    tau += 1;
                }
                selected_tau = Some(tau);
                break;
            }
            tau += 1;
        }

        let estimate = selected_tau.and_then(|best_tau| {
            let refined_tau = if best_tau > min_tau && best_tau < max_tau {
                let s0 = cmndf[best_tau - 1];
                let s1 = cmndf[best_tau];
                let s2 = cmndf[best_tau + 1];
                let denom = 2.0 * (s0 - 2.0 * s1 + s2);
                if denom.abs() > 1e-12 {
                    (best_tau as f32 + (s0 - s2) / denom)
                        .clamp(min_tau as f32, max_tau as f32)
                } else {
                    best_tau as f32
                }
            } else {
                best_tau as f32
            };

            let f0 = sample_rate / refined_tau;
            if f0.is_finite() && f0 >= config.min_f0_hz && f0 <= config.max_f0_hz {
                Some(f0)
            } else {
                None
            }
        });

        raw_f0.push(estimate);
        start += hop;
    }

    Ok(median_filter_3_voiced(&raw_f0))
}

fn median_filter_3_voiced(contour: &[Option<f32>]) -> Vec<Option<f32>> {
    if contour.len() < 3 {
        return contour.to_vec();
    }

    let mut smoothed = contour.to_vec();
    for i in 1..(contour.len() - 1) {
        if let (Some(a), Some(b), Some(c)) = (contour[i - 1], contour[i], contour[i + 1]) {
            let mut triple = [a, b, c];
            triple.sort_by(f32::total_cmp);
            smoothed[i] = Some(triple[1]);
        }
    }
    smoothed
}

/// Normalizes voiced `F0` values to semitones relative to the contour's median `F0`.
pub fn normalize_to_semitones(f0_hz: &[Option<f32>]) -> Result<(Vec<Option<f32>>, f32)> {
    let mut voiced: Vec<f32> = f0_hz
        .iter()
        .filter_map(|&val| val.filter(|hz| hz.is_finite() && *hz > 0.0))
        .collect();

    if voiced.is_empty() {
        bail!("No voiced pitch frames were detected for semitone normalization");
    }

    voiced.sort_by(f32::total_cmp);
    let mid = voiced.len() / 2;
    let median_hz = if voiced.len().is_multiple_of(2) {
        0.5 * (voiced[mid - 1] + voiced[mid])
    } else {
        voiced[mid]
    };

    if !median_hz.is_finite() || median_hz <= 0.0 {
        bail!("Computed speaker median F0 is not positive");
    }

    let semitones = f0_hz
        .iter()
        .map(|&val| {
            val.filter(|hz| hz.is_finite() && *hz > 0.0)
                .map(|hz| 12.0 * (hz / median_hz).log2())
        })
        .collect();

    Ok((semitones, median_hz))
}

/// Computes path-normalized Dynamic Time Warping distance (in semitones) between voiced pitch contours.
pub fn dtw_pitch_distance(reference: &[Option<f32>], user: &[Option<f32>]) -> Result<f32> {
    let ref_voiced: Vec<f32> = reference
        .iter()
        .filter_map(|&val| val.filter(|v| v.is_finite()))
        .collect();
    let user_voiced: Vec<f32> = user
        .iter()
        .filter_map(|&val| val.filter(|v| v.is_finite()))
        .collect();

    if ref_voiced.is_empty() {
        bail!("Reference pitch contour has no voiced frames for DTW alignment");
    }
    if user_voiced.is_empty() {
        bail!("User recording has no voiced pitch frames for DTW alignment");
    }

    let n = ref_voiced.len();
    let m = user_voiced.len();
    let mut cost = vec![f32::INFINITY; n * m];
    let mut steps = vec![0_usize; n * m];

    cost[0] = (ref_voiced[0] - user_voiced[0]).abs();
    steps[0] = 1;

    for i in 1..n {
        let idx = i * m;
        let local = (ref_voiced[i] - user_voiced[0]).abs();
        cost[idx] = cost[(i - 1) * m] + local;
        steps[idx] = steps[(i - 1) * m] + 1;
    }

    for j in 1..m {
        let local = (ref_voiced[0] - user_voiced[j]).abs();
        cost[j] = cost[j - 1] + local;
        steps[j] = steps[j - 1] + 1;
    }

    for i in 1..n {
        for j in 1..m {
            let local = (ref_voiced[i] - user_voiced[j]).abs();
            let diag_idx = (i - 1) * m + (j - 1);
            let up_idx = (i - 1) * m + j;
            let left_idx = i * m + (j - 1);

            let mut best_cost = cost[diag_idx];
            let mut best_steps = steps[diag_idx];

            if cost[up_idx] < best_cost {
                best_cost = cost[up_idx];
                best_steps = steps[up_idx];
            }
            if cost[left_idx] < best_cost {
                best_cost = cost[left_idx];
                best_steps = steps[left_idx];
            }

            let idx = i * m + j;
            cost[idx] = best_cost + local;
            steps[idx] = best_steps + 1;
        }
    }

    let total_steps = steps[n * m - 1].max(1) as f32;
    Ok(cost[n * m - 1] / total_steps)
}

/// Converts a mean semitone DTW distance into an intuitive `0.0..=100.0` pitch error percentage.
pub fn dtw_distance_to_error_percent(dtw_distance_semitones: f32) -> f32 {
    if !dtw_distance_semitones.is_finite() {
        return 100.0;
    }
    if dtw_distance_semitones <= 0.0 {
        return 0.0;
    }
    ((dtw_distance_semitones / 6.0) * 100.0).clamp(0.0, 100.0)
}

/// Compares reference and user audio buffers to produce a complete [`PitchAnalysis`].
#[allow(dead_code)]
pub fn analyze_pitch_contour(
    reference: &DecodedAudio,
    user: &DecodedAudio,
    config: &PitchExtractionConfig,
) -> Result<PitchAnalysis> {
    let ref_f0 = extract_f0_contour(reference, config)?;
    let user_f0 = extract_f0_contour(user, config)?;

    let (reference_semitones, reference_median_hz) = normalize_to_semitones(&ref_f0)?;
    let (user_semitones, user_median_hz) = normalize_to_semitones(&user_f0)?;

    let dtw_distance = dtw_pitch_distance(&reference_semitones, &user_semitones)?;
    let pitch_error_percent = dtw_distance_to_error_percent(dtw_distance);

    PitchAnalysis::new(
        reference_semitones,
        user_semitones,
        dtw_distance,
        pitch_error_percent,
        reference_median_hz,
        user_median_hz,
        Utc::now(),
    )
}

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;

    use super::*;
    use crate::audio::playback::time_stretch_preserve_pitch;
    use crate::models::PlaybackSpeed;

    fn constant_sine(sample_rate: u32, duration_ms: u64, freq_hz: f32, amp: f32) -> DecodedAudio {
        let len = (u64::from(sample_rate) * duration_ms / 1000) as usize;
        let samples = (0..len)
            .map(|i| (TAU * freq_hz * i as f32 / sample_rate as f32).sin() * amp)
            .collect();
        DecodedAudio {
            samples,
            sample_rate,
            channels: 1,
        }
    }

    fn sweep_sine(
        sample_rate: u32,
        duration_ms: u64,
        start_hz: f32,
        end_hz: f32,
        amp: f32,
    ) -> DecodedAudio {
        let len = (u64::from(sample_rate) * duration_ms / 1000) as usize;
        let mut samples = Vec::with_capacity(len);
        let mut phase = 0.0_f64;
        for i in 0..len {
            let t = i as f64 / (len.saturating_sub(1).max(1)) as f64;
            let freq = f64::from(start_hz) + (f64::from(end_hz) - f64::from(start_hz)) * t;
            phase += std::f64::consts::TAU * freq / f64::from(sample_rate);
            samples.push((phase.sin() as f32) * amp);
        }
        DecodedAudio {
            samples,
            sample_rate,
            channels: 1,
        }
    }

    #[test]
    fn extract_f0_accurately_tracks_synthetic_sine_waves_across_sample_rates() -> Result<()> {
        let cfg = PitchExtractionConfig::default();

        for (sample_rate, freq_hz) in [(16_000_u32, 150.0_f32), (44_100_u32, 280.0_f32)] {
            let audio = constant_sine(sample_rate, 400, freq_hz, 0.5);
            let contour = extract_f0_contour(&audio, &cfg)?;
            let voiced: Vec<f32> = contour.iter().flatten().copied().collect();
            assert!(!voiced.is_empty(), "expected voiced frames for {freq_hz} Hz");

            for &hz in &voiced {
                assert!(
                    (hz - freq_hz).abs() < 2.0,
                    "expected ~{freq_hz} Hz at {sample_rate} Hz SR, got {hz:.2} Hz"
                );
            }

            let (_, median_hz) = normalize_to_semitones(&contour)?;
            assert!((median_hz - freq_hz).abs() < 1.5);
        }
        Ok(())
    }

    #[test]
    fn silence_and_white_noise_produce_no_voiced_pitch_contour() -> Result<()> {
        let cfg = PitchExtractionConfig::default();

        let silence = DecodedAudio {
            samples: vec![0.0; 8_000],
            sample_rate: 16_000,
            channels: 1,
        };
        let silent_f0 = extract_f0_contour(&silence, &cfg)?;
        assert!(silent_f0.iter().all(Option::is_none));
        assert!(normalize_to_semitones(&silent_f0).is_err());

        // Deterministic xorshift32 noise.
        let mut state = 0x1234_5678_u32;
        let mut noise_samples = Vec::with_capacity(8_000);
        for _ in 0..8_000 {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let norm = (state as f32 / u32::MAX as f32) * 2.0 - 1.0;
            noise_samples.push(norm * 0.25);
        }
        let noise = DecodedAudio {
            samples: noise_samples,
            sample_rate: 16_000,
            channels: 1,
        };
        let noise_f0 = extract_f0_contour(&noise, &cfg)?;
        assert!(noise_f0.iter().all(Option::is_none));
        assert!(normalize_to_semitones(&noise_f0).is_err());
        Ok(())
    }

    #[test]
    fn semitone_normalization_and_dtw_are_invariant_to_register_and_tempo() -> Result<()> {
        let cfg = PitchExtractionConfig::default();

        let low_speaker = sweep_sine(16_000, 600, 110.0, 165.0, 0.5);
        let high_speaker = sweep_sine(16_000, 600, 220.0, 330.0, 0.5);
        let register_analysis = analyze_pitch_contour(&low_speaker, &high_speaker, &cfg)?;
        assert!(
            register_analysis.dtw_distance < 0.25,
            "octave-shifted speaker should have near-zero normalized DTW distance, got {}",
            register_analysis.dtw_distance
        );
        assert!(register_analysis.pitch_error_percent < 5.0);

        let normal_pace = sweep_sine(16_000, 800, 140.0, 210.0, 0.5);
        let slow_pace = sweep_sine(16_000, 1200, 140.0, 210.0, 0.5);
        let tempo_analysis = analyze_pitch_contour(&normal_pace, &slow_pace, &cfg)?;
        assert!(
            tempo_analysis.dtw_distance < 0.35,
            "DTW should align differing speaking rates, got {}",
            tempo_analysis.dtw_distance
        );
        assert!(tempo_analysis.pitch_error_percent < 6.0);

        let rising = sweep_sine(16_000, 600, 130.0, 200.0, 0.5);
        let falling = sweep_sine(16_000, 600, 200.0, 130.0, 0.5);
        let mismatch = analyze_pitch_contour(&rising, &falling, &cfg)?;
        assert!(
            mismatch.pitch_error_percent > 35.0,
            "opposing pitch contours should produce high error %, got {}",
            mismatch.pitch_error_percent
        );
        Ok(())
    }

    #[test]
    fn slow_motion_wsola_playback_preserves_extracted_pitch() -> Result<()> {
        let cfg = PitchExtractionConfig::default();
        let original = constant_sine(16_000, 600, 200.0, 0.5);
        let stretched = time_stretch_preserve_pitch(&original, PlaybackSpeed::Slow)?;

        let orig_f0 = extract_f0_contour(&original, &cfg)?;
        let slow_f0 = extract_f0_contour(&stretched, &cfg)?;
        let (_, orig_median) = normalize_to_semitones(&orig_f0)?;
        let (_, slow_median) = normalize_to_semitones(&slow_f0)?;

        assert!(
            (slow_median - orig_median).abs() < 5.0,
            "expected pitch-preserving 0.75x stretch near {orig_median:.1} Hz, got {slow_median:.1} Hz"
        );
        Ok(())
    }
}

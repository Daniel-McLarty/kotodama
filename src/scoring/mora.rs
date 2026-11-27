// SPDX-License-Identifier: MPL-2.0

use std::collections::BTreeMap;

use anyhow::{Result, bail};

use crate::audio::capture::compute_rms_and_peak;
use crate::models::{
    KanaMistakeEntry, KanaStats, MoraAttempt, MoraEquivalenceRule, MoraEvaluation,
    MoraScoreRequest, PhoneticRealization,
};

/// Pluggable boundary for Japanese mora pronunciation classifiers (heuristic or ONNX).
#[allow(dead_code)]
pub trait MoraScorer: Send + Sync {
    /// Returns a human-readable identifier for this scorer backend.
    fn name(&self) -> &'static str;

    /// Evaluates a user audio recording against the target kana sequence.
    fn score_morae(&self, request: &MoraScoreRequest) -> Result<MoraEvaluation>;
}

/// Built-in pure-Rust acoustic mora scorer honoring Japanese allophone and devoicing rules.
#[allow(dead_code)]
#[derive(Debug, Default, Clone, Copy)]
pub struct HeuristicAcousticMoraScorer;

impl MoraScorer for HeuristicAcousticMoraScorer {
    fn name(&self) -> &'static str {
        "heuristic-acoustic-v1"
    }

    fn score_morae(&self, request: &MoraScoreRequest) -> Result<MoraEvaluation> {
        if request.user_audio.sample_rate == 0 || request.user_audio.channels == 0 {
            bail!("User audio must have positive sample rate and channel count");
        }

        let morae = segment_kana_into_morae(&request.target_kana_sequence);
        if morae.is_empty() {
            bail!("Target kana sequence contains no scorable morae");
        }

        let user_mono = request.user_audio.to_mono();
        if user_mono.is_empty() {
            bail!("User recording contains no audio samples for mora scoring");
        }

        let ref_mono = request
            .reference_audio
            .as_ref()
            .filter(|audio| audio.sample_rate > 0 && audio.channels > 0)
            .map(|audio| audio.to_mono())
            .filter(|samples| !samples.is_empty());

        let sample_rate = request.user_audio.sample_rate as f32;
        let expected_mora_duration_ms =
            (user_mono.len() as f32 / morae.len() as f32 / sample_rate) * 1000.0;

        let mut attempts = Vec::with_capacity(morae.len());
        let mut accepted_allophone_notes = Vec::new();
        let mut correct_count = 0_usize;

        for (idx, expected) in morae.iter().enumerate() {
            let start = idx * user_mono.len() / morae.len();
            let end = ((idx + 1) * user_mono.len() / morae.len()).max(start);
            let segment = &user_mono[start..end];
            let seg_duration_ms = (segment.len() as f32 / sample_rate) * 1000.0;

            let (rms, _) = compute_rms_and_peak(segment);
            let zcr = zero_crossing_rate(segment);
            let periodicity = estimate_voicing_periodicity(segment, request.user_audio.sample_rate);

            let ref_rms = ref_mono.as_ref().map(|samples| {
                let r_start = idx * samples.len() / morae.len();
                let r_end = ((idx + 1) * samples.len() / morae.len()).max(r_start);
                compute_rms_and_peak(&samples[r_start..r_end]).0
            });

            let (heard_kana, confidence) = classify_mora_segment(
                &morae,
                idx,
                expected,
                rms,
                zcr,
                periodicity,
                seg_duration_ms,
                expected_mora_duration_ms,
                ref_rms,
                &mut accepted_allophone_notes,
            );

            let attempt = normalize_mora_attempt(MoraAttempt {
                card_id: request.card_id,
                expected_kana: expected.clone(),
                heard_kana,
                confidence,
                recorded_at: request.recorded_at,
            });
            if attempt.is_correct() {
                correct_count = correct_count.saturating_add(1);
            }
            attempts.push(attempt);
        }

        let mora_accuracy_percent = (correct_count as f32 / morae.len() as f32) * 100.0;
        Ok(MoraEvaluation {
            attempts,
            mora_accuracy_percent,
            scorer_name: self.name().to_string(),
            accepted_allophone_notes,
        })
    }
}

/// Converts katakana characters (`ァ..ヶ`) into hiragana while preserving other characters.
pub fn normalize_to_hiragana(input: &str) -> String {
    input
        .chars()
        .map(|ch| {
            let code = ch as u32;
            if (0x30A1..=0x30F6).contains(&code) {
                char::from_u32(code - 0x60).unwrap_or(ch)
            } else {
                ch
            }
        })
        .collect()
}

/// Segments a Japanese kana string into normalized hiragana morae, combining yōon digraphs.
pub fn segment_kana_into_morae(reading: &str) -> Vec<String> {
    let normalized = normalize_to_hiragana(reading);
    let mut morae: Vec<String> = Vec::new();

    for ch in normalized.chars() {
        if !is_scorable_kana_char(ch) {
            continue;
        }
        if is_combining_small_kana(ch)
            && let Some(prev) = morae.last_mut()
            && prev != "っ"
            && prev != "ん"
            && prev != "ー"
        {
            prev.push(ch);
            continue;
        }
        morae.push(ch.to_string());
    }

    morae
}

/// Returns `true` when `expected` and `heard` represent equivalent Japanese morae (including `じ`/`ぢ` and `ず`/`づ`).
pub fn are_kana_equivalent(expected: &str, heard: &str) -> bool {
    let left = normalize_to_hiragana(expected.trim());
    let right = normalize_to_hiragana(heard.trim());
    if left == right {
        return true;
    }

    matches!(
        (left.as_str(), right.as_str()),
        ("じ", "ぢ")
            | ("ぢ", "じ")
            | ("ず", "づ")
            | ("づ", "ず")
            | ("じゃ", "ぢゃ")
            | ("ぢゃ", "じゃ")
            | ("じゅ", "ぢゅ")
            | ("ぢゅ", "じゅ")
            | ("じょ", "ぢょ")
            | ("ぢょ", "じょ")
    )
}

/// Returns the standard Japanese allophone, equivalence, and devoicing rule for `kana`.
pub fn mora_equivalence_rule(kana: &str) -> MoraEquivalenceRule {
    let norm = normalize_to_hiragana(kana.trim());
    match norm.as_str() {
        "が" | "ぎ" | "ぐ" | "げ" | "ご" | "ぎゃ" | "ぎゅ" | "ぎょ" => {
            let (oral, nasal, approx) = ga_row_ipa(&norm);
            MoraEquivalenceRule {
                canonical_kana: norm.clone(),
                equivalent_kana: vec![norm],
                valid_realizations: vec![
                    realization(oral, "Standard oral voiced velar stop [g]", false),
                    realization(nasal, "Standard velar nasal allophone [ŋ] (bidakuon)", false),
                    realization(approx, "Intervocalic velar fricative [ɣ]", false),
                ],
                allows_vowel_devoicing: false,
            }
        }
        "ん" => MoraEquivalenceRule {
            canonical_kana: "ん".to_string(),
            equivalent_kana: vec!["ん".to_string()],
            valid_realizations: vec![
                realization("[m]", "Bilabial nasal assimilation before p/b/m", false),
                realization("[n]", "Alveolar nasal assimilation before t/d/n/r", false),
                realization("[ŋ]", "Velar nasal assimilation before k/g", false),
                realization("[ɴ]", "Word-final uvular nasal", false),
                realization("[ɰ̃]", "Nasalized glide before vowels/semivowels/fricatives", false),
            ],
            allows_vowel_devoicing: false,
        },
        "じ" | "ぢ" => MoraEquivalenceRule {
            canonical_kana: "じ".to_string(),
            equivalent_kana: vec!["じ".to_string(), "ぢ".to_string()],
            valid_realizations: vec![
                realization("[dʑi]", "Alveolo-palatal affricate realization", false),
                realization("[ʑi]", "Intervocalic alveolo-palatal fricative realization", false),
            ],
            allows_vowel_devoicing: false,
        },
        "ず" | "づ" => MoraEquivalenceRule {
            canonical_kana: "ず".to_string(),
            equivalent_kana: vec!["ず".to_string(), "づ".to_string()],
            valid_realizations: vec![
                realization("[dzɯ]", "Alveolar affricate realization", false),
                realization("[zɯ]", "Intervocalic alveolar fricative realization", false),
            ],
            allows_vowel_devoicing: false,
        },
        "じゃ" | "ぢゃ" => MoraEquivalenceRule {
            canonical_kana: "じゃ".to_string(),
            equivalent_kana: vec!["じゃ".to_string(), "ぢゃ".to_string()],
            valid_realizations: vec![
                realization("[dʑa]", "Alveolo-palatal affricate realization", false),
                realization("[ʑa]", "Intervocalic alveolo-palatal fricative realization", false),
            ],
            allows_vowel_devoicing: false,
        },
        "じゅ" | "ぢゅ" => MoraEquivalenceRule {
            canonical_kana: "じゅ".to_string(),
            equivalent_kana: vec!["じゅ".to_string(), "ぢゅ".to_string()],
            valid_realizations: vec![
                realization("[dʑɯ]", "Alveolo-palatal affricate realization", false),
                realization("[ʑɯ]", "Intervocalic alveolo-palatal fricative realization", false),
            ],
            allows_vowel_devoicing: false,
        },
        "じょ" | "ぢょ" => MoraEquivalenceRule {
            canonical_kana: "じょ".to_string(),
            equivalent_kana: vec!["じょ".to_string(), "ぢょ".to_string()],
            valid_realizations: vec![
                realization("[dʑo]", "Alveolo-palatal affricate realization", false),
                realization("[ʑo]", "Intervocalic alveolo-palatal fricative realization", false),
            ],
            allows_vowel_devoicing: false,
        },
        _ => {
            if let Some((voiced_ipa, devoiced_ipa)) = devoicable_high_vowel_ipa(&norm) {
                MoraEquivalenceRule {
                    canonical_kana: norm.clone(),
                    equivalent_kana: vec![norm],
                    valid_realizations: vec![
                        realization(voiced_ipa, "Standard voiced high-vowel realization", false),
                        realization(
                            devoiced_ipa,
                            "Standard devoiced mora with sustained voiceless consonant beat",
                            true,
                        ),
                    ],
                    allows_vowel_devoicing: true,
                }
            } else {
                MoraEquivalenceRule {
                    canonical_kana: norm.clone(),
                    equivalent_kana: vec![norm.clone()],
                    valid_realizations: vec![realization(
                        &format!("/{norm}/"),
                        "Standard modal mora realization",
                        false,
                    )],
                    allows_vowel_devoicing: false,
                }
            }
        }
    }
}

/// Returns `true` when `morae[index]` occupies a valid Japanese high-vowel devoicing context.
pub fn is_devoicing_environment(morae: &[String], index: usize) -> bool {
    let Some(current) = morae.get(index) else {
        return false;
    };
    if !mora_equivalence_rule(current).allows_vowel_devoicing {
        return false;
    }

    match morae.get(index + 1) {
        None => true,
        Some(next) => has_voiceless_onset(next),
    }
}

/// Returns `true` when a devoiced mora realization holds a full mora beat with audible voiceless energy.
pub fn accepts_devoiced_realization(
    morae: &[String],
    index: usize,
    segment_duration_ms: f32,
    expected_mora_duration_ms: f32,
    unvoiced_rms: f32,
) -> bool {
    if !is_devoicing_environment(morae, index) {
        return false;
    }
    if !segment_duration_ms.is_finite()
        || !expected_mora_duration_ms.is_finite()
        || expected_mora_duration_ms <= 0.0
        || !unvoiced_rms.is_finite()
    {
        return false;
    }

    segment_duration_ms >= expected_mora_duration_ms * 0.65 && unvoiced_rms >= 0.008
}

/// Checks whether `realization_ipa` is a valid realization for `expected_kana` given optional `next_mora`.
#[allow(dead_code)]
pub fn accepts_phonetic_realization(
    expected_kana: &str,
    realization_ipa: &str,
    next_mora: Option<&str>,
) -> bool {
    let norm_expected = normalize_to_hiragana(expected_kana.trim());
    let rule = mora_equivalence_rule(&norm_expected);
    let trimmed_ipa = realization_ipa.trim();

    let Some(matched) = rule
        .valid_realizations
        .iter()
        .find(|entry| entry.ipa == trimmed_ipa)
    else {
        return false;
    };

    if !matched.is_devoiced {
        return true;
    }

    let morae = match next_mora {
        Some(next) => vec![norm_expected, normalize_to_hiragana(next.trim())],
        None => vec![norm_expected],
    };
    is_devoicing_environment(&morae, 0)
}

/// Normalizes katakana to hiragana and canonicalizes yotsugana equivalences in a [`MoraAttempt`].
pub fn normalize_mora_attempt(mut attempt: MoraAttempt) -> MoraAttempt {
    attempt.expected_kana = normalize_to_hiragana(attempt.expected_kana.trim());
    if let Some(heard) = attempt.heard_kana.take() {
        let norm_heard = normalize_to_hiragana(heard.trim());
        if are_kana_equivalent(&attempt.expected_kana, &norm_heard) {
            attempt.heard_kana = Some(attempt.expected_kana.clone());
        } else {
            attempt.heard_kana = Some(norm_heard);
        }
    }
    attempt
}

/// Aggregates normalized [`MoraAttempt`] records into per-kana [`KanaStats`] keyed by canonical kana.
#[allow(dead_code)]
pub fn aggregate_kana_stats(attempts: &[MoraAttempt]) -> BTreeMap<String, KanaStats> {
    let mut map: BTreeMap<String, KanaStats> = BTreeMap::new();
    for attempt in attempts {
        let normalized = normalize_mora_attempt(attempt.clone());
        let stats = map
            .entry(normalized.expected_kana.clone())
            .or_insert_with(|| KanaStats::new(&normalized.expected_kana));
        stats.record_attempt(&normalized);
    }
    map
}

/// Returns up to the 5 most frequent mistakes for a single kana's [`KanaStats`].
#[allow(dead_code)]
pub fn top_mistakes_for_kana(stats: &KanaStats) -> Vec<KanaMistakeEntry> {
    stats.top_mistakes(5)
}

/// Formats a user-facing clarity and top-5 mistake breakdown string for `stats`.
#[allow(dead_code)]
pub fn format_kana_breakdown(stats: &KanaStats) -> String {
    stats.format_breakdown()
}

#[allow(clippy::too_many_arguments)]
fn classify_mora_segment(
    morae: &[String],
    index: usize,
    expected: &str,
    rms: f32,
    zcr: f32,
    periodicity: f32,
    seg_duration_ms: f32,
    expected_mora_duration_ms: f32,
    ref_rms: Option<f32>,
    notes: &mut Vec<String>,
) -> (Option<String>, f32) {
    if rms < 0.005 {
        return (None, 0.15);
    }

    if let Some(ref_energy) = ref_rms
        && ref_energy >= 0.02
        && rms < ref_energy * 0.12
    {
        return (None, 0.25);
    }

    if is_devoicing_environment(morae, index)
        && periodicity < 0.45
        && accepts_devoiced_realization(
            morae,
            index,
            seg_duration_ms,
            expected_mora_duration_ms,
            rms,
        )
    {
        notes.push(format!(
            "Accepted devoiced '{expected}' holding full mora beat ({seg_duration_ms:.0} ms)"
        ));
        let confidence = (0.78 + rms.min(0.20)).clamp(0.0, 0.96);
        return (Some(expected.to_string()), confidence);
    }

    if is_ga_row(expected) && rms >= 0.008 && periodicity >= 0.30 {
        if periodicity >= 0.65 {
            notes.push(format!(
                "Accepted nasal allophone [ŋ] for '{expected}'"
            ));
        } else {
            notes.push(format!(
                "Accepted oral stop [g] realization for '{expected}'"
            ));
        }
        let confidence = (0.65 + periodicity * 0.30).clamp(0.0, 0.96);
        return (Some(expected.to_string()), confidence);
    }

    if expected == "ん" && rms >= 0.008 && periodicity >= 0.30 {
        notes.push("Accepted assimilated syllabic nasal for 'ん'".to_string());
        let confidence = (0.68 + periodicity * 0.28).clamp(0.0, 0.96);
        return (Some("ん".to_string()), confidence);
    }

    if expected == "っ" {
        if periodicity < 0.35 && rms < 0.06 && seg_duration_ms >= expected_mora_duration_ms * 0.55
        {
            return (Some("っ".to_string()), 0.85);
        }
        return (None, 0.35);
    }

    if expected == "つ" && zcr > 0.28 && periodicity < 0.28 {
        return (Some("す".to_string()), 0.55);
    }

    if periodicity >= 0.30 && rms >= 0.008 {
        let confidence = (0.60 + periodicity * 0.35).clamp(0.0, 0.97);
        (Some(expected.to_string()), confidence)
    } else {
        (None, (0.25 + rms).clamp(0.0, 0.49))
    }
}

fn realization(ipa: &str, label: &str, is_devoiced: bool) -> PhoneticRealization {
    PhoneticRealization {
        ipa: ipa.to_string(),
        label: label.to_string(),
        is_devoiced,
    }
}

fn ga_row_ipa(kana: &str) -> (&'static str, &'static str, &'static str) {
    match kana {
        "が" => ("[ga]", "[ŋa]", "[ɣa]"),
        "ぎ" => ("[ɡʲi]", "[ŋʲi]", "[ɣʲi]"),
        "ぐ" => ("[gɯ]", "[ŋɯ]", "[ɣɯ]"),
        "げ" => ("[ge]", "[ŋe]", "[ɣe]"),
        "ご" => ("[go]", "[ŋo]", "[ɣo]"),
        "ぎゃ" => ("[ɡʲa]", "[ŋʲa]", "[ɣʲa]"),
        "ぎゅ" => ("[ɡʲɯ]", "[ŋʲɯ]", "[ɣʲɯ]"),
        "ぎょ" => ("[ɡʲo]", "[ŋʲo]", "[ɣʲo]"),
        _ => ("[g]", "[ŋ]", "[ɣ]"),
    }
}

fn devoicable_high_vowel_ipa(kana: &str) -> Option<(&'static str, &'static str)> {
    match kana {
        "き" => Some(("[kʲi]", "[kʲi̥]")),
        "く" => Some(("[kɯ]", "[kɯ̥]")),
        "し" => Some(("[ɕi]", "[ɕi̥]")),
        "す" => Some(("[sɯ]", "[sɯ̥]")),
        "ち" => Some(("[tɕi]", "[tɕi̥]")),
        "つ" => Some(("[tsɯ]", "[tsɯ̥]")),
        "ひ" => Some(("[çi]", "[çi̥]")),
        "ふ" => Some(("[ɸɯ]", "[ɸɯ̥]")),
        "ぴ" => Some(("[pʲi]", "[pʲi̥]")),
        "ぷ" => Some(("[pɯ]", "[pɯ̥]")),
        "きゅ" => Some(("[kʲɯ]", "[kʲɯ̥]")),
        "しゅ" => Some(("[ɕɯ]", "[ɕɯ̥]")),
        "ちゅ" => Some(("[tɕɯ]", "[tɕɯ̥]")),
        "ひゅ" => Some(("[çɯ]", "[çɯ̥]")),
        "ぴゅ" => Some(("[pʲɯ]", "[pʲɯ̥]")),
        _ => None,
    }
}

fn is_ga_row(kana: &str) -> bool {
    matches!(
        kana,
        "が" | "ぎ" | "ぐ" | "げ" | "ご" | "ぎゃ" | "ぎゅ" | "ぎょ"
    )
}

fn has_voiceless_onset(mora: &str) -> bool {
    let norm = normalize_to_hiragana(mora.trim());
    let Some(first) = norm.chars().next() else {
        return false;
    };
    matches!(
        first,
        'か' | 'き' | 'く' | 'け' | 'こ'
            | 'さ' | 'し' | 'す' | 'せ' | 'そ'
            | 'た' | 'ち' | 'つ' | 'て' | 'と'
            | 'は' | 'ひ' | 'ふ' | 'へ' | 'ほ'
            | 'ぱ' | 'ぴ' | 'ぷ' | 'ぺ' | 'ぽ'
            | 'っ'
    )
}

fn is_scorable_kana_char(ch: char) -> bool {
    let code = ch as u32;
    (0x3041..=0x3096).contains(&code) || ch == 'ー'
}

fn is_combining_small_kana(ch: char) -> bool {
    matches!(
        ch,
        'ゃ' | 'ゅ' | 'ょ' | 'ぁ' | 'ぃ' | 'ぅ' | 'ぇ' | 'ぉ' | 'ゎ'
    )
}

fn zero_crossing_rate(samples: &[f32]) -> f32 {
    if samples.len() < 2 {
        return 0.0;
    }
    let crossings = samples
        .windows(2)
        .filter(|pair| (pair[0] >= 0.0) != (pair[1] >= 0.0))
        .count();
    crossings as f32 / (samples.len() - 1) as f32
}

fn estimate_voicing_periodicity(samples: &[f32], sample_rate: u32) -> f32 {
    if samples.len() < 32 || sample_rate == 0 {
        return 0.0;
    }

    let min_lag = ((sample_rate as f32 / 500.0).floor() as usize).max(2);
    let max_lag = ((sample_rate as f32 / 65.0).ceil() as usize).min(samples.len() / 2);
    if max_lag <= min_lag {
        return 0.0;
    }

    let mut best_corr = 0.0_f32;
    for lag in min_lag..=max_lag {
        let len = samples.len() - lag;
        let mut num = 0.0_f64;
        let mut energy_a = 0.0_f64;
        let mut energy_b = 0.0_f64;
        for i in 0..len {
            let a = f64::from(samples[i]);
            let b = f64::from(samples[i + lag]);
            num += a * b;
            energy_a += a * a;
            energy_b += b * b;
        }
        let denom = (energy_a * energy_b).sqrt();
        if denom > 1e-12 {
            let corr = (num / denom) as f32;
            if corr > best_corr {
                best_corr = corr;
            }
        }
    }

    best_corr.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;

    use chrono::{DateTime, TimeZone, Utc};

    use super::*;
    use crate::models::DecodedAudio;

    fn now_ts() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 26, 18, 0, 0)
            .single()
            .unwrap_or_else(Utc::now)
    }

    fn sine_wave(sample_rate: u32, duration_ms: u64, freq_hz: f32, amp: f32) -> Vec<f32> {
        let len = (u64::from(sample_rate) * duration_ms / 1000) as usize;
        (0..len)
            .map(|i| (TAU * freq_hz * i as f32 / sample_rate as f32).sin() * amp)
            .collect()
    }

    fn deterministic_frication(len: usize, amp: f32) -> Vec<f32> {
        let mut state = 0xCAFE_BABE_u32;
        let mut out = Vec::with_capacity(len);
        for _ in 0..len {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let norm = (state as f32 / u32::MAX as f32) * 2.0 - 1.0;
            out.push(norm * amp);
        }
        out
    }

    #[test]
    fn kana_normalization_and_mora_segmentation_handle_digraphs_and_katakana() {
        assert_eq!(normalize_to_hiragana("コーヒー"), "こーひー");
        assert_eq!(
            segment_kana_into_morae("しゃしん"),
            vec!["しゃ", "し", "ん"]
        );
        assert_eq!(
            segment_kana_into_morae("がっこう・コーヒー！"),
            vec!["が", "っ", "こ", "う", "こ", "ー", "ひ", "ー"]
        );
        assert_eq!(
            segment_kana_into_morae("ファックス"),
            vec!["ふぁ", "っ", "く", "す"]
        );
    }

    #[test]
    fn allophone_rules_cover_ga_row_assimilated_n_and_yotsugana_equivalence() {
        assert!(accepts_phonetic_realization("が", "[ga]", None));
        assert!(accepts_phonetic_realization("が", "[ŋa]", None));
        assert!(accepts_phonetic_realization("ご", "[ŋo]", None));

        for nasal_ipa in ["[m]", "[n]", "[ŋ]", "[ɴ]"] {
            assert!(accepts_phonetic_realization("ん", nasal_ipa, None));
        }

        assert!(are_kana_equivalent("じ", "ぢ"));
        assert!(are_kana_equivalent("ず", "づ"));
        assert!(are_kana_equivalent("ヂ", "じ"));
        assert!(are_kana_equivalent("ヅ", "ず"));
        assert!(!are_kana_equivalent("す", "ず"));
    }

    #[test]
    fn devoicing_rules_accept_voiceless_beat_and_reject_dropped_or_voiced_contexts() {
        let desu = segment_kana_into_morae("です");
        assert!(is_devoicing_environment(&desu, 1));
        assert!(accepts_devoiced_realization(&desu, 1, 140.0, 150.0, 0.03));
        assert!(!accepts_devoiced_realization(&desu, 1, 40.0, 150.0, 0.03));
        assert!(!accepts_devoiced_realization(&desu, 1, 140.0, 150.0, 0.001));

        let suki = segment_kana_into_morae("すき");
        assert!(is_devoicing_environment(&suki, 0));
        assert!(accepts_phonetic_realization("す", "[sɯ̥]", Some("き")));

        let suzu = segment_kana_into_morae("すず");
        assert!(!is_devoicing_environment(&suzu, 0));
        assert!(!accepts_phonetic_realization("す", "[sɯ̥]", Some("ず")));
    }

    struct StubMoraScorer {
        heard_sequence: Vec<Option<String>>,
    }

    impl MoraScorer for StubMoraScorer {
        fn name(&self) -> &'static str {
            "stub-test-scorer"
        }

        fn score_morae(&self, request: &MoraScoreRequest) -> Result<MoraEvaluation> {
            let morae = segment_kana_into_morae(&request.target_kana_sequence);
            let mut attempts = Vec::with_capacity(morae.len());
            let mut correct = 0_usize;
            for (idx, expected) in morae.iter().enumerate() {
                let heard = self.heard_sequence.get(idx).cloned().flatten();
                let attempt = normalize_mora_attempt(MoraAttempt {
                    card_id: request.card_id,
                    expected_kana: expected.clone(),
                    heard_kana: heard,
                    confidence: 0.9,
                    recorded_at: request.recorded_at,
                });
                if attempt.is_correct() {
                    correct += 1;
                }
                attempts.push(attempt);
            }
            Ok(MoraEvaluation {
                mora_accuracy_percent: (correct as f32 / morae.len().max(1) as f32) * 100.0,
                attempts,
                scorer_name: self.name().to_string(),
                accepted_allophone_notes: Vec::new(),
            })
        }
    }

    #[test]
    fn pluggable_mora_scorer_trait_supports_swappable_backends_and_top_five_aggregation() -> Result<()> {
        let scorer: Box<dyn MoraScorer> = Box::new(StubMoraScorer {
            heard_sequence: vec![Some("く".to_string()), Some("ぢ".to_string())],
        });
        let req = MoraScoreRequest {
            card_id: 7,
            target_kana_sequence: "くじ".to_string(),
            user_audio: DecodedAudio {
                samples: vec![0.1; 1600],
                sample_rate: 16_000,
                channels: 1,
            },
            reference_audio: None,
            recorded_at: now_ts(),
        };

        let eval = scorer.score_morae(&req)?;
        assert_eq!(eval.scorer_name, "stub-test-scorer");
        assert!((eval.mora_accuracy_percent - 100.0).abs() < 1e-4);
        assert_eq!(eval.attempts[1].heard_kana.as_deref(), Some("じ"));

        let stats_map = aggregate_kana_stats(&eval.attempts);
        assert_eq!(format_kana_breakdown(&stats_map["じ"]), "じ: 100% correct");
        assert!(top_mistakes_for_kana(&stats_map["じ"]).is_empty());
        Ok(())
    }

    #[test]
    fn heuristic_mora_scorer_accepts_devoiced_desu_and_flags_silence() -> Result<()> {
        let scorer = HeuristicAcousticMoraScorer;
        let sample_rate = 16_000_u32;

        // "です": voiced first mora (180 ms sine) + unvoiced frication second mora (180 ms noise).
        let mut desu_samples = sine_wave(sample_rate, 180, 180.0, 0.35);
        desu_samples.extend(deterministic_frication((sample_rate * 180 / 1000) as usize, 0.08));

        let eval = scorer.score_morae(&MoraScoreRequest {
            card_id: 11,
            target_kana_sequence: "です".to_string(),
            user_audio: DecodedAudio {
                samples: desu_samples,
                sample_rate,
                channels: 1,
            },
            reference_audio: None,
            recorded_at: now_ts(),
        })?;
        assert!((eval.mora_accuracy_percent - 100.0).abs() < 1e-4);
        assert!(
            eval.accepted_allophone_notes
                .iter()
                .any(|note| note.contains("devoiced 'す'"))
        );

        let silent_eval = scorer.score_morae(&MoraScoreRequest {
            card_id: 12,
            target_kana_sequence: "みず".to_string(),
            user_audio: DecodedAudio {
                samples: vec![0.0; 4_800],
                sample_rate,
                channels: 1,
            },
            reference_audio: None,
            recorded_at: now_ts(),
        })?;
        assert!((silent_eval.mora_accuracy_percent - 0.0).abs() < 1e-4);
        assert!(silent_eval.attempts.iter().all(|a| a.heard_kana.is_none()));
        Ok(())
    }
}

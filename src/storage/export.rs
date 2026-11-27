// SPDX-License-Identifier: MPL-2.0

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use chrono::SecondsFormat;

use crate::models::{
    CsvExportReport, ExportBundle, KanaStats, PitchErrorLogEntry, ReviewLogEntry,
};
use crate::storage::db::KotodamaDb;

/// Writes a serialized [`ExportBundle`] snapshot as pretty-printed JSON to `path`.
pub fn export_bundle_to_json(bundle: &ExportBundle, path: &Path) -> Result<()> {
    ensure_parent_dir(path, "JSON export")?;
    let serialized = serde_json::to_string_pretty(bundle)
        .context("Failed to serialize Kotodama export bundle to JSON")?;
    fs::write(path, serialized)
        .with_context(|| format!("Failed to write JSON export file at {}", path.display()))?;
    Ok(())
}

/// Builds a full snapshot from `db` and exports it as pretty-printed JSON to `path`.
pub fn export_db_to_json(db: &KotodamaDb, path: &Path) -> Result<ExportBundle> {
    let bundle = db.build_export_bundle()?;
    export_bundle_to_json(&bundle, path)?;
    Ok(bundle)
}

/// Writes review session history rows to a UTF-8 CSV file at `path`, returning row count.
pub fn export_review_history_csv(reviews: &[ReviewLogEntry], path: &Path) -> Result<usize> {
    ensure_parent_dir(path, "review history CSV")?;
    let mut writer = csv::Writer::from_path(path).with_context(|| {
        format!(
            "Failed to create review history CSV file at {}",
            path.display()
        )
    })?;

    writer
        .write_record([
            "review_id",
            "card_id",
            "deck_name",
            "word",
            "kana",
            "practice_mode",
            "mora_accuracy_percent",
            "pitch_error_percent",
            "dtw_distance",
            "combined_score_percent",
            "recommended_rating",
            "applied_rating",
            "was_manual_override",
            "duration_ms",
            "reviewed_at",
        ])
        .context("Failed to write review history CSV header")?;

    for entry in reviews {
        let review_id = entry
            .id
            .map_or_else(String::new, |id| id.to_string());
        let mora = entry
            .mora_accuracy_percent
            .map_or_else(String::new, |v| format!("{v:.2}"));
        let pitch_err = entry
            .pitch_error_percent
            .map_or_else(String::new, |v| format!("{v:.2}"));
        let dtw = entry
            .dtw_distance
            .map_or_else(String::new, |v| format!("{v:.4}"));

        writer
            .write_record([
                review_id,
                entry.card_id.to_string(),
                entry.deck_name.clone(),
                entry.word.clone(),
                entry.kana.clone(),
                entry.practice_mode.as_db_str().to_string(),
                mora,
                pitch_err,
                dtw,
                format!("{:.2}", entry.combined_score_percent),
                entry.recommended_rating.as_db_str().to_string(),
                entry.applied_rating.as_db_str().to_string(),
                entry.was_manual_override.to_string(),
                entry.duration_ms.to_string(),
                entry
                    .reviewed_at
                    .to_rfc3339_opts(SecondsFormat::Micros, true),
            ])
            .with_context(|| {
                format!(
                    "Failed to write review history CSV row for card {}",
                    entry.card_id
                )
            })?;
    }

    writer
        .flush()
        .with_context(|| format!("Failed to flush review history CSV at {}", path.display()))?;
    Ok(reviews.len())
}

/// Writes per-kana clarity and confusion pairs to a UTF-8 CSV file at `path`, returning row count.
pub fn export_kana_confusion_csv(kana_stats: &[KanaStats], path: &Path) -> Result<usize> {
    ensure_parent_dir(path, "kana confusion CSV")?;
    let mut writer = csv::Writer::from_path(path).with_context(|| {
        format!(
            "Failed to create kana confusion CSV file at {}",
            path.display()
        )
    })?;

    writer
        .write_record([
            "kana",
            "total_attempts",
            "correct_attempts",
            "accuracy_percent",
            "confused_with",
            "mistake_count",
            "mistake_percent",
        ])
        .context("Failed to write kana confusion CSV header")?;

    let mut rows_written = 0_usize;
    for stats in kana_stats {
        let accuracy = format!("{:.2}", stats.accuracy_percent());
        let mistakes = stats.top_mistakes(stats.mistake_counts.len());
        if mistakes.is_empty() {
            writer
                .write_record([
                    stats.kana.as_str(),
                    &stats.total_attempts.to_string(),
                    &stats.correct_attempts.to_string(),
                    &accuracy,
                    "",
                    "0",
                    "0.00",
                ])
                .with_context(|| {
                    format!("Failed to write kana summary CSV row for '{}'", stats.kana)
                })?;
            rows_written = rows_written.saturating_add(1);
            continue;
        }

        for mistake in mistakes {
            writer
                .write_record([
                    stats.kana.as_str(),
                    &stats.total_attempts.to_string(),
                    &stats.correct_attempts.to_string(),
                    &accuracy,
                    &mistake.heard_kana,
                    &mistake.count.to_string(),
                    &format!("{:.2}", mistake.percentage),
                ])
                .with_context(|| {
                    format!(
                        "Failed to write kana confusion CSV row for '{}' -> '{}'",
                        stats.kana, mistake.heard_kana
                    )
                })?;
            rows_written = rows_written.saturating_add(1);
        }
    }

    writer
        .flush()
        .with_context(|| format!("Failed to flush kana confusion CSV at {}", path.display()))?;
    Ok(rows_written)
}

/// Writes pitch accent error log rows to a UTF-8 CSV file at `path`, returning row count.
pub fn export_pitch_errors_csv(pitch_logs: &[PitchErrorLogEntry], path: &Path) -> Result<usize> {
    ensure_parent_dir(path, "pitch errors CSV")?;
    let mut writer = csv::Writer::from_path(path).with_context(|| {
        format!(
            "Failed to create pitch errors CSV file at {}",
            path.display()
        )
    })?;

    writer
        .write_record([
            "review_id",
            "card_id",
            "deck_name",
            "word",
            "kana",
            "pitch_error_percent",
            "dtw_distance",
            "reviewed_at",
        ])
        .context("Failed to write pitch errors CSV header")?;

    for entry in pitch_logs {
        writer
            .write_record([
                entry.review_id.to_string(),
                entry.card_id.to_string(),
                entry.deck_name.clone(),
                entry.word.clone(),
                entry.kana.clone(),
                format!("{:.2}", entry.pitch_error_percent),
                format!("{:.4}", entry.dtw_distance),
                entry
                    .reviewed_at
                    .to_rfc3339_opts(SecondsFormat::Micros, true),
            ])
            .with_context(|| {
                format!(
                    "Failed to write pitch error CSV row for review {}",
                    entry.review_id
                )
            })?;
    }

    writer
        .flush()
        .with_context(|| format!("Failed to flush pitch errors CSV at {}", path.display()))?;
    Ok(pitch_logs.len())
}

/// Exports review history, kana confusion matrix, and pitch error CSV files into `output_dir`.
pub fn export_db_to_csv_directory(db: &KotodamaDb, output_dir: &Path) -> Result<CsvExportReport> {
    fs::create_dir_all(output_dir).with_context(|| {
        format!(
            "Failed to create CSV export directory at {}",
            output_dir.display()
        )
    })?;

    let bundle = db.build_export_bundle()?;
    let reviews_path = output_dir.join("kotodama_review_history.csv");
    let kana_confusion_path = output_dir.join("kotodama_kana_confusion.csv");
    let pitch_errors_path = output_dir.join("kotodama_pitch_errors.csv");

    let review_rows = export_review_history_csv(&bundle.review_history, &reviews_path)?;
    let kana_rows = export_kana_confusion_csv(&bundle.kana_stats, &kana_confusion_path)?;
    let pitch_rows = export_pitch_errors_csv(&bundle.pitch_error_logs, &pitch_errors_path)?;

    Ok(CsvExportReport {
        reviews_path,
        kana_confusion_path,
        pitch_errors_path,
        review_rows,
        kana_rows,
        pitch_rows,
    })
}

fn ensure_parent_dir(path: &Path, label: &str) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create parent directory for {label} at {}",
                parent.display()
            )
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Duration, TimeZone, Utc};
    use tempfile::tempdir;

    use super::*;
    use crate::models::{DailyGoal, FsrsCardState, FsrsRating, MoraAttempt, PracticeMode};

    fn ts(year: i32, month: u32, day: u32, hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, hour, 0, 0)
            .single()
            .unwrap_or_else(Utc::now)
    }

    fn populated_db() -> Result<KotodamaDb> {
        let mut db = KotodamaDb::open_in_memory()?;
        let now = ts(2026, 11, 26, 16);

        let review = ReviewLogEntry {
            id: None,
            card_id: 77,
            deck_name: "Core 2k".to_string(),
            word: "靴".to_string(),
            kana: "くつ".to_string(),
            practice_mode: PracticeMode::CombinedShadowing,
            mora_accuracy_percent: Some(50.0),
            pitch_error_percent: Some(12.5),
            dtw_distance: Some(0.75),
            combined_score_percent: 68.75,
            recommended_rating: FsrsRating::Hard,
            applied_rating: FsrsRating::Hard,
            was_manual_override: false,
            duration_ms: 1_500,
            reviewed_at: now,
        };
        let state = FsrsCardState {
            card_id: 77,
            stability: 2.5,
            stability_fast: 2.4,
            difficulty: 5.2,
            elapsed_days: 0,
            scheduled_days: 3,
            reps: 1,
            lapses: 0,
            due_at: now + Duration::days(3),
            last_reviewed_at: Some(now),
            last_rating: Some(FsrsRating::Hard),
        };
        let attempts = vec![
            MoraAttempt {
                card_id: 77,
                expected_kana: "く".to_string(),
                heard_kana: Some("く".to_string()),
                confidence: 0.92,
                recorded_at: now,
            },
            MoraAttempt {
                card_id: 77,
                expected_kana: "つ".to_string(),
                heard_kana: Some("す".to_string()),
                confidence: 0.41,
                recorded_at: now,
            },
        ];

        db.commit_review_transaction(
            &review,
            &attempts,
            &state,
            now.date_naive(),
            1,
            DailyGoal::Cards(10),
        )?;
        Ok(db)
    }

    #[test]
    fn json_export_round_trips_full_bundle_with_japanese_utf8() -> Result<()> {
        let dir = tempdir()?;
        let json_path = dir.path().join("exports").join("kotodama.json");
        let db = populated_db()?;

        let exported = export_db_to_json(&db, &json_path)?;
        let raw = fs::read_to_string(&json_path)?;
        let decoded: ExportBundle = serde_json::from_str(&raw)?;

        assert_eq!(exported, decoded);
        assert_eq!(decoded.review_history.len(), 1);
        assert_eq!(decoded.review_history[0].word, "靴");
        assert_eq!(decoded.kana_stats.len(), 2);
        assert_eq!(decoded.pitch_error_logs.len(), 1);
        assert_eq!(decoded.fsrs_states.len(), 1);
        Ok(())
    }

    #[test]
    fn csv_directory_export_parses_headers_utf8_rows_and_empty_db() -> Result<()> {
        let empty_dir = tempdir()?;
        let empty_db = KotodamaDb::open_in_memory()?;
        let empty_report = export_db_to_csv_directory(&empty_db, empty_dir.path())?;
        assert_eq!(empty_report.review_rows, 0);
        assert_eq!(empty_report.kana_rows, 0);
        assert_eq!(empty_report.pitch_rows, 0);

        let mut empty_reader = csv::Reader::from_path(&empty_report.reviews_path)?;
        assert_eq!(empty_reader.headers()?.get(0), Some("review_id"));
        assert_eq!(empty_reader.records().count(), 0);

        let dir = tempdir()?;
        let db = populated_db()?;
        let report = export_db_to_csv_directory(&db, dir.path())?;
        assert_eq!(report.review_rows, 1);
        assert_eq!(report.kana_rows, 2);
        assert_eq!(report.pitch_rows, 1);

        let mut review_reader = csv::Reader::from_path(&report.reviews_path)?;
        let review_records: Vec<csv::StringRecord> = review_reader
            .records()
            .collect::<std::result::Result<Vec<_>, _>>()?;
        assert_eq!(review_records.len(), 1);
        assert_eq!(review_records[0].get(3), Some("靴"));
        assert_eq!(review_records[0].get(4), Some("くつ"));

        let mut kana_reader = csv::Reader::from_path(&report.kana_confusion_path)?;
        let kana_records: Vec<csv::StringRecord> = kana_reader
            .records()
            .collect::<std::result::Result<Vec<_>, _>>()?;
        assert_eq!(kana_records.len(), 2);
        // Sorted lowest accuracy first: "つ" (0%) then "く" (100%).
        assert_eq!(kana_records[0].get(0), Some("つ"));
        assert_eq!(kana_records[0].get(4), Some("す"));
        assert_eq!(kana_records[1].get(0), Some("く"));
        assert_eq!(kana_records[1].get(4), Some(""));

        let mut pitch_reader = csv::Reader::from_path(&report.pitch_errors_path)?;
        let pitch_records: Vec<csv::StringRecord> = pitch_reader
            .records()
            .collect::<std::result::Result<Vec<_>, _>>()?;
        assert_eq!(pitch_records.len(), 1);
        assert_eq!(pitch_records[0].get(5), Some("12.50"));
        Ok(())
    }
}

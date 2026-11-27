// SPDX-License-Identifier: MPL-2.0

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDate, SecondsFormat, Utc};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::models::{
    DailyAccuracyPoint, DailyGoal, DailyStreak, ExportBundle, FsrsCardState, FsrsRating, KanaStats,
    MoraAttempt, PitchErrorLogEntry, PracticeMode, ReviewLogEntry,
};

const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS daily_streak (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    current_streak_days INTEGER NOT NULL DEFAULT 0,
    longest_streak_days INTEGER NOT NULL DEFAULT 0,
    total_sessions INTEGER NOT NULL DEFAULT 0,
    cards_practiced_today INTEGER NOT NULL DEFAULT 0,
    minutes_practiced_today INTEGER NOT NULL DEFAULT 0,
    last_practice_date TEXT,
    daily_goal_kind TEXT NOT NULL DEFAULT 'Cards',
    daily_goal_value INTEGER NOT NULL DEFAULT 10
);

INSERT OR IGNORE INTO daily_streak (
    id,
    current_streak_days,
    longest_streak_days,
    total_sessions,
    cards_practiced_today,
    minutes_practiced_today,
    last_practice_date,
    daily_goal_kind,
    daily_goal_value
) VALUES (1, 0, 0, 0, 0, 0, NULL, 'Cards', 10);

CREATE TABLE IF NOT EXISTS fsrs_card_states (
    card_id INTEGER PRIMARY KEY,
    stability REAL NOT NULL,
    stability_fast REAL NOT NULL DEFAULT 0.0,
    difficulty REAL NOT NULL,
    elapsed_days INTEGER NOT NULL,
    scheduled_days INTEGER NOT NULL,
    reps INTEGER NOT NULL,
    lapses INTEGER NOT NULL,
    due_at TEXT NOT NULL,
    last_reviewed_at TEXT,
    last_rating TEXT
);

CREATE INDEX IF NOT EXISTS idx_fsrs_due_at ON fsrs_card_states (due_at);

CREATE TABLE IF NOT EXISTS review_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    card_id INTEGER NOT NULL,
    deck_name TEXT NOT NULL,
    word TEXT NOT NULL,
    kana TEXT NOT NULL,
    practice_mode TEXT NOT NULL,
    mora_accuracy_percent REAL,
    pitch_error_percent REAL,
    dtw_distance REAL,
    combined_score_percent REAL NOT NULL,
    recommended_rating TEXT NOT NULL,
    applied_rating TEXT NOT NULL,
    was_manual_override INTEGER NOT NULL,
    duration_ms INTEGER NOT NULL,
    reviewed_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_review_logs_card_id ON review_logs (card_id);
CREATE INDEX IF NOT EXISTS idx_review_logs_reviewed_at ON review_logs (reviewed_at);
CREATE INDEX IF NOT EXISTS idx_review_logs_deck_name ON review_logs (deck_name);

CREATE TABLE IF NOT EXISTS mora_attempts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    review_id INTEGER REFERENCES review_logs(id) ON DELETE CASCADE,
    card_id INTEGER NOT NULL,
    expected_kana TEXT NOT NULL,
    heard_kana TEXT,
    is_correct INTEGER NOT NULL,
    confidence REAL NOT NULL,
    recorded_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_mora_attempts_expected ON mora_attempts (expected_kana);

CREATE TABLE IF NOT EXISTS kana_confusion_counts (
    expected_kana TEXT NOT NULL,
    heard_label TEXT NOT NULL,
    count INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (expected_kana, heard_label)
);
"#;

/// Local SQLite store managing independent FSRS scheduling, streaks, review history, and kana stats.
#[allow(dead_code)]
pub struct KotodamaDb {
    conn: Connection,
}

#[allow(dead_code)]
impl KotodamaDb {
    /// Opens or creates a file-backed Kotodama SQLite database at `path` and ensures schema readiness.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create database directory at {}", parent.display())
            })?;
        }

        let conn = Connection::open(path)
            .with_context(|| format!("Failed to open Kotodama database at {}", path.display()))?;
        let db = Self { conn };
        db.configure_and_migrate()?;
        Ok(db)
    }

    /// Opens an isolated in-memory SQLite database initialized with Kotodama's schema.
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .context("Failed to open in-memory Kotodama SQLite database")?;
        let db = Self { conn };
        db.configure_and_migrate()?;
        Ok(db)
    }

    fn configure_and_migrate(&self) -> Result<()> {
        self.conn
            .execute_batch(
                "PRAGMA journal_mode = WAL;
                 PRAGMA foreign_keys = ON;
                 PRAGMA busy_timeout = 5000;",
            )
            .context("Failed to configure SQLite pragmas")?;

        self.conn
            .execute_batch(SCHEMA_SQL)
            .context("Failed to initialize Kotodama SQLite schema")?;

        self.ensure_stability_fast_column()?;
        Ok(())
    }

    fn ensure_stability_fast_column(&self) -> Result<()> {
        let mut stmt = self
            .conn
            .prepare("PRAGMA table_info(fsrs_card_states)")
            .context("Failed to inspect fsrs_card_states columns")?;
        let columns = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .context("Failed to read fsrs_card_states column names")?
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("Failed to collect fsrs_card_states column names")?;

        if !columns.iter().any(|col| col == "stability_fast") {
            self.conn
                .execute(
                    "ALTER TABLE fsrs_card_states ADD COLUMN stability_fast REAL NOT NULL DEFAULT 0.0",
                    [],
                )
                .context("Failed to add stability_fast column to fsrs_card_states")?;
        }
        Ok(())
    }

    /// Loads the persisted [`DailyStreak`] and active daily goal configuration.
    pub fn load_daily_streak(&self) -> Result<DailyStreak> {
        load_daily_streak_conn(&self.conn)
    }

    /// Persists the given [`DailyStreak`] record in SQLite.
    pub fn save_daily_streak(&self, streak: &DailyStreak) -> Result<()> {
        save_daily_streak_conn(&self.conn, streak)
    }

    /// Records completed practice activity for `date`, updating and persisting the daily streak.
    pub fn record_practice_activity(
        &self,
        date: NaiveDate,
        cards_completed: u32,
        minutes_spent: u32,
        daily_goal: DailyGoal,
    ) -> Result<DailyStreak> {
        let mut streak = self.load_daily_streak()?;
        streak.daily_goal = daily_goal;
        streak.record_practice(date, cards_completed, minutes_spent);
        self.save_daily_streak(&streak)?;
        Ok(streak)
    }

    /// Fetches the persisted [`FsrsCardState`] for `card_id`, if any reviews exist.
    pub fn get_fsrs_state(&self, card_id: i64) -> Result<Option<FsrsCardState>> {
        let row = self
            .conn
            .query_row(
                "SELECT card_id, stability, stability_fast, difficulty, elapsed_days,
                        scheduled_days, reps, lapses, due_at, last_reviewed_at, last_rating
                 FROM fsrs_card_states
                 WHERE card_id = ?1",
                params![card_id],
                row_to_fsrs_state,
            )
            .optional()
            .with_context(|| format!("Failed to load FSRS state for card {card_id}"))?;

        row.transpose()
    }

    /// Inserts or updates a card's independent [`FsrsCardState`].
    pub fn upsert_fsrs_state(&self, state: &FsrsCardState) -> Result<()> {
        upsert_fsrs_state_conn(&self.conn, state)
    }

    /// Returns all persisted [`FsrsCardState`] rows ordered by `due_at` ascending, then `card_id`.
    pub fn list_fsrs_states(&self) -> Result<Vec<FsrsCardState>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT card_id, stability, stability_fast, difficulty, elapsed_days,
                        scheduled_days, reps, lapses, due_at, last_reviewed_at, last_rating
                 FROM fsrs_card_states
                 ORDER BY card_id ASC",
            )
            .context("Failed to prepare FSRS state listing query")?;

        let mut states = stmt
            .query_map([], row_to_fsrs_state)
            .context("Failed to query FSRS states")?
            .map(|res| {
                res.context("Failed to read FSRS state row")?
                    .context("Failed to parse FSRS state row")
            })
            .collect::<Result<Vec<_>>>()?;

        states.sort_by(|left, right| {
            left.due_at
                .cmp(&right.due_at)
                .then_with(|| left.card_id.cmp(&right.card_id))
        });
        Ok(states)
    }

    /// Orders candidate card IDs by due priority: overdue/due cards earliest first, then unseen cards.
    pub fn list_due_card_ids(
        &self,
        candidate_card_ids: &[i64],
        now: DateTime<Utc>,
    ) -> Result<Vec<i64>> {
        let states_by_id: HashMap<i64, FsrsCardState> = self
            .list_fsrs_states()?
            .into_iter()
            .map(|state| (state.card_id, state))
            .collect();

        let mut seen = HashSet::with_capacity(candidate_card_ids.len());
        let mut due_cards: Vec<(DateTime<Utc>, i64)> = Vec::new();
        let mut unseen_cards: Vec<i64> = Vec::new();

        for &card_id in candidate_card_ids {
            if !seen.insert(card_id) {
                continue;
            }
            match states_by_id.get(&card_id) {
                Some(state) => {
                    if state.is_due(now) {
                        due_cards.push((state.due_at, card_id));
                    }
                }
                None => unseen_cards.push(card_id),
            }
        }

        due_cards.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));

        let mut ordered = Vec::with_capacity(due_cards.len() + unseen_cards.len());
        ordered.extend(due_cards.into_iter().map(|(_, card_id)| card_id));
        ordered.extend(unseen_cards);
        Ok(ordered)
    }

    /// Inserts a [`ReviewLogEntry`] and returns the assigned SQLite row ID.
    pub fn insert_review_log(&self, entry: &ReviewLogEntry) -> Result<i64> {
        insert_review_log_conn(&self.conn, entry)
    }

    /// Lists review logs ordered chronologically from newest to oldest, optionally capped by `limit`.
    pub fn list_review_logs(&self, limit: Option<usize>) -> Result<Vec<ReviewLogEntry>> {
        let sql = "SELECT id, card_id, deck_name, word, kana, practice_mode,
                          mora_accuracy_percent, pitch_error_percent, dtw_distance,
                          combined_score_percent, recommended_rating, applied_rating,
                          was_manual_override, duration_ms, reviewed_at
                   FROM review_logs
                   ORDER BY reviewed_at DESC, id DESC";

        let mut stmt = self
            .conn
            .prepare(sql)
            .context("Failed to prepare review logs query")?;

        let mut logs = stmt
            .query_map([], row_to_review_log)
            .context("Failed to query review logs")?
            .map(|res| {
                res.context("Failed to read review log row")?
                    .context("Failed to parse review log row")
            })
            .collect::<Result<Vec<_>>>()?;

        logs.sort_by(|left, right| {
            right
                .reviewed_at
                .cmp(&left.reviewed_at)
                .then_with(|| right.id.cmp(&left.id))
        });

        if let Some(max_rows) = limit {
            logs.truncate(max_rows);
        }
        Ok(logs)
    }

    /// Returns pitch error history for a specific Anki `card_id` ordered newest first.
    pub fn list_card_pitch_history(&self, card_id: i64) -> Result<Vec<PitchErrorLogEntry>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, card_id, deck_name, word, kana, pitch_error_percent,
                        COALESCE(dtw_distance, 0.0), reviewed_at
                 FROM review_logs
                 WHERE card_id = ?1 AND pitch_error_percent IS NOT NULL
                 ORDER BY reviewed_at DESC, id DESC",
            )
            .with_context(|| format!("Failed to prepare pitch history query for card {card_id}"))?;

        let mut entries = stmt
            .query_map(params![card_id], row_to_pitch_log)
            .with_context(|| format!("Failed to query pitch history for card {card_id}"))?
            .map(|res| {
                res.context("Failed to read pitch error row")?
                    .context("Failed to parse pitch error row")
            })
            .collect::<Result<Vec<_>>>()?;

        entries.sort_by(|left, right| {
            right
                .reviewed_at
                .cmp(&left.reviewed_at)
                .then_with(|| right.review_id.cmp(&left.review_id))
        });
        Ok(entries)
    }

    /// Lists all review records that include pitch accent error scores, newest first.
    pub fn list_pitch_error_logs(&self) -> Result<Vec<PitchErrorLogEntry>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, card_id, deck_name, word, kana, pitch_error_percent,
                        COALESCE(dtw_distance, 0.0), reviewed_at
                 FROM review_logs
                 WHERE pitch_error_percent IS NOT NULL
                 ORDER BY reviewed_at DESC, id DESC",
            )
            .context("Failed to prepare pitch error logs query")?;

        let mut entries = stmt
            .query_map([], row_to_pitch_log)
            .context("Failed to query pitch error logs")?
            .map(|res| {
                res.context("Failed to read pitch error row")?
                    .context("Failed to parse pitch error row")
            })
            .collect::<Result<Vec<_>>>()?;

        entries.sort_by(|left, right| {
            right
                .reviewed_at
                .cmp(&left.reviewed_at)
                .then_with(|| right.review_id.cmp(&left.review_id))
        });
        Ok(entries)
    }

    /// Returns each card's most recent pitch error percentage keyed by `card_id`.
    pub fn latest_pitch_errors_by_card(&self) -> Result<BTreeMap<i64, f32>> {
        let mut latest: BTreeMap<i64, f32> = BTreeMap::new();
        for entry in self.list_pitch_error_logs()? {
            latest.entry(entry.card_id).or_insert(entry.pitch_error_percent);
        }
        Ok(latest)
    }

    /// Aggregates daily average mora accuracy, pitch error, and combined score sorted chronologically.
    pub fn daily_accuracy_series(&self) -> Result<Vec<DailyAccuracyPoint>> {
        #[derive(Default)]
        struct DayAccumulator {
            review_count: u64,
            mora_sum: f64,
            mora_count: u64,
            pitch_sum: f64,
            pitch_count: u64,
            combined_sum: f64,
        }

        let mut by_date: BTreeMap<NaiveDate, DayAccumulator> = BTreeMap::new();
        for entry in self.list_review_logs(None)? {
            let date = entry.reviewed_at.date_naive();
            let acc = by_date.entry(date).or_default();
            acc.review_count = acc.review_count.saturating_add(1);
            acc.combined_sum += f64::from(entry.combined_score_percent);
            if let Some(mora) = entry.mora_accuracy_percent {
                acc.mora_sum += f64::from(mora);
                acc.mora_count = acc.mora_count.saturating_add(1);
            }
            if let Some(pitch) = entry.pitch_error_percent {
                acc.pitch_sum += f64::from(pitch);
                acc.pitch_count = acc.pitch_count.saturating_add(1);
            }
        }

        let series = by_date
            .into_iter()
            .map(|(date, acc)| DailyAccuracyPoint {
                date,
                review_count: acc.review_count,
                avg_mora_accuracy_percent: if acc.mora_count > 0 {
                    Some((acc.mora_sum / acc.mora_count as f64) as f32)
                } else {
                    None
                },
                avg_pitch_error_percent: if acc.pitch_count > 0 {
                    Some((acc.pitch_sum / acc.pitch_count as f64) as f32)
                } else {
                    None
                },
                avg_combined_score_percent: (acc.combined_sum / acc.review_count.max(1) as f64)
                    as f32,
            })
            .collect();
        Ok(series)
    }

    /// Atomically persists mora attempts and updates cumulative per-kana confusion counts.
    pub fn record_mora_attempts(
        &mut self,
        review_id: Option<i64>,
        attempts: &[MoraAttempt],
    ) -> Result<()> {
        if attempts.is_empty() {
            return Ok(());
        }
        let tx = self
            .conn
            .transaction()
            .context("Failed to start SQLite transaction for mora attempts")?;
        record_mora_attempts_tx(&tx, review_id, attempts)?;
        tx.commit()
            .context("Failed to commit mora attempts SQLite transaction")?;
        Ok(())
    }

    /// Loads aggregated clarity and confusion statistics for a single target `kana`.
    pub fn get_kana_stats(&self, kana: &str) -> Result<Option<KanaStats>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT heard_label, count
                 FROM kana_confusion_counts
                 WHERE expected_kana = ?1",
            )
            .with_context(|| format!("Failed to prepare kana stats query for '{kana}'"))?;

        let rows = stmt
            .query_map(params![kana], |row| {
                let heard_label: String = row.get(0)?;
                let count = u64::try_from(row.get::<_, i64>(1)?).unwrap_or(0);
                Ok((heard_label, count))
            })
            .with_context(|| format!("Failed to query kana confusion counts for '{kana}'"))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .with_context(|| format!("Failed to collect kana confusion rows for '{kana}'"))?;

        if rows.is_empty() {
            return Ok(None);
        }

        let mut stats = KanaStats::new(kana);
        for (heard_label, count) in rows {
            stats.total_attempts = stats.total_attempts.saturating_add(count);
            if heard_label == kana {
                stats.correct_attempts = stats.correct_attempts.saturating_add(count);
            } else if count > 0 {
                let entry = stats.mistake_counts.entry(heard_label).or_insert(0);
                *entry = entry.saturating_add(count);
            }
        }

        Ok(Some(stats))
    }

    /// Lists all tracked kana clarity records sorted by lowest accuracy first for targeted drilling.
    pub fn list_all_kana_stats(&self) -> Result<Vec<KanaStats>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT expected_kana, heard_label, count
                 FROM kana_confusion_counts
                 ORDER BY expected_kana ASC",
            )
            .context("Failed to prepare full kana confusion query")?;

        let mut by_kana: BTreeMap<String, KanaStats> = BTreeMap::new();
        let rows = stmt
            .query_map([], |row| {
                let expected_kana: String = row.get(0)?;
                let heard_label: String = row.get(1)?;
                let count = u64::try_from(row.get::<_, i64>(2)?).unwrap_or(0);
                Ok((expected_kana, heard_label, count))
            })
            .context("Failed to query full kana confusion matrix")?;

        for row in rows {
            let (expected_kana, heard_label, count) =
                row.context("Failed to read kana confusion row")?;
            let stats = by_kana
                .entry(expected_kana.clone())
                .or_insert_with(|| KanaStats::new(&expected_kana));
            stats.total_attempts = stats.total_attempts.saturating_add(count);
            if heard_label == expected_kana {
                stats.correct_attempts = stats.correct_attempts.saturating_add(count);
            } else if count > 0 {
                let entry = stats.mistake_counts.entry(heard_label).or_insert(0);
                *entry = entry.saturating_add(count);
            }
        }

        let mut list: Vec<KanaStats> = by_kana.into_values().collect();
        list.sort_by(|left, right| {
            left.accuracy_percent()
                .total_cmp(&right.accuracy_percent())
                .then_with(|| right.total_attempts.cmp(&left.total_attempts))
                .then_with(|| left.kana.cmp(&right.kana))
        });
        Ok(list)
    }

    /// Commits a review log, its mora attempts, updated FSRS state, and daily streak in one transaction.
    pub fn commit_review_transaction(
        &mut self,
        review: &ReviewLogEntry,
        attempts: &[MoraAttempt],
        next_fsrs_state: &FsrsCardState,
        streak_date: NaiveDate,
        minutes_spent: u32,
        daily_goal: DailyGoal,
    ) -> Result<(i64, DailyStreak)> {
        let tx = self
            .conn
            .transaction()
            .context("Failed to start atomic review commit transaction")?;

        let review_id = insert_review_log_conn(&tx, review)?;
        record_mora_attempts_tx(&tx, Some(review_id), attempts)?;
        upsert_fsrs_state_conn(&tx, next_fsrs_state)?;

        let mut streak = load_daily_streak_conn(&tx)?;
        streak.daily_goal = daily_goal;
        streak.record_practice(streak_date, 1, minutes_spent);
        save_daily_streak_conn(&tx, &streak)?;

        tx.commit()
            .context("Failed to commit review SQLite transaction")?;
        Ok((review_id, streak))
    }

    /// Collects streaks, review history, kana stats, pitch logs, and FSRS states into an [`ExportBundle`].
    pub fn build_export_bundle(&self) -> Result<ExportBundle> {
        Ok(ExportBundle {
            exported_at: Utc::now(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            streak: self.load_daily_streak()?,
            review_history: self.list_review_logs(None)?,
            kana_stats: self.list_all_kana_stats()?,
            pitch_error_logs: self.list_pitch_error_logs()?,
            fsrs_states: self.list_fsrs_states()?,
        })
    }
}

fn load_daily_streak_conn(conn: &Connection) -> Result<DailyStreak> {
    conn.query_row(
        "SELECT current_streak_days, longest_streak_days, total_sessions,
                cards_practiced_today, minutes_practiced_today, last_practice_date,
                daily_goal_kind, daily_goal_value
         FROM daily_streak
         WHERE id = 1",
        [],
        |row| {
            let current_streak_days = u32::try_from(row.get::<_, i64>(0)?).unwrap_or(0);
            let longest_streak_days = u32::try_from(row.get::<_, i64>(1)?).unwrap_or(0);
            let total_sessions = u64::try_from(row.get::<_, i64>(2)?).unwrap_or(0);
            let cards_practiced_today = u32::try_from(row.get::<_, i64>(3)?).unwrap_or(0);
            let minutes_practiced_today = u32::try_from(row.get::<_, i64>(4)?).unwrap_or(0);
            let last_practice_raw: Option<String> = row.get(5)?;
            let goal_kind: String = row.get(6)?;
            let goal_value = u32::try_from(row.get::<_, i64>(7)?).unwrap_or(10);

            Ok((
                current_streak_days,
                longest_streak_days,
                total_sessions,
                cards_practiced_today,
                minutes_practiced_today,
                last_practice_raw,
                goal_kind,
                goal_value,
            ))
        },
    )
    .context("Failed to read daily_streak row from SQLite")
    .and_then(
        |(
            current_streak_days,
            longest_streak_days,
            total_sessions,
            cards_practiced_today,
            minutes_practiced_today,
            last_practice_raw,
            goal_kind,
            goal_value,
        )| {
            let last_practice_date = last_practice_raw
                .as_deref()
                .map(parse_naive_date)
                .transpose()?;
            let daily_goal = DailyGoal::from_parts(&goal_kind, goal_value)?;
            Ok(DailyStreak {
                current_streak_days,
                longest_streak_days,
                total_sessions,
                cards_practiced_today,
                minutes_practiced_today,
                last_practice_date,
                daily_goal,
            })
        },
    )
}

fn save_daily_streak_conn(conn: &Connection, streak: &DailyStreak) -> Result<()> {
    let last_date = streak
        .last_practice_date
        .map(|date| date.format("%Y-%m-%d").to_string());
    conn.execute(
        "UPDATE daily_streak
         SET current_streak_days = ?1,
             longest_streak_days = ?2,
             total_sessions = ?3,
             cards_practiced_today = ?4,
             minutes_practiced_today = ?5,
             last_practice_date = ?6,
             daily_goal_kind = ?7,
             daily_goal_value = ?8
         WHERE id = 1",
        params![
            i64::from(streak.current_streak_days),
            i64::from(streak.longest_streak_days),
            i64::try_from(streak.total_sessions).unwrap_or(i64::MAX),
            i64::from(streak.cards_practiced_today),
            i64::from(streak.minutes_practiced_today),
            last_date,
            streak.daily_goal.kind_str(),
            i64::from(streak.daily_goal.target_value()),
        ],
    )
    .context("Failed to update daily_streak row in SQLite")?;
    Ok(())
}

fn upsert_fsrs_state_conn(conn: &Connection, state: &FsrsCardState) -> Result<()> {
    let due_at = format_utc(state.due_at);
    let last_reviewed_at = state.last_reviewed_at.map(format_utc);
    let last_rating = state.last_rating.map(FsrsRating::as_db_str);

    conn.execute(
        "INSERT INTO fsrs_card_states (
             card_id, stability, stability_fast, difficulty, elapsed_days,
             scheduled_days, reps, lapses, due_at, last_reviewed_at, last_rating
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
         ON CONFLICT(card_id) DO UPDATE SET
             stability = excluded.stability,
             stability_fast = excluded.stability_fast,
             difficulty = excluded.difficulty,
             elapsed_days = excluded.elapsed_days,
             scheduled_days = excluded.scheduled_days,
             reps = excluded.reps,
             lapses = excluded.lapses,
             due_at = excluded.due_at,
             last_reviewed_at = excluded.last_reviewed_at,
             last_rating = excluded.last_rating",
        params![
            state.card_id,
            state.stability,
            state.stability_fast,
            state.difficulty,
            i64::from(state.elapsed_days),
            i64::from(state.scheduled_days),
            i64::from(state.reps),
            i64::from(state.lapses),
            due_at,
            last_reviewed_at,
            last_rating,
        ],
    )
    .with_context(|| format!("Failed to upsert FSRS state for card {}", state.card_id))?;
    Ok(())
}

fn insert_review_log_conn(conn: &Connection, entry: &ReviewLogEntry) -> Result<i64> {
    conn.execute(
        "INSERT INTO review_logs (
             card_id, deck_name, word, kana, practice_mode,
             mora_accuracy_percent, pitch_error_percent, dtw_distance,
             combined_score_percent, recommended_rating, applied_rating,
             was_manual_override, duration_ms, reviewed_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            entry.card_id,
            entry.deck_name,
            entry.word,
            entry.kana,
            entry.practice_mode.as_db_str(),
            entry.mora_accuracy_percent.map(f64::from),
            entry.pitch_error_percent.map(f64::from),
            entry.dtw_distance.map(f64::from),
            f64::from(entry.combined_score_percent),
            entry.recommended_rating.as_db_str(),
            entry.applied_rating.as_db_str(),
            i64::from(entry.was_manual_override),
            i64::try_from(entry.duration_ms).unwrap_or(i64::MAX),
            format_utc(entry.reviewed_at),
        ],
    )
    .with_context(|| format!("Failed to insert review log for card {}", entry.card_id))?;

    Ok(conn.last_insert_rowid())
}

fn record_mora_attempts_tx(
    tx: &Transaction<'_>,
    review_id: Option<i64>,
    attempts: &[MoraAttempt],
) -> Result<()> {
    if attempts.is_empty() {
        return Ok(());
    }

    let mut insert_attempt = tx
        .prepare(
            "INSERT INTO mora_attempts (
                 review_id, card_id, expected_kana, heard_kana, is_correct, confidence, recorded_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )
        .context("Failed to prepare mora_attempts insert statement")?;

    let mut upsert_confusion = tx
        .prepare(
            "INSERT INTO kana_confusion_counts (expected_kana, heard_label, count)
             VALUES (?1, ?2, 1)
             ON CONFLICT(expected_kana, heard_label) DO UPDATE SET
                 count = kana_confusion_counts.count + 1",
        )
        .context("Failed to prepare kana_confusion_counts upsert statement")?;

    for attempt in attempts {
        let is_correct = attempt.is_correct();
        let heard_label = if is_correct {
            attempt.expected_kana.as_str()
        } else {
            attempt.heard_label()
        };

        insert_attempt
            .execute(params![
                review_id,
                attempt.card_id,
                attempt.expected_kana,
                attempt.heard_kana.as_deref(),
                i64::from(is_correct),
                f64::from(attempt.confidence),
                format_utc(attempt.recorded_at),
            ])
            .with_context(|| {
                format!(
                    "Failed to insert mora attempt for kana '{}'",
                    attempt.expected_kana
                )
            })?;

        upsert_confusion
            .execute(params![attempt.expected_kana, heard_label])
            .with_context(|| {
                format!(
                    "Failed to update kana confusion count for '{}' -> '{}'",
                    attempt.expected_kana, heard_label
                )
            })?;
    }

    Ok(())
}

fn row_to_fsrs_state(row: &rusqlite::Row<'_>) -> rusqlite::Result<Result<FsrsCardState>> {
    let card_id: i64 = row.get(0)?;
    let stability: f64 = row.get(1)?;
    let stability_fast: f64 = row.get(2)?;
    let difficulty: f64 = row.get(3)?;
    let elapsed_days = u32::try_from(row.get::<_, i64>(4)?).unwrap_or(0);
    let scheduled_days = u32::try_from(row.get::<_, i64>(5)?).unwrap_or(0);
    let reps = u32::try_from(row.get::<_, i64>(6)?).unwrap_or(0);
    let lapses = u32::try_from(row.get::<_, i64>(7)?).unwrap_or(0);
    let due_raw: String = row.get(8)?;
    let last_reviewed_raw: Option<String> = row.get(9)?;
    let last_rating_raw: Option<String> = row.get(10)?;

    Ok((|| {
        let due_at = parse_utc(&due_raw)?;
        let last_reviewed_at = last_reviewed_raw.as_deref().map(parse_utc).transpose()?;
        let last_rating = last_rating_raw
            .as_deref()
            .map(FsrsRating::from_db_str)
            .transpose()?;

        Ok(FsrsCardState {
            card_id,
            stability,
            stability_fast,
            difficulty,
            elapsed_days,
            scheduled_days,
            reps,
            lapses,
            due_at,
            last_reviewed_at,
            last_rating,
        })
    })())
}

fn row_to_review_log(row: &rusqlite::Row<'_>) -> rusqlite::Result<Result<ReviewLogEntry>> {
    let id: i64 = row.get(0)?;
    let card_id: i64 = row.get(1)?;
    let deck_name: String = row.get(2)?;
    let word: String = row.get(3)?;
    let kana: String = row.get(4)?;
    let mode_raw: String = row.get(5)?;
    let mora_accuracy_percent = row.get::<_, Option<f64>>(6)?.map(|v| v as f32);
    let pitch_error_percent = row.get::<_, Option<f64>>(7)?.map(|v| v as f32);
    let dtw_distance = row.get::<_, Option<f64>>(8)?.map(|v| v as f32);
    let combined_score_percent = row.get::<_, f64>(9)? as f32;
    let recommended_raw: String = row.get(10)?;
    let applied_raw: String = row.get(11)?;
    let was_manual_override = row.get::<_, i64>(12)? != 0;
    let duration_ms = u64::try_from(row.get::<_, i64>(13)?).unwrap_or(0);
    let reviewed_raw: String = row.get(14)?;

    Ok((|| {
        Ok(ReviewLogEntry {
            id: Some(id),
            card_id,
            deck_name,
            word,
            kana,
            practice_mode: PracticeMode::from_db_str(&mode_raw)?,
            mora_accuracy_percent,
            pitch_error_percent,
            dtw_distance,
            combined_score_percent,
            recommended_rating: FsrsRating::from_db_str(&recommended_raw)?,
            applied_rating: FsrsRating::from_db_str(&applied_raw)?,
            was_manual_override,
            duration_ms,
            reviewed_at: parse_utc(&reviewed_raw)?,
        })
    })())
}

fn row_to_pitch_log(row: &rusqlite::Row<'_>) -> rusqlite::Result<Result<PitchErrorLogEntry>> {
    let review_id: i64 = row.get(0)?;
    let card_id: i64 = row.get(1)?;
    let deck_name: String = row.get(2)?;
    let word: String = row.get(3)?;
    let kana: String = row.get(4)?;
    let pitch_error_percent = row.get::<_, f64>(5)? as f32;
    let dtw_distance = row.get::<_, f64>(6)? as f32;
    let reviewed_raw: String = row.get(7)?;

    Ok((|| {
        Ok(PitchErrorLogEntry {
            review_id,
            card_id,
            deck_name,
            word,
            kana,
            pitch_error_percent,
            dtw_distance,
            reviewed_at: parse_utc(&reviewed_raw)?,
        })
    })())
}

fn format_utc(dt: DateTime<Utc>) -> String {
    dt.to_rfc3339_opts(SecondsFormat::Micros, true)
}

fn parse_utc(raw: &str) -> Result<DateTime<Utc>> {
    let parsed = DateTime::parse_from_rfc3339(raw)
        .with_context(|| format!("Invalid UTC timestamp '{raw}' in SQLite"))?;
    Ok(parsed.with_timezone(&Utc))
}

fn parse_naive_date(raw: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(raw, "%Y-%m-%d")
        .with_context(|| format!("Invalid calendar date '{raw}' in SQLite"))
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone};
    use tempfile::tempdir;

    use super::*;

    fn ts(year: i32, month: u32, day: u32, hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, hour, 0, 0)
            .single()
            .unwrap_or_else(Utc::now)
    }

    #[test]
    fn file_backed_db_persists_streaks_across_reopen() -> Result<()> {
        let dir = tempdir()?;
        let db_path = dir.path().join("nested").join("kotodama.db");
        let day1 = NaiveDate::from_ymd_opt(2026, 11, 24).unwrap_or(NaiveDate::MIN);
        let day2 = NaiveDate::from_ymd_opt(2026, 11, 25).unwrap_or(NaiveDate::MIN);
        let day5 = NaiveDate::from_ymd_opt(2026, 11, 28).unwrap_or(NaiveDate::MIN);

        {
            let db = KotodamaDb::open(&db_path)?;
            let initial = db.load_daily_streak()?;
            assert_eq!(initial.current_streak_days, 0);

            let s1 = db.record_practice_activity(day1, 5, 3, DailyGoal::Cards(10))?;
            assert_eq!(s1.current_streak_days, 1);
            assert_eq!(s1.cards_practiced_today, 5);

            let s1_same = db.record_practice_activity(day1, 5, 2, DailyGoal::Cards(10))?;
            assert_eq!(s1_same.current_streak_days, 1);
            assert_eq!(s1_same.cards_practiced_today, 10);

            let s2 = db.record_practice_activity(day2, 4, 2, DailyGoal::Cards(10))?;
            assert_eq!(s2.current_streak_days, 2);
            assert_eq!(s2.longest_streak_days, 2);
        }

        let reopened = KotodamaDb::open(&db_path)?;
        let persisted = reopened.load_daily_streak()?;
        assert_eq!(persisted.current_streak_days, 2);
        assert_eq!(persisted.longest_streak_days, 2);
        assert_eq!(persisted.total_sessions, 3);

        let after_gap = reopened.record_practice_activity(day5, 2, 1, DailyGoal::Minutes(15))?;
        assert_eq!(after_gap.current_streak_days, 1);
        assert_eq!(after_gap.longest_streak_days, 2);
        assert_eq!(after_gap.daily_goal, DailyGoal::Minutes(15));
        Ok(())
    }

    #[test]
    fn fsrs_state_upsert_lookup_and_due_queue_ordering() -> Result<()> {
        let db = KotodamaDb::open_in_memory()?;
        let now = ts(2026, 11, 26, 12);

        let overdue = FsrsCardState {
            card_id: 10,
            stability: 4.2,
            stability_fast: 3.8,
            difficulty: 5.1,
            elapsed_days: 2,
            scheduled_days: 2,
            reps: 3,
            lapses: 0,
            due_at: now - Duration::hours(6),
            last_reviewed_at: Some(now - Duration::days(2)),
            last_rating: Some(FsrsRating::Good),
        };
        let due_now = FsrsCardState {
            card_id: 20,
            stability: 1.5,
            stability_fast: 1.5,
            difficulty: 6.0,
            elapsed_days: 1,
            scheduled_days: 1,
            reps: 1,
            lapses: 1,
            due_at: now,
            last_reviewed_at: Some(now - Duration::days(1)),
            last_rating: Some(FsrsRating::Hard),
        };
        let future = FsrsCardState {
            card_id: 30,
            stability: 14.0,
            stability_fast: 14.0,
            difficulty: 4.0,
            elapsed_days: 5,
            scheduled_days: 14,
            reps: 4,
            lapses: 0,
            due_at: now + Duration::days(7),
            last_reviewed_at: Some(now - Duration::days(1)),
            last_rating: Some(FsrsRating::Easy),
        };

        db.upsert_fsrs_state(&due_now)?;
        db.upsert_fsrs_state(&future)?;
        db.upsert_fsrs_state(&overdue)?;

        let loaded = db.get_fsrs_state(10)?.expect("card 10 should exist");
        assert_eq!(loaded, overdue);
        assert!(db.get_fsrs_state(999)?.is_none());

        let ordered = db.list_due_card_ids(&[30, 40, 20, 10, 40, 50], now)?;
        assert_eq!(ordered, vec![10, 20, 40, 50]);
        Ok(())
    }

    #[test]
    fn mora_attempts_aggregate_into_top_five_confusion_matrix() -> Result<()> {
        let mut db = KotodamaDb::open_in_memory()?;
        let now = ts(2026, 11, 26, 15);
        let mut attempts = Vec::new();

        for _ in 0..82 {
            attempts.push(MoraAttempt {
                card_id: 1,
                expected_kana: "つ".to_string(),
                heard_kana: Some("つ".to_string()),
                confidence: 0.94,
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
                attempts.push(MoraAttempt {
                    card_id: 1,
                    expected_kana: "つ".to_string(),
                    heard_kana: heard.map(str::to_string),
                    confidence: 0.42,
                    recorded_at: now,
                });
            }
        }

        db.record_mora_attempts(None, &attempts)?;

        let tsu = db.get_kana_stats("つ")?.expect("つ stats should exist");
        assert_eq!(tsu.total_attempts, 100);
        assert_eq!(tsu.correct_attempts, 82);
        assert_eq!(
            tsu.format_breakdown(),
            "つ: 82% correct — 6% unknown, 5% す, 4% ず, 3% すう"
        );
        assert!(db.get_kana_stats("ぬ")?.is_none());
        Ok(())
    }

    #[test]
    fn commit_review_transaction_persists_all_analytics_and_export_bundle() -> Result<()> {
        let mut db = KotodamaDb::open_in_memory()?;
        let t1 = ts(2026, 11, 25, 9);
        let t2 = ts(2026, 11, 26, 11);

        let r1 = ReviewLogEntry {
            id: None,
            card_id: 101,
            deck_name: "Core 2k".to_string(),
            word: "靴".to_string(),
            kana: "くつ".to_string(),
            practice_mode: PracticeMode::CombinedShadowing,
            mora_accuracy_percent: Some(50.0),
            pitch_error_percent: Some(20.0),
            dtw_distance: Some(1.2),
            combined_score_percent: 65.0,
            recommended_rating: FsrsRating::Hard,
            applied_rating: FsrsRating::Hard,
            was_manual_override: false,
            duration_ms: 1_800,
            reviewed_at: t1,
        };
        let state1 = FsrsCardState {
            card_id: 101,
            stability: 2.1,
            stability_fast: 2.0,
            difficulty: 5.5,
            elapsed_days: 0,
            scheduled_days: 2,
            reps: 1,
            lapses: 0,
            due_at: t1 + Duration::days(2),
            last_reviewed_at: Some(t1),
            last_rating: Some(FsrsRating::Hard),
        };
        let attempts1 = vec![
            MoraAttempt {
                card_id: 101,
                expected_kana: "く".to_string(),
                heard_kana: Some("く".to_string()),
                confidence: 0.9,
                recorded_at: t1,
            },
            MoraAttempt {
                card_id: 101,
                expected_kana: "つ".to_string(),
                heard_kana: Some("す".to_string()),
                confidence: 0.45,
                recorded_at: t1,
            },
        ];

        let (id1, streak1) = db.commit_review_transaction(
            &r1,
            &attempts1,
            &state1,
            t1.date_naive(),
            2,
            DailyGoal::Cards(10),
        )?;
        assert!(id1 > 0);
        assert_eq!(streak1.current_streak_days, 1);

        let r2 = ReviewLogEntry {
            id: None,
            card_id: 101,
            deck_name: "Core 2k".to_string(),
            word: "靴".to_string(),
            kana: "くつ".to_string(),
            practice_mode: PracticeMode::PitchAccent,
            mora_accuracy_percent: None,
            pitch_error_percent: Some(10.0),
            dtw_distance: Some(0.6),
            combined_score_percent: 90.0,
            recommended_rating: FsrsRating::Easy,
            applied_rating: FsrsRating::Good,
            was_manual_override: true,
            duration_ms: 1_100,
            reviewed_at: t2,
        };
        let state2 = FsrsCardState {
            card_id: 101,
            stability: 6.4,
            stability_fast: 6.1,
            difficulty: 4.9,
            elapsed_days: 1,
            scheduled_days: 6,
            reps: 2,
            lapses: 0,
            due_at: t2 + Duration::days(6),
            last_reviewed_at: Some(t2),
            last_rating: Some(FsrsRating::Good),
        };

        let (id2, streak2) = db.commit_review_transaction(
            &r2,
            &[],
            &state2,
            t2.date_naive(),
            1,
            DailyGoal::Cards(10),
        )?;
        assert!(id2 > id1);
        assert_eq!(streak2.current_streak_days, 2);

        let pitch_hist = db.list_card_pitch_history(101)?;
        assert_eq!(pitch_hist.len(), 2);
        assert!((pitch_hist[0].pitch_error_percent - 10.0).abs() < 1e-5);

        let latest_pitch = db.latest_pitch_errors_by_card()?;
        assert!((latest_pitch[&101] - 10.0).abs() < 1e-5);

        let series = db.daily_accuracy_series()?;
        assert_eq!(series.len(), 2);
        assert_eq!(series[0].avg_mora_accuracy_percent, Some(50.0));
        assert_eq!(series[1].avg_mora_accuracy_percent, None);
        assert_eq!(series[1].avg_pitch_error_percent, Some(10.0));

        let bundle = db.build_export_bundle()?;
        assert_eq!(bundle.review_history.len(), 2);
        assert_eq!(bundle.kana_stats.len(), 2);
        assert_eq!(bundle.kana_stats[0].kana, "つ");
        assert_eq!(bundle.pitch_error_logs.len(), 2);
        assert_eq!(bundle.fsrs_states.len(), 1);
        assert_eq!(bundle.streak.total_sessions, 2);
        Ok(())
    }
}

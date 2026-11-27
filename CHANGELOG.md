# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.0] - 2026-11-26

### Added
- Official `fsrs` (`7.0.0`) and `csv` (`1.4.0`) crate dependencies for independent spaced-repetition scheduling and UTF-8 tabular exports.
- Local SQLite persistence (`src/storage/db.rs` with `KotodamaDb` and `default_database_path` in `src/storage/mod.rs`) storing `daily_streak`, `fsrs_card_states`, `review_logs`, `mora_attempts`, and `kana_confusion_counts` with atomic review transactions and due-card queue ordering.
- Pluggable mora clarity engine (`src/scoring/mora.rs`) defining the `MoraScorer` trait, pure-Rust `HeuristicAcousticMoraScorer`, katakana-to-hiragana normalization, yōon mora segmentation, top-5 mistake aggregation, and Japanese allophone/devoicing rules (が-row `[g]`/`[ŋ]`, high-vowel devoicing when voiceless consonants hold the full mora beat, assimilated `ん` nasals, and `じ`/`ぢ` + `ず`/`づ` equivalence).
- Pitch scoring helper (`src/scoring/pitch.rs`) and FSRS scheduling bridge (`src/scoring/fsrs_bridge.rs`) combining mora and pitch metrics across practice modes, mapping scores via configurable `ScoreThresholds` presets (`Lenient`, `Balanced`, `Strict`), and supporting manual `1`–`4` rating overrides.
- UTF-8 CSV and JSON exporters (`src/storage/export.rs`) generating `kotodama_review_history.csv`, `kotodama_kana_confusion.csv`, `kotodama_pitch_errors.csv`, and full `ExportBundle` JSON snapshots.
- Centralized Phase 4 domain models in `src/models.rs` (`PhoneticRealization`, `MoraEquivalenceRule`, `MoraScoreRequest`, `MoraEvaluation`, `CombinedScoreBreakdown`, `ReviewLogEntry`, `PitchErrorLogEntry`, `DailyAccuracyPoint`, `FsrsSchedulingOutcome`, `ExportBundle`, `CsvExportReport`) plus `FsrsCardState::stability_fast` and DB serialization helpers.

### Changed
- Wired `KotodamaApp::boot` to initialize the local SQLite database and restore persisted `DailyStreak` progress safely.
- Upgraded the Settings **FSRS Score Thresholds** card with interactive `Lenient`, `Balanced`, and `Strict` preset pills.
- Wired Dashboard **Export CSV** and **Export JSON** buttons to native folder/file pickers (`rfd`) backed by `src/storage/export.rs` with live status feedback and recoverable error toasts.

## [0.3.0] - 2026-11-26

### Added
- Cross-platform `cpal` microphone capture and live monitoring (`src/audio/capture.rs`) with input device enumeration (`list_input_devices`), non-blocking `MicMonitorHandle`, `RecordingSession`, and safe stream error propagation.
- Frame-RMS Voice Activity Detection (VAD) silence trimming (`trim_silence_vad`) and automatic trailing-silence stop tracking (`VadAutoStopTracker`) with configurable sensitivity, trailing silence duration, and max-duration caps.
- Pure-Rust WSOLA time-stretching (`time_stretch_preserve_pitch` and `play_decoded_audio` / `play_audio_file_at_speed` in `src/audio/playback.rs`) for pitch-preserving `0.75x` slow-motion reference and test-take playback.
- Pure-Rust pitch analysis engine (`src/audio/dsp.rs`) implementing YIN `F0` extraction (`extract_f0_contour`), 3-point voiced median smoothing, semitone median normalization (`normalize_to_semitones`), path-normalized DTW alignment (`dtw_pitch_distance`), pitch error % scoring, and synthetic sine-wave / noise / cross-register / tempo / WSOLA verification tests.
- Interactive **Microphone & VAD Calibration** card in Settings (`src/ui/mod.rs`) featuring an input device selector, device refresh, Iced `Canvas` live level meter (`MicMeterProgram`) with coral VAD threshold indicator, mic test monitor, test recording & playback controls, VAD sensitivity slider (`slider_style` in `src/ui/theme.rs`), and trailing silence presets (`600ms`, `1000ms`, `1500ms`).
- Centralized Phase 3 domain types in `src/models.rs` (`InputDeviceInfo`, `VadConfig`, `MicLevelSnapshot`, `PitchExtractionConfig`, `CaptureStopReason`, `RecordedTake`, and `DecodedAudio::to_mono`) plus backwards-compatible `AppSettings` fields (`preferred_input_device`, `vad_rms_threshold`).

### Changed
- Wired Setup **Play Test Audio** to honor the active `0.75x` / `1.0x` `PlaybackSpeed` setting.
- Automatically stops active microphone monitoring or test recording when navigating away from Settings.

## [0.2.1] - 2026-11-26

### Added
- Per-deck card exclusions (`excluded_card_ids` and `excluded_card_labels` in `DeckWorkspaceConfig`) persisted locally in `deck_config.json`, with backwards-compatible migration from `0.2.0` configs.
- **Exclude Card** / **Restore Card** buttons in the Setup live card preview, **Auto-Exclude Invalid Audio Cards**, **View Excluded**, and **Restore Excluded** controls in the Setup GIGO banner, and an **Excluded Cards** management section in Settings.
- Active practice queue filtering (`active_practice_cards`) and subtle excluded-card count badges in Setup, Practice, and Settings.

### Fixed
- Kept `rodio`'s `MixerDeviceSink` alive inside a persistent background playback worker thread with `.log_on_drop(false)` in `src/audio/playback.rs`, eliminating premature sink drops and stderr log spam on repeated test audio clicks.

## [0.2.0] - 2026-11-26

### Added
- Safe temporary-copy SQLite reader (`src/anki/reader.rs`) supporting both classic (`col.decks`/`col.models`) and modern (`decks`/`notetypes`/`fields` with `unicase` collation and WAL sidecars) Anki collections, strictly filtering graduated cards (`queue = 2`) and deduplicating multi-template notes.
- AnkiConnect (`http://127.0.0.1:8765`) client for listing decks, discovering media directories, and fetching learned (`queue = 2`) cards.
- Symphonia-based multi-format audio decoder and Rodio playback helper (`src/audio/playback.rs`) for previewing `.mp3`, `.ogg`, `.wav`, `.flac`, and `.m4a` clips.
- Garbage-In, Garbage-Out (GIGO) audio pre-flight validator (`src/anki/validator.rs`) flagging missing media files, clips shorter than 200 ms, digital clipping, and decode failures, with aggregated `DeckAudioReport` summaries.
- Persistent deck workspace configuration (`deck_config.json`) in `src/storage/mod.rs` storing the active deck and per-deck field mappings.
- Interactive Setup UI (`src/ui/setup.rs`) and adaptive `text_input`/`pick_list` theme styles (`src/ui/theme.rs`) featuring native file/folder pickers (`rfd`), deck selector, 5-field mapping dropdowns with smart defaults, live sample card preview, test audio playback, and non-fatal error toasts.

### Changed
- Centralized domain models in `src/models.rs` extended with `AnkiDeckSummary`, `AnkiNoteType`, `AnkiRawNote`, `AnkiCollectionSummary`, `DeckAudioReport`, `DeckWorkspaceConfig`, `DecodedAudio`, and `AudioQualityIssue::summary`.
- Practice view status badge now displays the count of imported learned cards when a deck is connected.

## [0.1.1] - 2026-11-26

### Added
- `ThemePreference` (`System`, `Light`, `Dark`, defaulting to `System`) and `ResolvedTheme` enums in `src/models.rs`, persisted in `AppSettings`.
- Local JSON settings persistence (`default_settings_path`, `load_settings`, `save_settings`) in `src/storage/mod.rs` using platform-standard config directories.
- Adaptive warm Light and warm Dark palettes (`ThemeTokens`, `LIGHT_TOKENS`, `DARK_TOKENS`) in `src/ui/theme.rs` with warm charcoal/espresso surfaces and soft cream text in Dark mode.
- Cross-platform OS color-scheme detection and live theme change subscription (`iced::system::theme` and `iced::system::theme_changes`), a quick theme toggle in the top navigation bar, and a `System` / `Light` / `Dark` selector in Settings.

### Changed
- Updated all UI views (`setup.rs`, `practice.rs`, `dashboard.rs`, `mod.rs`) to resolve colors dynamically from the active Light or Dark `ThemeTokens`.
- Updated `SPEC.md`, `README.md`, and `ROADMAP.md` to document adaptive cross-platform theming and `v0.1.1`.

## [0.1.0] - 2026-11-26

### Added
- Centralized domain structs and enums in `src/models.rs` (`NavTab`, `DeckConfig`, `DeckSource`, `FieldMapping`, `AudioQualityIssue`, `AnkiCard`, `PracticeMode`, `PitchAnalysis`, `MoraAttempt`, `KanaMistakeEntry`, `KanaStats`, `FsrsRating`, `ScoreThresholds`, `FsrsCardState`, `DailyGoal`, `DailyStreak`, `PlaybackSpeed`, `AppSettings`, `ErrorSeverity`, `AppErrorToast`) with validation logic and unit tests.
- Warm, playful Iced `0.14` color palette and widget styles in `src/ui/theme.rs` (cream canvas, warm-slate typography, coral/amber/sage accents).
- Desktop navigation shell in `src/ui/mod.rs` with Setup, Practice, Dashboard, and Settings views (`src/ui/setup.rs`, `src/ui/practice.rs`, `src/ui/dashboard.rs`), GIGO audio quality notice, keyboard shortcut legend, and interactive settings controls.
- Non-fatal, dismissible toast overlay (`AppErrorToast`) for surfacing recoverable errors and warnings cleanly inside Iced's `update` loop.
- Iced desktop application entry point in `src/main.rs`.

## [0.0.0] - 2026-11-26

### Added
- Initial project specification (`SPEC.md`), engineering rules (`AGENTS.md`), and build roadmap (`ROADMAP.md`).
- User-facing `README.md` with project overview, core features, planned keyboard shortcuts, and build instructions.
- `CHANGELOG.md` following Keep a Changelog 1.1.0.
- `.gitignore` configured for Rust/Cargo, local SQLite databases, temporary Anki collection copies, environment files, and OS/editor artifacts.
- GitHub Actions CI workflow (`.github/workflows/ci.yml`) running `cargo test`, `cargo clippy --all-targets -- -D warnings`, and `cargo doc --no-deps`.
- `MPL-2.0` `LICENSE` and SPDX license header in `src/main.rs`.

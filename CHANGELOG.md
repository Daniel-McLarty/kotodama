# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

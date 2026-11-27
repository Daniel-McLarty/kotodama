# Kotodama Build Roadmap (to v1.0.0)

Execute one phase at a time. Commit and push logically after each meaningful unit of work using Conventional Commits. At the end of each phase, run `cargo test`, `cargo clippy --all-targets -- -D warnings`, and `cargo doc --no-deps`, update `CHANGELOG.md` and `README.md`, bump `Cargo.toml`, tag the version, and push with `--follow-tags`.

- [x] **Phase 1: Foundation & Centralized Models (`v0.1.0`)**
  - Initialize Cargo project (`kotodama`), MPL-2.0 `LICENSE`, `CHANGELOG.md`, and initial `README.md`.
  - Define core domain structs and enums in `src/models.rs` (`DeckConfig`, `FieldMapping`, `AnkiCard`, `PracticeMode`, `PitchAnalysis`, `MoraAttempt`, `KanaStats`, `FsrsCardState`, `DailyStreak`, `AppErrorToast`).
  - Set up the warm/playful Iced theme in `src/ui/theme.rs`, basic navigation shell (Setup, Practice, Dashboard, Settings), and dismissible error toast/popup handling.
  - Add adaptive cross-platform Light/Dark mode (`v0.1.1`) with `ThemePreference` (`System`, `Light`, `Dark`), live OS detection, top-bar toggle, and persisted Settings selector.

- [x] **Phase 2: Anki Ingestion & Field Mapper (`v0.2.0`)**
  - Implement safe `collection.anki2` temp-copy reader in `src/anki/reader.rs` filtering for learned/review cards (`queue = 2`).
  - Implement audio pre-flight validator in `src/anki/validator.rs` (missing files, `< 200ms`, clipping).
  - Build the Setup UI with deck picker, field-mapping dropdowns, GIGO warning banner, live card preview, and graceful file-picker error popups.

- [x] **Phase 3: Audio Pipeline & DSP Engine (`v0.3.0`)**
  - Implement `cpal` mic capture, VAD silence trimming, auto-stop silence detection, and `symphonia`/`rodio` playback (including `0.75x` / `1.0x` speed toggle).
  - Implement $F_0$ pitch extraction, semitone median normalization, and DTW pitch error % in `src/audio/dsp.rs` with synthetic sine-wave unit tests.
  - Add a Mic Calibration bar in Settings.

- [x] **Phase 4: Mora Scoring, FSRS Bridge & Local DB (`v0.4.0`)**
  - Implement local SQLite schema in `src/storage/db.rs` for streaks, review logs, per-kana confusion counts, and independent FSRS states.
  - Implement `src/scoring/mora.rs` (pluggable scorer + top-5 mistakes aggregation) and `src/scoring/fsrs_bridge.rs` (configurable score-to-rating thresholds + manual override).
  - Implement CSV and JSON exporters in `src/storage/export.rs`.

- [x] **Phase 5: Practice Views & Ergonomics (`v0.5.0`)**
  - Build Mora Purity, Pitch Testing (with Iced `Canvas` pitch contour overlay), and Combined Shadowing screens.
  - Wire up keyboard-first shortcuts (`Space`, `R`, `T`, `1–4`), Blind/Audio-Only mode, and Daily Session Cap tracking.

- [x] **Phase 6: Analytics Dashboard & Export UI (`v0.6.0`)**
  - Build Dashboard with daily streak badge, daily goal progress, and accuracy-over-time line graphs (mora clarity % and pitch error %).
  - Build per-kana breakdown view displaying accuracy % and top-5 most common mistakes per kana, plus per-card pitch error history and CSV/JSON export buttons.

- [ ] **Phase 7: Polish, Edge Cases & Integration Testing (`v0.9.0`)**
  - Audit all error paths to verify zero panics on missing files, bad Anki schemas, or unplugged audio devices.
  - Add end-to-end integration tests covering deck import -> synthetic audio scoring -> FSRS update -> DB persistence -> CSV/JSON export.
  - Refine UI spacing, warm micro-copy, and `Canvas` rendering performance.

- [ ] **Phase 8: Production Release (`v1.0.0`)**
  - Finalize user-facing `README.md` (features, setup guide, Anki field-mapping walkthrough, shortcuts, build instructions).
  - Finalize `CHANGELOG.md` for `1.0.0`, verify clean `cargo test`, `cargo clippy`, and `cargo doc --no-deps`, and tag `v1.0.0`.

# Kotodama

**Kotodama** is a local-first, gamified Japanese pronunciation and pitch-accent trainer written in Rust with [Iced](https://iced.rs/). It uses your existing Anki decks—focusing on cards you have already graduated into the **Learned / Review** state—as ground-truth audio and text, then schedules pronunciation drills using an independent **FSRS** database so your reading/recognition Anki intervals are never disturbed.

## Features

- **Anki Learned Deck Integration**
  - Safely reads local `collection.anki2` databases via temporary copies (preventing database locks) or connects via AnkiConnect (`localhost:8765`).
  - Filters exclusively for graduated/learned cards (`queue = 2`) so you practice words and sentences you already know.
  - Interactive **Field Mapper** for Word/Expression, Kana Reading, optional Example Sentence, Word Audio, and optional Sentence Audio, with live card preview and test audio playback.
  - **Garbage-In, Garbage-Out (GIGO) Audio Validator** that flags missing files, ultra-short clips (`< 200ms`), and clipped audio before practice.
- **Mora Purity Testing**
  - Isolates individual kana/mora clarity (such as distinguishing つ, す, and ず) behind a clean scoring boundary.
- **Word & Sentence Pitch Contour DTW**
  - Extracts fundamental frequency ($F_0$), normalizes to semitones relative to each speaker's median, and compares your contour against the native reference using Dynamic Time Warping (DTW) overlaid on an Iced `Canvas` graph.
- **Combined Shadowing**
  - Plays the reference sentence with a visual countdown, records your take with optional Voice Activity Detection (VAD) auto-stop after ~1 second of trailing silence, and supports pitch-preserving `0.75x` / `1.0x` playback and **Blind / Audio-Only Mode**.
- **Independent FSRS Scheduling**
  - Dedicated local SQLite database powered by `fsrs`, mapping scores to `Again`, `Hard`, `Good`, and `Easy` ratings with configurable thresholds and manual overrides.
- **Analytics, Streaks & Per-Kana Mistake Breakdown**
  - Tracks daily streaks, configurable daily goals (cards or minutes), accuracy over time, per-card pitch error %, and top-5 confusion matrices per kana (e.g., `つ: 82% correct — 5% す, 4% ず, 3% すう, 6% unknown`).
  - Exports session history, confusion matrices, and pitch error logs to **CSV** and **JSON**.

## Current Status (`v0.1.0`)

Phase 1 (Foundation & Centralized Models) is complete:
- Warm, playful Iced `0.14` desktop shell with Setup, Practice, Dashboard, and Settings navigation.
- Centralized domain models with validation, streak/goal tracking, and per-kana mistake formatting.
- Dismissible toast overlay for recoverable errors and warnings.

Upcoming phases add Anki collection ingestion & GIGO audio validation (`v0.2.0`), microphone capture & $F_0$/DTW DSP (`v0.3.0`), SQLite/FSRS scheduling & exporters (`v0.4.0`), interactive practice canvases (`v0.5.0`), and full analytics (`v0.6.0`).

## Planned Keyboard Shortcuts

| Shortcut | Action |
| :--- | :--- |
| `Space` | Play reference audio |
| `R` | Start / stop microphone recording |
| `T` | Play your recorded take |
| `1` – `4` | Manual FSRS rating override (`1`: Again, `2`: Hard, `3`: Good, `4`: Easy) |

## Building & Running

### Prerequisites

- Rust stable toolchain (`rustc` / `cargo`)
- On Linux (Ubuntu/Debian), install audio and windowing development headers:

```bash
sudo apt-get update
sudo apt-get install -y libasound2-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev pkg-config
```

### Build & Run

```bash
cargo run
```

### Verification Checks

```bash
cargo test
cargo clippy --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
```

## License

Kotodama is licensed under the **Mozilla Public License 2.0 (MPL-2.0)**. See [LICENSE](LICENSE) for full details.

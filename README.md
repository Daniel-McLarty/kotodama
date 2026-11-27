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

## Current Status (`v0.5.0`)

Phase 5 (Practice Views & Ergonomics) is complete:
- Interactive **Practice** workflows (`src/ui/practice.rs`, `src/ui/mod.rs`) for **Mora Purity**, **Pitch Accent** (with an Iced `Canvas` semitone pitch contour overlay), and **Combined Shadowing** (with a `3 · 2 · 1` visual countdown and auto-record flow).
- Keyboard-first Practice shortcuts (`Space`, `R`, `T`, `1`–`4`, `Enter`, `B`, `S`, `Esc`) with physical-key support under Japanese IMEs, **Blind / Audio-Only Mode** with automatic post-take reveal, `0.75x`/`1.0x` WSOLA playback, and **Daily Session Cap** enforcement for both card and minute goals (with **Continue Extra Practice** override).
- Local SQLite persistence (`src/storage/db.rs`) tracking daily streaks, review logs, per-kana confusion matrices, pitch error logs, and independent `fsrs` scheduling states without modifying Anki databases.
- Pluggable `MoraScorer` trait and pure-Rust `HeuristicAcousticMoraScorer` (`src/scoring/mora.rs`) implementing Japanese allophone and high-vowel devoicing rules plus top-5 confusion aggregation.
- UTF-8 CSV and JSON exporters (`src/storage/export.rs`), `cpal` microphone capture & VAD calibration, YIN `F0` + DTW pitch engine, safe `collection.anki2` / `AnkiConnect` ingestion (`queue = 2`), interactive 5-field Setup mapper, GIGO audio checks, per-deck exclusions, and adaptive Light/Dark mode.

Upcoming phases add the full Analytics Dashboard charts & confusion breakdown UI (`v0.6.0`).

## Connecting an Anki Deck & Mapping Fields

1. Open the **Setup** tab and choose either **Local collection.anki2** or **AnkiConnect (localhost:8765)**.
   - For local collections, browse to your Anki profile's `collection.anki2` (Kotodama automatically infers the sibling `collection.media` folder and reads from an isolated temporary copy so Anki is never locked).
   - For AnkiConnect, verify Anki is running with AnkiConnect installed and click **Connect to AnkiConnect**.
2. Pick your target deck from the **Deck** dropdown (subdecks are included automatically, and only graduated `queue = 2` cards are loaded).
3. Verify or adjust the **Note Field Mapping** dropdowns while watching the **Live Sample Card Preview** update in real time, and click **Play Test Audio** to confirm reference playback.
4. Click **Validate Audio & Save Deck** to run GIGO pre-flight checks across all learned cards and persist your deck mapping locally.
5. Optionally click **Exclude Card** in the card preview or **Auto-Exclude Invalid Audio Cards** in the GIGO banner to filter problematic cards out of active practice queues (you can inspect or restore excluded cards anytime in Setup or Settings).

## Keyboard Shortcuts (Practice Tab)

| Shortcut | Action |
| :--- | :--- |
| `Space` | Play reference audio (and start shadowing countdown in Combined Shadowing when auto-record is on) |
| `R` | Start / stop microphone recording |
| `T` | Play your recorded take |
| `1` – `4` | Manual FSRS rating override (`1`: Again, `2`: Hard, `3`: Good, `4`: Easy) |
| `Enter` | Accept the automated FSRS rating recommendation |
| `B` | Toggle Blind / Audio-Only Mode |
| `S` | Toggle `0.75x` / `1.0x` playback speed |
| `Esc` | Stop active recording or cancel shadowing countdown |

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

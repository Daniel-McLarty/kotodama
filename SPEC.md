# Application Specification: Kotodama

## 1. Overview & Vibe
**Kotodama** is a local-first, gamified Japanese pronunciation trainer built in Rust with **Iced**. It uses the user's existing Anki decks (specifically cards in the `Learned` / `Review` state) as ground-truth audio and text, then schedules pronunciation drills using an independent **FSRS** database.

- **Aesthetic:** Warm, playful, and approachable with **adaptive cross-platform Light & Dark mode** (soft cream/warm-slate Light palette and warm charcoal/espresso Dark palette—avoiding harsh pure black/white—paired with coral/amber/sage accents, rounded cards, encouraging micro-copy, and clear visual feedback). Defaults to the OS color-scheme preference (`System`) with live OS change tracking, a quick top-bar toggle, and a `System` / `Light` / `Dark` selector persisted in local settings.
- **Garbage-In, Garbage-Out Warning:** Prominently warn users during deck setup that pronunciation grading is only as good as their deck's native audio. Flag suspicious audio (missing files, `< 200ms` clips, clipped audio).
- **Adaptive Light & Dark Mode:** Default to following the operating system's color-scheme preference across Linux (XDG Desktop Portal / GTK), Windows, and macOS, with a manual three-way override (`System`, `Light`, `Dark`) in Settings and a quick toggle in the navigation header. Maintain the warm, playful palette in both variants (soft cream/coral/sage in Light mode; warm charcoal/espresso/amber/sage in Dark mode—never harsh pure `#000000` or `#FFFFFF`).

## 2. Core Features

### A. Anki Integration & Field Mapping
- Support reading local `collection.anki2` (copied to a temp file first to prevent DB locks) and/or `AnkiConnect` (`localhost:8765`).
- Only pull cards that are graduated/learned (`queue = 2` / review state) so the user only practices words and sentences they already know.
- **Field Mapper UI:** Let the user select a deck and map its note fields to:
  - Target Word / Expression
  - Kana Reading
  - Example Sentence (optional)
  - Word Audio (`[sound:...]`)
  - Sentence Audio (`[sound:...]`)
- Include a live preview of a sample card and a "Play Test Audio" button. Persist deck mappings in local config.
- **Non-Destructive Card Exclusion:** Allow users to exclude individual cards or auto-exclude all cards with flagged audio quality issues per deck (persisted locally and restorable anytime in Setup or Settings) without modifying the underlying Anki deck.

### B. Practice Modes & Ergonomics
1. **Mora Purity Testing:** Isolates individual kana/mora clarity (e.g., distinguishing つ, す, ず).
2. **Word & Sentence Pitch Testing:** Compares the user's normalized fundamental frequency ($F_0$ in semitones relative to speaker median) against the reference audio using Dynamic Time Warping (DTW). Displays an overlaid pitch contour graph in an Iced `Canvas`.
3. **Combined Shadowing:** Plays the reference sentence with a visual countdown, records the user, and scores both mora clarity and pitch contour together.
4. **Quality-of-Life Controls:**
   - **Keyboard-First Flow:** `Space` (play reference), `R` (start/stop recording), `T` (play your take), `1–4` (manual FSRS override).
   - **Auto-Stop Recording:** Use Voice Activity Detection (VAD) to automatically stop recording after ~1 second of trailing silence (with a toggle in Settings).
   - **Slow-Motion Reference (`0.75x` / `1.0x`):** Pitch-preserving playback slowdown for fast native sentences.
   - **Blind / Audio-Only Mode:** Toggle to hide the text until after speaking.
   - **Daily Session Cap:** Configurable daily goal (e.g., 10 cards or 5 minutes) to keep sessions fun and sustainable.
5. **Japanese Allophone & Devoicing Rules (src/scoring/mora.rs):** 
   - Map target kana to sets of valid standard realizations rather than single rigid acoustic templates.
   - Accept both oral [g] and nasal [ŋ] as valid for the が-row (が・ぎ・ぐ・げ・ご).
   - Accept devoiced い/う (e.g., です, ます, 好き) when the voiceless consonant holds the full mora beat.
   - Accept assimilated nasals ([m], [n], [ŋ], [ɴ]) for syllabic ん, and treat じ/ぢ and ず/づ as equivalent.

### C. Analytics, Streaks & Mistake Breakdown
- **Daily Streaks:** Track consecutive days practiced, daily goal progress, and total sessions.
- **Accuracy Over Time:** Line/area graphs showing mora clarity % and pitch accent error % over time.
- **Pronunciation Clarity Breakdown (Per-Kana):**
  - Track exact accuracy % per kana.
  - Track the **top 5 most common mistakes per kana**, formatted clearly for the user—e.g.:
    `つ: 82% correct — 5% す, 4% ず, 3% すう, 6% unknown`
- **Pitch Accent Breakdown:**
  - Show average pitch error % over time.
  - Show per-card pitch accent error % on each review card.
- **Export:** Export full session history, per-kana confusion matrix, and pitch error logs to **CSV** and **JSON**.

### D. Independent FSRS Scheduling
- Use the `fsrs` crate with a dedicated local SQLite database so pronunciation reviews never alter the user's Anki reading/recognition intervals.
- Map combined numeric scores to FSRS ratings (`Again`, `Hard`, `Good`, `Easy`) with configurable thresholds and a manual override button.

## 3. Architecture & Crate Selection Freedom
- **Crate Selection:** You have full autonomy to choose the best, currently maintained crates on `crates.io` and install their latest compatible versions via `cargo add` as each phase needs them.
- **Architectural Preferences:**
  - **UI:** `iced` (with canvas/drawing support for pitch graphs and waveforms)
  - **Audio I/O & Decoding:** Cross-platform mic capture and playback supporting `.mp3`, `.ogg`, and `.opus` (e.g., `cpal`, `rodio`, `symphonia`, or better modern equivalents)
  - **DSP & Pitch Extraction:** Pure-Rust $F_0$ extraction (MPM/YIN), semitone median normalization, VAD silence trimming, and DTW
  - **Mora Scoring:** Keep behind a clean trait boundary in `src/scoring/mora.rs` so acoustic classifiers or local ONNX phoneme models (`ort`) can be swapped cleanly
  - **Scheduling & Storage:** Official `fsrs` crate, bundled SQLite (`rusqlite`), `serde`, `csv`, `chrono`, and `anyhow`

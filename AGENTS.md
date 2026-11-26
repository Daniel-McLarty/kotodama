# Agent Engineering Rules & Constraints — Kotodama

You are building **Kotodama**, a native Rust desktop application using Iced. Follow these rules strictly on every commit and iteration.

## 1. Rust Style & Architecture
- **Idiomatic Rust:** Write clean, idiomatic Rust. Avoid `.unwrap()` and `.expect()` in production code.
- **Centralized Structs:** Define all shared domain structs and enums in `src/models.rs`. Module-private helper structs are only allowed if they never cross module boundaries.
- **Modularization:** Split files when a module grows beyond a single clear responsibility. Use Rust's module system (`mod.rs` or named module files).
- **Self-Documenting Code:** Use highly descriptive function, struct, and variable names so the code reads clearly without inline commentary.
- **Documentation Comments (`rustdoc`):**
  - Every public and module-level function, struct, and enum MUST have a `///` doc comment describing **WHAT** it does, never **HOW** it does it.
  - Format comments so `cargo doc --no-deps` builds with zero warnings.
- **Inline Comments:** Do NOT put comments inside function bodies unless explaining a non-obvious magic number, a DSP/mathematical invariant, or genuinely dense logic that cannot be simplified by renaming.

## 2. Error Handling Philosophy (Application Rust)
- Use `anyhow::Result` with descriptive `.context("...")` messages to propagate errors cleanly up to the main application boundary.
- **Never crash on recoverable user errors:** File-not-found, invalid deck paths, missing audio files, or disconnected microphones MUST be caught in Iced's `update` loop and surfaced as a friendly, dismissible popup or toast explaining what went wrong and letting the user pick another file or retry.
- Reserve hard exits strictly for truly unrecoverable startup failures, printing a clear, human-readable error message.

## 3. Verification & Testing Gate
- Every non-trivial function MUST have unit tests (`#[cfg(test)]`) or `rustdoc` tests verifying its behavior.
- Use compile-time guarantees (strong newtypes, enums, `const` assertions) wherever possible.
- Before completing any milestone or pushing a release, you MUST run and pass:
  1. `cargo test`
  2. `cargo clippy --all-targets -- -D warnings`
  3. `cargo doc --no-deps`

## 4. Git Workflow, Commits & Pushing
- **Atomic Commits:** Commit and push (`git push origin <branch>`) after any meaningful, self-contained unit of work (e.g., finishing a module, adding a UI view, implementing a DSP function with tests). Never accumulate thousands of lines into a single mega-commit.
- **Conventional Commits:** Every commit message MUST follow the Conventional Commits specification:
  - `feat(anki): add safe temp-copy reader for collection.anki2`
  - `fix(audio): trim leading silence before DTW scoring`
  - `docs(readme): add deck field-mapping guide`
  - `test(dsp): add synthetic sine-wave pitch extraction tests`
- **Versioning & Tagging:** Follow Semantic Versioning (`MAJOR.MINOR.PATCH`). At the end of each completed phase in `ROADMAP.md`, update `Cargo.toml`, update `CHANGELOG.md`, commit the release, create an annotated git tag (e.g., `git tag -a v0.1.0 -m "Release v0.1.0"`), and push both commits and tags (`git push --follow-tags`).

## 5. Documentation & Licensing
- **License:** Mozilla Public License 2.0 (`MPL-2.0`). Include the SPDX header `// SPDX-License-Identifier: MPL-2.0` at the top of every `.rs` file.
- **Changelog:** Maintain `CHANGELOG.md` following [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) (`Added`, `Changed`, `Fixed`, `Removed`).
- **README:** Maintain a clear, user-facing `README.md` covering what Kotodama does, how to connect an Anki deck, how to map fields, keyboard shortcuts, and how to build/run it. Update `README.md` whenever user-facing features or setup steps change.

## 6. Project Directory Layout
Strictly adhere to this module layout so structs stay centralized and responsibilities remain cleanly separated:
```text
kotodama/
├── .github/workflows/ci.yml   # GitHub Actions (cargo test, clippy, rustdoc)
├── AGENTS.md
├── SPEC.md
├── ROADMAP.md
├── CHANGELOG.md
├── LICENSE                    # MPL-2.0
├── README.md
├── .gitignore                 # Ignore /target, *.db, *.sqlite, temp Anki copies, .env
├── Cargo.toml
└── src/
    ├── main.rs                # Entry point, Iced application bootstrap
    ├── models.rs              # CENTRALIZED structs, enums, and domain types
    ├── anki/
    │   ├── mod.rs
    │   ├── reader.rs          # Safe SQLite temp-copy / AnkiConnect ingestion
    │   └── validator.rs       # Garbage-in/garbage-out audio quality checks
    ├── audio/
    │   ├── mod.rs
    │   ├── capture.rs         # cpal mic recording + VAD silence trimming
    │   ├── playback.rs        # symphonia/rodio reference & user playback
    │   └── dsp.rs             # Pitch extraction (F0), semitone norm, DTW
    ├── scoring/
    │   ├── mod.rs
    │   ├── mora.rs            # Mora purity & top-5 confusion matrix
    │   ├── pitch.rs           # Pitch accent error % calculation
    │   └── fsrs_bridge.rs     # Maps scores to FSRS ratings & updates local DB
    ├── storage/
    │   ├── mod.rs
    │   ├── db.rs              # Local SQLite for streaks, history, FSRS state
    │   └── export.rs          # CSV and JSON data exporters
    └── ui/
        ├── mod.rs
        ├── theme.rs           # Warm, playful color palette & typography
        ├── setup.rs           # Deck picker, field mapper, GIGO warning
        ├── practice.rs        # Mora, Pitch, and Shadowing views + Canvas graphs
        └── dashboard.rs       # Streaks, accuracy graphs, per-kana mistake breakdown
```
## GitHub Remote, GPG Signing & CI/CD

- Remote & Branch: The remote is GitHub (origin) and primary development/releases live on main.

 - **GPG** Signing: Commits and annotated tags use **GPG** signing. Do not pass --no-gpg-sign or override signing configs. Allow interactive time/retries if a **GPG** passphrase prompt appears, and pause to notify the user if **GPG** signing fails due to a locked keyring.

- Repository Hygiene & CI:

    - Create and maintain .gitignore (excluding /target, local .db/.sqlite files, temporary Anki copies, and OS/editor artifacts).

    - Set up .github/workflows/ci.yml to run cargo test, cargo clippy --all-targets -- -D warnings, and cargo doc --no-deps on pushes and pull requests to main (install libasound2-dev and libxkbcommon-dev on Ubuntu runners so cpal and iced build cleanly in CI).

    - If gh **CLI** is authenticated, you may also create GitHub Releases corresponding to SemVer tags (v0.1.0 through v1.0.0) using the matching **CHANGELOG**.md notes.
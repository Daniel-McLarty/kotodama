# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.0] - 2026-11-26

### Added
- Initial project specification (`SPEC.md`), engineering rules (`AGENTS.md`), and build roadmap (`ROADMAP.md`).
- User-facing `README.md` with project overview, core features, planned keyboard shortcuts, and build instructions.
- `CHANGELOG.md` following Keep a Changelog 1.1.0.
- `.gitignore` configured for Rust/Cargo, local SQLite databases, temporary Anki collection copies, environment files, and OS/editor artifacts.
- GitHub Actions CI workflow (`.github/workflows/ci.yml`) running `cargo test`, `cargo clippy --all-targets -- -D warnings`, and `cargo doc --no-deps`.
- `MPL-2.0` `LICENSE` and SPDX license header in `src/main.rs`.

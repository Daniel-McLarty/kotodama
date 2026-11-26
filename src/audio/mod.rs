// SPDX-License-Identifier: MPL-2.0

/// Microphone input capture, level monitoring, and Voice Activity Detection (VAD).
pub mod capture;
/// Pure-Rust pitch extraction (`F0`), semitone median normalization, and DTW alignment.
pub mod dsp;
/// Audio decoding and playback helpers powered by Symphonia and Rodio.
pub mod playback;

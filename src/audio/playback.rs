// SPDX-License-Identifier: MPL-2.0

use std::fs::File;
use std::num::NonZero;
use std::path::Path;

use anyhow::{Context, Result, bail};
use rodio::buffer::SamplesBuffer;
use rodio::{DeviceSinkBuilder, Player};
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::{MediaSource, MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;

use crate::models::DecodedAudio;

/// Decodes an audio file (`.mp3`, `.ogg`, `.wav`, `.flac`, `.m4a`, etc.) into interleaved `f32` PCM.
pub fn decode_audio_file(path: &Path) -> Result<DecodedAudio> {
    if !path.is_file() {
        bail!("Audio file does not exist at {}", path.display());
    }

    let file = File::open(path)
        .with_context(|| format!("Failed to open audio file at {}", path.display()))?;
    let source: Box<dyn MediaSource> = Box::new(file);
    let mss = MediaSourceStream::new(source, MediaSourceStreamOptions::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|value| value.to_str()) {
        hint.with_extension(ext);
    }

    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .with_context(|| format!("Unsupported or corrupt audio container in {}", path.display()))?;

    let track = format
        .default_track(TrackType::Audio)
        .with_context(|| format!("No playable audio track found in {}", path.display()))?;
    let track_id = track.id;
    let audio_params = track
        .codec_params
        .as_ref()
        .and_then(|params| params.audio())
        .with_context(|| format!("Missing audio codec parameters in {}", path.display()))?
        .clone();

    let mut sample_rate = audio_params.sample_rate.unwrap_or(0);
    let mut channels = audio_params
        .channels
        .as_ref()
        .map(|ch| ch.count() as u16)
        .unwrap_or(0);

    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&audio_params, &AudioDecoderOptions::default())
        .with_context(|| format!("Unsupported audio codec in {}", path.display()))?;

    let mut samples = Vec::new();
    let mut packet_samples = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            Err(SymphoniaError::ResetRequired) => {
                decoder.reset();
                continue;
            }
            Err(err) => {
                return Err(err).with_context(|| {
                    format!("Failed while reading audio packets from {}", path.display())
                });
            }
        };

        if packet.track_id != track_id {
            continue;
        }

        let decoded = decoder.decode(&packet).with_context(|| {
            format!("Failed while decoding audio frame in {}", path.display())
        })?;

        if sample_rate == 0 {
            sample_rate = decoded.spec().rate();
        }
        if channels == 0 {
            channels = decoded.spec().channels().count() as u16;
        }

        packet_samples.clear();
        decoded.copy_to_vec_interleaved::<f32>(&mut packet_samples);
        samples.extend_from_slice(&packet_samples);
    }

    if samples.is_empty() || sample_rate == 0 || channels == 0 {
        bail!("Audio file {} decoded to zero playable frames", path.display());
    }

    Ok(DecodedAudio {
        samples,
        sample_rate,
        channels,
    })
}

/// Decodes and plays an audio clip on the default system output device until completion.
#[allow(dead_code)]
pub fn play_audio_file(path: &Path) -> Result<()> {
    let decoded = decode_audio_file(path)?;

    let sample_rate = NonZero::new(decoded.sample_rate)
        .context("Decoded audio reported an invalid 0 Hz sample rate")?;
    let channels = NonZero::new(decoded.channels)
        .context("Decoded audio reported an invalid 0 channel count")?;

    let stream_handle = DeviceSinkBuilder::open_default_sink()
        .context("No audio output device is available for playback")?;
    let player = Player::connect_new(stream_handle.mixer());
    let buffer = SamplesBuffer::new(channels, sample_rate, decoded.samples);

    player.append(buffer);
    player.sleep_until_end();
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;
    use std::fs;
    use std::path::PathBuf;

    use tempfile::TempDir;

    use super::*;

    fn write_test_wav(path: &Path, sample_rate: u32, duration_ms: u64, amplitude: f32) -> Result<()> {
        let total_frames = (u64::from(sample_rate) * duration_ms / 1000) as usize;
        let data_bytes_len = (total_frames * 2) as u32;
        let riff_size = 36_u32 + data_bytes_len;

        let mut bytes = Vec::with_capacity(44 + data_bytes_len as usize);
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&riff_size.to_le_bytes());
        bytes.extend_from_slice(b"WAVE");
        bytes.extend_from_slice(b"fmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&sample_rate.to_le_bytes());
        bytes.extend_from_slice(&(sample_rate * 2).to_le_bytes());
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_bytes_len.to_le_bytes());

        for frame in 0..total_frames {
            let t = frame as f32 / sample_rate as f32;
            let sample = (TAU * 440.0 * t).sin() * amplitude;
            let pcm = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16;
            bytes.extend_from_slice(&pcm.to_le_bytes());
        }

        fs::write(path, bytes)?;
        Ok(())
    }

    #[test]
    fn decode_audio_file_reads_valid_wav_samples_and_duration() -> Result<()> {
        let dir = TempDir::new()?;
        let wav_path = dir.path().join("tone.wav");
        write_test_wav(&wav_path, 16_000, 350, 0.5)?;

        let decoded = decode_audio_file(&wav_path)?;
        assert_eq!(decoded.sample_rate, 16_000);
        assert_eq!(decoded.channels, 1);
        assert_eq!(decoded.duration_ms(), 350);
        assert!(!decoded.samples.is_empty());
        Ok(())
    }

    #[test]
    fn decode_audio_file_errors_cleanly_on_missing_and_corrupt_files() -> Result<()> {
        let dir = TempDir::new()?;
        let missing = PathBuf::from(dir.path()).join("missing.wav");
        assert!(decode_audio_file(&missing).is_err());

        let corrupt = dir.path().join("corrupt.mp3");
        fs::write(&corrupt, b"definitely not valid audio bytes")?;
        assert!(decode_audio_file(&corrupt).is_err());
        Ok(())
    }
}

// SPDX-License-Identifier: MPL-2.0

use std::f32::consts::TAU;
use std::fs::File;
use std::num::NonZero;
use std::path::Path;
use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, OnceLock};
use std::thread;

use anyhow::{Context, Result, bail};
use rodio::buffer::SamplesBuffer;
use rodio::{ChannelCount, DeviceSinkBuilder, MixerDeviceSink, Player, SampleRate};
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::{MediaSource, MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;

use crate::models::{DecodedAudio, PlaybackSpeed};

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

struct PlaybackJob {
    decoded: DecodedAudio,
    reply_tx: Sender<Result<()>>,
}

static PLAYBACK_WORKER: OnceLock<Mutex<Sender<PlaybackJob>>> = OnceLock::new();

fn validate_playback_params(decoded: &DecodedAudio) -> Result<(ChannelCount, SampleRate)> {
    let channels = NonZero::new(decoded.channels)
        .context("Decoded audio reported an invalid 0 channel count")?;
    let sample_rate = NonZero::new(decoded.sample_rate)
        .context("Decoded audio reported an invalid 0 Hz sample rate")?;
    Ok((channels, sample_rate))
}

fn playback_worker_sender() -> Result<&'static Mutex<Sender<PlaybackJob>>> {
    if let Some(sender) = PLAYBACK_WORKER.get() {
        return Ok(sender);
    }

    let (tx, rx) = mpsc::channel::<PlaybackJob>();
    thread::Builder::new()
        .name("kotodama-audio-playback".to_string())
        .spawn(move || {
            let mut device_sink: Option<MixerDeviceSink> = None;

            while let Ok(job) = rx.recv() {
                let result = play_on_persistent_sink(&mut device_sink, job.decoded);
                let _ = job.reply_tx.send(result);
            }
        })
        .context("Failed to spawn audio playback worker thread")?;

    let _ = PLAYBACK_WORKER.set(Mutex::new(tx));
    PLAYBACK_WORKER
        .get()
        .context("Audio playback worker failed to initialize")
}

fn play_on_persistent_sink(
    device_sink: &mut Option<MixerDeviceSink>,
    decoded: DecodedAudio,
) -> Result<()> {
    let (channels, sample_rate) = validate_playback_params(&decoded)?;

    if device_sink.is_none() {
        let mut opened = DeviceSinkBuilder::open_default_sink()
            .context("No audio output device is available for playback")?;
        opened.log_on_drop(false);
        *device_sink = Some(opened);
    }

    let Some(sink) = device_sink.as_ref() else {
        bail!("Audio output device could not be initialized");
    };

    let player = Player::connect_new(sink.mixer());
    let buffer = SamplesBuffer::new(channels, sample_rate, decoded.samples);
    player.append(buffer);
    player.sleep_until_end();
    Ok(())
}

/// Time-stretches `audio` to the requested [`PlaybackSpeed`] while preserving vocal pitch via WSOLA.
pub fn time_stretch_preserve_pitch(
    audio: &DecodedAudio,
    speed: PlaybackSpeed,
) -> Result<DecodedAudio> {
    let _ = validate_playback_params(audio)?;
    if audio.samples.is_empty() {
        bail!("Cannot time-stretch an empty audio buffer");
    }

    if speed == PlaybackSpeed::Normal {
        return Ok(audio.clone());
    }

    let rate = speed.multiplier();
    let total_frames = audio.frame_count();
    let mut win_len = ((audio.sample_rate as f32 * 0.030).round() as usize).max(64);
    if !win_len.is_multiple_of(2) {
        win_len += 1;
    }
    if total_frames <= win_len + 4 {
        return Ok(audio.clone());
    }

    let syn_hop = win_len / 2;
    let delta = ((audio.sample_rate as f32 * 0.010).round() as usize).max(1);
    let hann: Vec<f32> = (0..win_len)
        .map(|i| 0.5 * (1.0 - (TAU * i as f32 / win_len as f32).cos()))
        .collect();
    let mono = audio.to_mono();
    let ch = usize::from(audio.channels);

    let est_frames = ((total_frames as f32 / rate).ceil() as usize) + win_len * 2;
    let mut out_samples = vec![0.0_f32; est_frames * ch];
    let mut weight_sum = vec![0.0_f32; est_frames];

    let mut max_written_frame = 0_usize;
    let overlap_add = |out_samples: &mut Vec<f32>,
                       weight_sum: &mut Vec<f32>,
                       max_written_frame: &mut usize,
                       in_start: usize,
                       out_start: usize| {
        let needed_frames = out_start + win_len;
        if needed_frames > weight_sum.len() {
            weight_sum.resize(needed_frames, 0.0);
            out_samples.resize(needed_frames * ch, 0.0);
        }
        for i in 0..win_len {
            let w = hann[i];
            let in_base = (in_start + i) * ch;
            let out_base = (out_start + i) * ch;
            for c in 0..ch {
                out_samples[out_base + c] += audio.samples[in_base + c] * w;
            }
            weight_sum[out_start + i] += w;
        }
        if needed_frames > *max_written_frame {
            *max_written_frame = needed_frames;
        }
    };

    overlap_add(&mut out_samples, &mut weight_sum, &mut max_written_frame, 0, 0);
    let mut prev_in = 0_usize;
    let mut step = 1_usize;

    loop {
        let nom_pos = ((step * syn_hop) as f32 * rate).round() as usize;
        if nom_pos + win_len > total_frames {
            break;
        }

        let nat_pos = prev_in + syn_hop;
        let best_cand = if nat_pos + win_len <= total_frames {
            let min_cand = nom_pos.saturating_sub(delta);
            let max_cand = (nom_pos + delta).min(total_frames - win_len);
            find_best_wsola_offset(&mono, nat_pos, min_cand, max_cand, win_len, nom_pos)
        } else {
            nom_pos
        };

        let out_pos = step * syn_hop;
        overlap_add(
            &mut out_samples,
            &mut weight_sum,
            &mut max_written_frame,
            best_cand,
            out_pos,
        );
        prev_in = best_cand;
        step += 1;
    }

    out_samples.truncate(max_written_frame * ch);
    for (frame_idx, &w) in weight_sum[..max_written_frame].iter().enumerate() {
        let base = frame_idx * ch;
        if w > 1e-4 {
            let inv = 1.0 / w;
            for c in 0..ch {
                out_samples[base + c] = (out_samples[base + c] * inv).clamp(-1.0, 1.0);
            }
        } else {
            for c in 0..ch {
                out_samples[base + c] = 0.0;
            }
        }
    }

    Ok(DecodedAudio {
        samples: out_samples,
        sample_rate: audio.sample_rate,
        channels: audio.channels,
    })
}

fn find_best_wsola_offset(
    mono: &[f32],
    nat_pos: usize,
    min_cand: usize,
    max_cand: usize,
    win_len: usize,
    fallback: usize,
) -> usize {
    let target = &mono[nat_pos..nat_pos + win_len];
    let target_energy: f32 = target.iter().map(|&s| s * s).sum();
    if target_energy <= 1e-9 {
        return fallback;
    }

    let mut best_cand = fallback;
    let mut best_score = f32::NEG_INFINITY;

    for cand in min_cand..=max_cand {
        let candidate = &mono[cand..cand + win_len];
        let mut dot = 0.0_f32;
        let mut cand_energy = 0.0_f32;
        for (&a, &b) in target.iter().zip(candidate.iter()) {
            dot += a * b;
            cand_energy += b * b;
        }
        let denom = (target_energy * cand_energy).sqrt();
        if denom > 1e-9 {
            let score = dot / denom;
            if score > best_score {
                best_score = score;
                best_cand = cand;
            }
        }
    }

    best_cand
}

/// Plays an in-memory [`DecodedAudio`] buffer at the requested [`PlaybackSpeed`] on the persistent output sink.
pub fn play_decoded_audio(decoded: DecodedAudio, speed: PlaybackSpeed) -> Result<()> {
    let stretched = time_stretch_preserve_pitch(&decoded, speed)?;
    let _ = validate_playback_params(&stretched)?;

    let (reply_tx, reply_rx) = mpsc::channel();
    let job = PlaybackJob {
        decoded: stretched,
        reply_tx,
    };

    {
        let sender_lock = playback_worker_sender()?;
        let sender = sender_lock
            .lock()
            .map_err(|_| anyhow::anyhow!("Audio playback worker mutex was poisoned"))?;
        sender
            .send(job)
            .map_err(|_| anyhow::anyhow!("Audio playback worker stopped unexpectedly"))?;
    }

    reply_rx
        .recv()
        .context("Audio playback worker disconnected before finishing playback")?
}

/// Decodes and plays an audio clip at the specified [`PlaybackSpeed`] until completion.
pub fn play_audio_file_at_speed(path: &Path, speed: PlaybackSpeed) -> Result<()> {
    let decoded = decode_audio_file(path)?;
    play_decoded_audio(decoded, speed)
}

/// Decodes and plays an audio clip at normal (`1.0x`) speed on a persistent system output sink.
#[allow(dead_code)]
pub fn play_audio_file(path: &Path) -> Result<()> {
    play_audio_file_at_speed(path, PlaybackSpeed::Normal)
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

    #[test]
    fn validate_playback_params_rejects_zero_rate_or_channels() -> Result<()> {
        let valid = DecodedAudio {
            samples: vec![0.0; 16],
            sample_rate: 16_000,
            channels: 1,
        };
        let (ch, sr) = validate_playback_params(&valid)?;
        assert_eq!(ch.get(), 1);
        assert_eq!(sr.get(), 16_000);

        let zero_rate = DecodedAudio {
            samples: vec![0.0; 16],
            sample_rate: 0,
            channels: 1,
        };
        assert!(validate_playback_params(&zero_rate).is_err());

        let zero_channels = DecodedAudio {
            samples: vec![0.0; 16],
            sample_rate: 16_000,
            channels: 0,
        };
        assert!(validate_playback_params(&zero_channels).is_err());
        Ok(())
    }

    #[test]
    fn time_stretch_normal_returns_unchanged_and_slow_extends_duration_cleanly() -> Result<()> {
        let sample_rate = 16_000_u32;
        let frames = (sample_rate as usize * 600) / 1000;
        let mono_samples: Vec<f32> = (0..frames)
            .map(|i| (TAU * 220.0 * i as f32 / sample_rate as f32).sin() * 0.5)
            .collect();
        let mono = DecodedAudio {
            samples: mono_samples,
            sample_rate,
            channels: 1,
        };

        let normal = time_stretch_preserve_pitch(&mono, PlaybackSpeed::Normal)?;
        assert_eq!(normal, mono);

        let slow = time_stretch_preserve_pitch(&mono, PlaybackSpeed::Slow)?;
        assert_eq!(slow.sample_rate, sample_rate);
        assert_eq!(slow.channels, 1);
        let ratio = slow.duration_ms() as f32 / mono.duration_ms() as f32;
        assert!(
            (1.25..=1.42).contains(&ratio),
            "expected ~1.33x duration ratio at 0.75x speed, got {ratio:.3}"
        );
        assert!(
            slow.samples
                .iter()
                .all(|&s| s.is_finite() && (-1.0..=1.0).contains(&s))
        );

        let stereo_samples: Vec<f32> = mono.samples.iter().flat_map(|&s| [s, -s]).collect();
        let stereo = DecodedAudio {
            samples: stereo_samples,
            sample_rate,
            channels: 2,
        };
        let slow_stereo = time_stretch_preserve_pitch(&stereo, PlaybackSpeed::Slow)?;
        assert_eq!(slow_stereo.channels, 2);
        assert_eq!(slow_stereo.samples.len() % 2, 0);
        Ok(())
    }

    #[test]
    fn time_stretch_handles_short_and_invalid_buffers_without_panicking() -> Result<()> {
        let tiny = DecodedAudio {
            samples: vec![0.1; 120],
            sample_rate: 16_000,
            channels: 1,
        };
        let stretched_tiny = time_stretch_preserve_pitch(&tiny, PlaybackSpeed::Slow)?;
        assert_eq!(stretched_tiny, tiny);

        let empty = DecodedAudio {
            samples: Vec::new(),
            sample_rate: 16_000,
            channels: 1,
        };
        assert!(time_stretch_preserve_pitch(&empty, PlaybackSpeed::Slow).is_err());
        Ok(())
    }
}

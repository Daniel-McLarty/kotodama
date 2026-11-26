// SPDX-License-Identifier: MPL-2.0

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample, SupportedStreamConfig};

use crate::models::{
    CaptureStopReason, DecodedAudio, InputDeviceInfo, MicLevelSnapshot, RecordedTake, VadConfig,
};

/// Computes root-mean-square amplitude and peak absolute sample level for `samples`.
pub fn compute_rms_and_peak(samples: &[f32]) -> (f32, f32) {
    if samples.is_empty() {
        return (0.0, 0.0);
    }

    let mut sum_sq = 0.0_f64;
    let mut peak = 0.0_f32;
    for &sample in samples {
        if sample.is_finite() {
            let abs = sample.abs();
            if abs > peak {
                peak = abs;
            }
            let s = f64::from(sample);
            sum_sq += s * s;
        }
    }

    let rms = (sum_sq / samples.len() as f64).sqrt() as f32;
    (rms, peak)
}

/// Converts a linear RMS amplitude into decibels relative to full scale (`-60.0..=0.0`).
pub fn rms_to_dbfs(rms: f32) -> f32 {
    if !rms.is_finite() || rms <= 0.0 {
        return -60.0;
    }
    (20.0 * rms.max(1e-6).log10()).clamp(-60.0, 0.0)
}

/// Maps a decibel value (`-60.0..=0.0 dBFS`) onto a normalized `0.0..=1.0` meter ratio.
pub fn dbfs_to_meter_ratio(dbfs: f32) -> f32 {
    if !dbfs.is_finite() {
        return 0.0;
    }
    ((dbfs + 60.0) / 60.0).clamp(0.0, 1.0)
}

/// Converts a linear VAD RMS threshold into a normalized `0.0..=1.0` meter position.
pub fn vad_threshold_to_meter_ratio(rms_threshold: f32) -> f32 {
    dbfs_to_meter_ratio(rms_to_dbfs(rms_threshold))
}

/// Trims leading and trailing silence from `audio` using frame RMS Voice Activity Detection.
pub fn trim_silence_vad(audio: &DecodedAudio, config: &VadConfig) -> (DecodedAudio, bool) {
    let mono = audio.to_mono();
    if mono.is_empty() || audio.sample_rate == 0 {
        return (
            DecodedAudio {
                samples: Vec::new(),
                sample_rate: audio.sample_rate,
                channels: 1,
            },
            false,
        );
    }

    let frame_samples =
        ((u64::from(audio.sample_rate) * config.frame_ms.max(1)) / 1000).max(1) as usize;

    let mut first_active_frame: Option<usize> = None;
    let mut last_active_frame: Option<usize> = None;

    for (frame_idx, chunk) in mono.chunks(frame_samples).enumerate() {
        let (rms, _) = compute_rms_and_peak(chunk);
        if rms >= config.rms_threshold {
            if first_active_frame.is_none() {
                first_active_frame = Some(frame_idx);
            }
            last_active_frame = Some(frame_idx);
        }
    }

    let (Some(first_frame), Some(last_frame)) = (first_active_frame, last_active_frame) else {
        return (
            DecodedAudio {
                samples: Vec::new(),
                sample_rate: audio.sample_rate,
                channels: 1,
            },
            false,
        );
    };

    let padding_samples =
        ((u64::from(audio.sample_rate) * config.padding_ms) / 1000) as usize;
    let speech_start = first_frame.saturating_mul(frame_samples);
    let speech_end = last_frame
        .saturating_add(1)
        .saturating_mul(frame_samples)
        .min(mono.len());

    let trimmed_start = speech_start.saturating_sub(padding_samples);
    let trimmed_end = speech_end.saturating_add(padding_samples).min(mono.len());

    (
        DecodedAudio {
            samples: mono[trimmed_start..trimmed_end].to_vec(),
            sample_rate: audio.sample_rate,
            channels: 1,
        },
        true,
    )
}

/// Enumerates all available audio input devices on the current system.
pub fn list_input_devices() -> Result<Vec<InputDeviceInfo>> {
    let host = cpal::default_host();
    let default_name = host.default_input_device().map(|device| device.to_string());
    let devices = host
        .input_devices()
        .context("Failed to enumerate system microphone devices")?;

    let mut collected: Vec<InputDeviceInfo> = devices
        .map(|device| {
            let name = device.to_string();
            let is_default = default_name.as_deref() == Some(name.as_str());
            InputDeviceInfo { name, is_default }
        })
        .collect();

    collected.sort_by(|left, right| {
        right
            .is_default
            .cmp(&left.is_default)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });

    Ok(collected)
}

struct VadAutoStopTracker {
    sample_rate: u32,
    config: VadConfig,
    frame_samples: usize,
    pending: Vec<f32>,
    consecutive_speech_frames: usize,
    speech_started: bool,
    trailing_silence_samples: usize,
    total_samples: usize,
}

impl VadAutoStopTracker {
    fn new(sample_rate: u32, config: VadConfig) -> Self {
        let frame_samples =
            ((u64::from(sample_rate.max(1)) * config.frame_ms.max(1)) / 1000).max(1) as usize;
        Self {
            sample_rate: sample_rate.max(1),
            config,
            frame_samples,
            pending: Vec::with_capacity(frame_samples * 2),
            consecutive_speech_frames: 0,
            speech_started: false,
            trailing_silence_samples: 0,
            total_samples: 0,
        }
    }

    fn push_samples(&mut self, samples: &[f32]) -> Option<CaptureStopReason> {
        self.pending.extend_from_slice(samples);
        self.total_samples = self.total_samples.saturating_add(samples.len());

        let mut offset = 0_usize;
        while self.pending.len().saturating_sub(offset) >= self.frame_samples {
            let frame = &self.pending[offset..offset + self.frame_samples];
            let (rms, _) = compute_rms_and_peak(frame);
            if rms >= self.config.rms_threshold {
                self.consecutive_speech_frames = self.consecutive_speech_frames.saturating_add(1);
                if self.consecutive_speech_frames >= 2 {
                    self.speech_started = true;
                }
                self.trailing_silence_samples = 0;
            } else {
                self.consecutive_speech_frames = 0;
                if self.speech_started {
                    self.trailing_silence_samples =
                        self.trailing_silence_samples.saturating_add(self.frame_samples);
                }
            }
            offset += self.frame_samples;
        }
        if offset > 0 {
            self.pending.drain(..offset);
        }

        if self.config.auto_stop && self.speech_started {
            let trailing_ms = (self.trailing_silence_samples as u64).saturating_mul(1000)
                / u64::from(self.sample_rate);
            if trailing_ms >= self.config.trailing_silence_ms {
                return Some(CaptureStopReason::AutoStopSilence);
            }
        }

        let total_ms =
            (self.total_samples as u64).saturating_mul(1000) / u64::from(self.sample_rate);
        if total_ms >= self.config.max_duration_ms {
            return Some(CaptureStopReason::MaxDurationReached);
        }

        None
    }
}

struct MonitorSharedState {
    latest_snapshot: MicLevelSnapshot,
    vad_threshold: f32,
    stream_error: Option<String>,
}

/// Live microphone monitor stream providing non-blocking signal levels for UI calibration.
pub struct MicMonitorHandle {
    device_name: String,
    sample_rate: u32,
    state: Arc<Mutex<MonitorSharedState>>,
    stop_flag: Arc<AtomicBool>,
    join_handle: Option<JoinHandle<()>>,
}

impl MicMonitorHandle {
    /// Opens the selected (or default) microphone input stream and begins monitoring RMS/peak levels.
    pub fn start(preferred_device: Option<&str>, vad_threshold: f32) -> Result<Self> {
        let preferred = preferred_device.map(str::to_string);
        let state = Arc::new(Mutex::new(MonitorSharedState {
            latest_snapshot: MicLevelSnapshot::default(),
            vad_threshold,
            stream_error: None,
        }));
        let stop_flag = Arc::new(AtomicBool::new(false));
        let (init_tx, init_rx) = mpsc::channel::<Result<(String, u32)>>();

        let thread_state = Arc::clone(&state);
        let thread_stop = Arc::clone(&stop_flag);

        let join_handle = thread::Builder::new()
            .name("kotodama-mic-monitor".to_string())
            .spawn(move || {
                let setup = (|| -> Result<(cpal::Stream, String, u32)> {
                    let (device, device_name) = resolve_input_device(preferred.as_deref())?;
                    let supported = device
                        .default_input_config()
                        .with_context(|| format!("Microphone '{device_name}' has no supported input configuration"))?;
                    let sample_rate = supported.sample_rate();

                    let cb_state = Arc::clone(&thread_state);
                    let err_state = Arc::clone(&thread_state);
                    let stream = build_mono_input_stream_with_err(
                        &device,
                        &supported,
                        move |mono_chunk| {
                            if let Ok(mut guard) = cb_state.lock() {
                                let threshold = guard.vad_threshold;
                                guard.latest_snapshot =
                                    MicLevelSnapshot::from_samples(mono_chunk, threshold);
                            }
                        },
                        move |err_msg| {
                            if let Ok(mut guard) = err_state.lock() {
                                guard.stream_error = Some(err_msg);
                            }
                        },
                    )?;

                    stream
                        .play()
                        .context("Failed to start microphone monitoring stream")?;
                    Ok((stream, device_name, sample_rate))
                })();

                match setup {
                    Ok((stream, device_name, sample_rate)) => {
                        let _ = init_tx.send(Ok((device_name, sample_rate)));
                        while !thread_stop.load(Ordering::Relaxed) {
                            thread::sleep(Duration::from_millis(30));
                        }
                        drop(stream);
                    }
                    Err(err) => {
                        let _ = init_tx.send(Err(err));
                    }
                }
            })
            .context("Failed to spawn microphone monitor thread")?;

        match init_rx.recv() {
            Ok(Ok((device_name, sample_rate))) => Ok(Self {
                device_name,
                sample_rate,
                state,
                stop_flag,
                join_handle: Some(join_handle),
            }),
            Ok(Err(err)) => {
                stop_flag.store(true, Ordering::Relaxed);
                let _ = join_handle.join();
                Err(err)
            }
            Err(_) => {
                stop_flag.store(true, Ordering::Relaxed);
                let _ = join_handle.join();
                bail!("Microphone monitor worker disconnected during startup")
            }
        }
    }

    /// Returns the name of the active microphone input device.
    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    /// Returns the hardware sampling rate in Hertz of the active monitor stream.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Updates the VAD RMS threshold used when classifying live monitor levels.
    pub fn set_vad_threshold(&self, threshold: f32) {
        if let Ok(mut guard) = self.state.lock() {
            guard.vad_threshold = threshold;
        }
    }

    /// Returns the most recently observed [`MicLevelSnapshot`].
    pub fn snapshot(&self) -> MicLevelSnapshot {
        self.state
            .lock()
            .map(|guard| guard.latest_snapshot)
            .unwrap_or_default()
    }

    /// Returns and clears any asynchronous audio stream error reported by the backend.
    pub fn take_error(&self) -> Option<String> {
        self.state
            .lock()
            .ok()
            .and_then(|mut guard| guard.stream_error.take())
    }

    /// Stops the live microphone monitor stream and joins its background thread.
    pub fn stop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        if let Some(handle) = self.join_handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for MicMonitorHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

struct RecordingSharedState {
    samples: Vec<f32>,
    latest_snapshot: MicLevelSnapshot,
    stop_reason: Option<CaptureStopReason>,
    stream_error: Option<String>,
}

/// Active microphone recording session supporting live telemetry and VAD auto-stop.
pub struct RecordingSession {
    device_name: String,
    sample_rate: u32,
    vad_config: VadConfig,
    state: Arc<Mutex<RecordingSharedState>>,
    stop_flag: Arc<AtomicBool>,
    finished_flag: Arc<AtomicBool>,
    join_handle: Option<JoinHandle<()>>,
}

impl RecordingSession {
    /// Starts capturing mono audio from the selected (or default) microphone with VAD tracking.
    pub fn start(preferred_device: Option<&str>, vad_config: VadConfig) -> Result<Self> {
        vad_config.validate()?;
        let preferred = preferred_device.map(str::to_string);

        let state = Arc::new(Mutex::new(RecordingSharedState {
            samples: Vec::with_capacity(16_000 * 4),
            latest_snapshot: MicLevelSnapshot::default(),
            stop_reason: None,
            stream_error: None,
        }));
        let stop_flag = Arc::new(AtomicBool::new(false));
        let finished_flag = Arc::new(AtomicBool::new(false));
        let (init_tx, init_rx) = mpsc::channel::<Result<(String, u32)>>();

        let thread_state = Arc::clone(&state);
        let thread_stop = Arc::clone(&stop_flag);
        let thread_finished = Arc::clone(&finished_flag);
        let worker_vad = vad_config.clone();

        let join_handle = thread::Builder::new()
            .name("kotodama-mic-capture".to_string())
            .spawn(move || {
                let setup = (|| -> Result<(cpal::Stream, String, u32)> {
                    let (device, device_name) = resolve_input_device(preferred.as_deref())?;
                    let supported = device
                        .default_input_config()
                        .with_context(|| format!("Microphone '{device_name}' has no supported input configuration"))?;
                    let sample_rate = supported.sample_rate();

                    let mut tracker = VadAutoStopTracker::new(sample_rate, worker_vad.clone());
                    let cb_state = Arc::clone(&thread_state);
                    let cb_stop = Arc::clone(&thread_stop);
                    let cb_finished = Arc::clone(&thread_finished);
                    let err_state = Arc::clone(&thread_state);
                    let err_stop = Arc::clone(&thread_stop);
                    let err_finished = Arc::clone(&thread_finished);

                    let stream = build_mono_input_stream_with_err(
                        &device,
                        &supported,
                        move |mono_chunk| {
                            if cb_stop.load(Ordering::Relaxed) {
                                return;
                            }
                            let snapshot =
                                MicLevelSnapshot::from_samples(mono_chunk, worker_vad.rms_threshold);
                            let maybe_stop = tracker.push_samples(mono_chunk);
                            if let Ok(mut guard) = cb_state.lock() {
                                guard.samples.extend_from_slice(mono_chunk);
                                guard.latest_snapshot = snapshot;
                                if let Some(reason) = maybe_stop {
                                    guard.stop_reason = Some(reason);
                                    cb_stop.store(true, Ordering::Relaxed);
                                    cb_finished.store(true, Ordering::Relaxed);
                                }
                            }
                        },
                        move |err_msg| {
                            if let Ok(mut guard) = err_state.lock() {
                                guard.stream_error = Some(err_msg);
                            }
                            err_stop.store(true, Ordering::Relaxed);
                            err_finished.store(true, Ordering::Relaxed);
                        },
                    )?;

                    stream
                        .play()
                        .context("Failed to start microphone recording stream")?;
                    Ok((stream, device_name, sample_rate))
                })();

                match setup {
                    Ok((stream, device_name, sample_rate)) => {
                        let _ = init_tx.send(Ok((device_name, sample_rate)));
                        while !thread_stop.load(Ordering::Relaxed) {
                            thread::sleep(Duration::from_millis(25));
                        }
                        drop(stream);
                        thread_finished.store(true, Ordering::Relaxed);
                    }
                    Err(err) => {
                        thread_finished.store(true, Ordering::Relaxed);
                        let _ = init_tx.send(Err(err));
                    }
                }
            })
            .context("Failed to spawn microphone recording thread")?;

        match init_rx.recv() {
            Ok(Ok((device_name, sample_rate))) => Ok(Self {
                device_name,
                sample_rate,
                vad_config,
                state,
                stop_flag,
                finished_flag,
                join_handle: Some(join_handle),
            }),
            Ok(Err(err)) => {
                stop_flag.store(true, Ordering::Relaxed);
                let _ = join_handle.join();
                Err(err)
            }
            Err(_) => {
                stop_flag.store(true, Ordering::Relaxed);
                let _ = join_handle.join();
                bail!("Microphone capture worker disconnected during startup")
            }
        }
    }

    /// Returns the name of the active recording microphone device.
    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    /// Returns the most recently observed [`MicLevelSnapshot`].
    pub fn snapshot(&self) -> MicLevelSnapshot {
        self.state
            .lock()
            .map(|guard| guard.latest_snapshot)
            .unwrap_or_default()
    }

    /// Returns the elapsed captured audio duration in milliseconds.
    pub fn elapsed_ms(&self) -> u64 {
        let count = self
            .state
            .lock()
            .map(|guard| guard.samples.len() as u64)
            .unwrap_or(0);
        count.saturating_mul(1000) / u64::from(self.sample_rate.max(1))
    }

    /// Returns `true` when VAD auto-stop, max duration, or a stream error ended capture.
    pub fn is_finished(&self) -> bool {
        self.finished_flag.load(Ordering::Relaxed)
    }

    /// Returns and clears any asynchronous audio stream error reported during recording.
    pub fn take_error(&self) -> Option<String> {
        self.state
            .lock()
            .ok()
            .and_then(|mut guard| guard.stream_error.take())
    }

    /// Stops recording, trims silence using VAD, and returns the completed [`RecordedTake`].
    pub fn finish(mut self) -> Result<RecordedTake> {
        self.stop_flag.store(true, Ordering::Relaxed);
        if let Some(handle) = self.join_handle.take() {
            let _ = handle.join();
        }

        let mut guard = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("Microphone recording state lock was poisoned"))?;

        if let Some(err_msg) = guard.stream_error.take()
            && guard.samples.is_empty()
        {
            bail!("{err_msg}");
        }

        let samples = std::mem::take(&mut guard.samples);
        let (_, peak_level) = compute_rms_and_peak(&samples);
        let stop_reason = guard.stop_reason.unwrap_or(CaptureStopReason::Manual);
        drop(guard);

        let raw_audio = DecodedAudio {
            samples,
            sample_rate: self.sample_rate,
            channels: 1,
        };
        let (trimmed_audio, speech_detected) = trim_silence_vad(&raw_audio, &self.vad_config);

        Ok(RecordedTake {
            raw_audio,
            trimmed_audio,
            peak_level,
            speech_detected,
            stop_reason,
        })
    }
}

impl Drop for RecordingSession {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        if let Some(handle) = self.join_handle.take() {
            let _ = handle.join();
        }
    }
}

fn resolve_input_device(preferred_name: Option<&str>) -> Result<(cpal::Device, String)> {
    let host = cpal::default_host();
    if let Some(target_name) = preferred_name.map(str::trim).filter(|name| !name.is_empty()) {
        let mut devices = host
            .input_devices()
            .context("Failed to enumerate system microphone devices")?;
        let found = devices
            .find(|device| device.to_string() == target_name)
            .with_context(|| {
                format!("Preferred microphone '{target_name}' was not found or is disconnected")
            })?;
        let resolved_name = found.to_string();
        return Ok((found, resolved_name));
    }

    let device = host
        .default_input_device()
        .context("No default microphone input device is available")?;
    let name = device.to_string();
    Ok((device, name))
}

fn build_mono_input_stream_with_err<F, E>(
    device: &cpal::Device,
    supported_config: &SupportedStreamConfig,
    on_mono_chunk: F,
    on_error: E,
) -> Result<cpal::Stream>
where
    F: FnMut(&[f32]) + Send + 'static,
    E: FnMut(String) + Send + 'static,
{
    let channels = usize::from(supported_config.channels()).max(1);
    let sample_format = supported_config.sample_format();
    let stream_config = supported_config.config();

    match sample_format {
        SampleFormat::F32 => {
            build_typed_mono_stream::<f32, _, _>(device, stream_config, channels, on_mono_chunk, on_error)
        }
        SampleFormat::F64 => {
            build_typed_mono_stream::<f64, _, _>(device, stream_config, channels, on_mono_chunk, on_error)
        }
        SampleFormat::I16 => {
            build_typed_mono_stream::<i16, _, _>(device, stream_config, channels, on_mono_chunk, on_error)
        }
        SampleFormat::U16 => {
            build_typed_mono_stream::<u16, _, _>(device, stream_config, channels, on_mono_chunk, on_error)
        }
        SampleFormat::I32 => {
            build_typed_mono_stream::<i32, _, _>(device, stream_config, channels, on_mono_chunk, on_error)
        }
        SampleFormat::I8 => {
            build_typed_mono_stream::<i8, _, _>(device, stream_config, channels, on_mono_chunk, on_error)
        }
        SampleFormat::U8 => {
            build_typed_mono_stream::<u8, _, _>(device, stream_config, channels, on_mono_chunk, on_error)
        }
        other => bail!("Unsupported microphone sample format: {other:?}"),
    }
}

fn build_typed_mono_stream<T, F, E>(
    device: &cpal::Device,
    stream_config: cpal::StreamConfig,
    channels: usize,
    mut on_mono_chunk: F,
    mut on_error: E,
) -> Result<cpal::Stream>
where
    T: SizedSample,
    f32: FromSample<T>,
    F: FnMut(&[f32]) + Send + 'static,
    E: FnMut(String) + Send + 'static,
{
    let mut mono_scratch = Vec::<f32>::with_capacity(2048);
    let inv_channels = 1.0 / channels as f32;

    device
        .build_input_stream::<T, _, _>(
            stream_config,
            move |data, _| {
                mono_scratch.clear();
                if channels == 1 {
                    mono_scratch.extend(data.iter().map(|&sample| f32::from_sample(sample)));
                } else {
                    for frame in data.chunks_exact(channels) {
                        let sum: f32 = frame.iter().map(|&sample| f32::from_sample(sample)).sum();
                        mono_scratch.push(sum * inv_channels);
                    }
                }
                if !mono_scratch.is_empty() {
                    on_mono_chunk(&mono_scratch);
                }
            },
            move |err| {
                on_error(format!("Microphone stream error: {err}"));
            },
            None,
        )
        .context("Failed to initialize microphone input stream")
}

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;

    use super::*;

    fn sine_samples(sample_rate: u32, duration_ms: u64, freq_hz: f32, amp: f32) -> Vec<f32> {
        let len = (u64::from(sample_rate) * duration_ms / 1000) as usize;
        (0..len)
            .map(|i| (TAU * freq_hz * i as f32 / sample_rate as f32).sin() * amp)
            .collect()
    }

    #[test]
    fn rms_dbfs_and_meter_ratios_behave_accurately() {
        let (rms_empty, peak_empty) = compute_rms_and_peak(&[]);
        assert_eq!(rms_empty, 0.0);
        assert_eq!(peak_empty, 0.0);
        assert!((rms_to_dbfs(0.0) + 60.0).abs() < 1e-4);
        assert_eq!(dbfs_to_meter_ratio(-60.0), 0.0);
        assert_eq!(dbfs_to_meter_ratio(0.0), 1.0);

        let sine = sine_samples(16_000, 100, 220.0, 0.5);
        let (rms, peak) = compute_rms_and_peak(&sine);
        // RMS of 0.5-amplitude sine is 0.5 / sqrt(2) ~= 0.3536.
        assert!((rms - 0.3536).abs() < 0.01);
        assert!((peak - 0.5).abs() < 0.01);
        assert!(vad_threshold_to_meter_ratio(0.025) > 0.3);
        assert!(vad_threshold_to_meter_ratio(0.025) < 0.6);
    }

    #[test]
    fn trim_silence_vad_strips_leading_and_trailing_silence_with_padding() {
        let sample_rate = 16_000_u32;
        let mut samples = vec![0.0_f32; 8_000]; // 500 ms silence
        samples.extend(sine_samples(sample_rate, 300, 200.0, 0.4)); // 300 ms speech
        samples.extend(vec![0.0_f32; 8_000]); // 500 ms silence

        let audio = DecodedAudio {
            samples,
            sample_rate,
            channels: 1,
        };
        let config = VadConfig {
            rms_threshold: 0.05,
            padding_ms: 40,
            frame_ms: 20,
            ..VadConfig::default()
        };

        let (trimmed, detected) = trim_silence_vad(&audio, &config);
        assert!(detected);
        assert_eq!(trimmed.channels, 1);
        // 300 ms speech + 2 * 40 ms padding = 380 ms (allow frame quantization).
        assert!((360..=420).contains(&trimmed.duration_ms()));

        let stereo = DecodedAudio {
            samples: audio.samples.iter().flat_map(|&s| [s, s]).collect(),
            sample_rate,
            channels: 2,
        };
        let (trimmed_stereo, stereo_detected) = trim_silence_vad(&stereo, &config);
        assert!(stereo_detected);
        assert_eq!(trimmed_stereo.channels, 1);
        assert_eq!(trimmed_stereo.samples.len(), trimmed.samples.len());

        let all_silence = DecodedAudio {
            samples: vec![0.001_f32; 8_000],
            sample_rate,
            channels: 1,
        };
        let (empty_trimmed, none_detected) = trim_silence_vad(&all_silence, &config);
        assert!(!none_detected);
        assert!(empty_trimmed.samples.is_empty());
    }

    #[test]
    fn vad_auto_stop_tracker_ignores_single_click_and_stops_after_trailing_silence() {
        let sample_rate = 1000_u32;
        let config = VadConfig {
            rms_threshold: 0.1,
            trailing_silence_ms: 200,
            frame_ms: 20,
            padding_ms: 20,
            auto_stop: true,
            max_duration_ms: 2000,
        };
        let mut tracker = VadAutoStopTracker::new(sample_rate, config);

        // Single 20 ms click followed by 300 ms silence should NOT arm speech or auto-stop.
        assert_eq!(tracker.push_samples(&[0.5; 20]), None);
        assert_eq!(tracker.push_samples(&[0.0; 300]), None);

        // Two consecutive voiced frames (40 ms) arm speech detection.
        assert_eq!(tracker.push_samples(&[0.4; 40]), None);
        // 160 ms silence is still below 200 ms threshold.
        assert_eq!(tracker.push_samples(&[0.0; 160]), None);
        // Another 40 ms brings trailing silence to 200 ms -> AutoStopSilence.
        assert_eq!(
            tracker.push_samples(&[0.0; 40]),
            Some(CaptureStopReason::AutoStopSilence)
        );
    }

    #[test]
    fn vad_auto_stop_tracker_respects_manual_mode_and_max_duration_cap() {
        let sample_rate = 1000_u32;
        let config = VadConfig {
            rms_threshold: 0.1,
            trailing_silence_ms: 100,
            frame_ms: 20,
            padding_ms: 20,
            auto_stop: false,
            max_duration_ms: 500,
        };
        let mut tracker = VadAutoStopTracker::new(sample_rate, config);

        assert_eq!(tracker.push_samples(&[0.5; 60]), None);
        assert_eq!(tracker.push_samples(&[0.0; 300]), None);
        assert_eq!(
            tracker.push_samples(&[0.0; 140]),
            Some(CaptureStopReason::MaxDurationReached)
        );
    }
}

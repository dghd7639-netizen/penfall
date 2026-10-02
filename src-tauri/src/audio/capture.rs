use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot};

/// Reject brief impulses: Qwen3 ASR can transcribe silence/key clicks as "嗯".
/// Only runs above the adaptive floor lasting at least this long count as voice.
const MIN_VOICE_US: u64 = 50_000;
/// A few 16-bit PCM steps, rather than a threshold tuned to one microphone.
const MIN_VOICE_RMS: f32 = 0.0001;

#[derive(Clone, Copy, Default)]
struct VoiceActivity {
    noise_rms: Option<f32>,
    run_us: u64,
    voiced_us: u64,
}

/// Pure, allocation-free energy gate shared by dictation and Ask capture.
/// Prime the floor from real input, follow quiet samples quickly and rising
/// background slowly; above-floor speech/impulses must not raise the floor.
// ponytail: RMS contrast cannot distinguish sustained noise from speech or an
// entirely steady utterance present at startup; use spectral VAD if that matters.
fn measure_voice(
    mut activity: VoiceActivity,
    samples: &[f32],
    sample_rate: u32,
    channels: u16,
    muted: bool,
) -> VoiceActivity {
    if muted || samples.is_empty() || sample_rate == 0 || channels == 0 {
        activity.run_us = 0;
        return activity;
    }
    // Keep an impulse short even when CPAL delivers a large callback buffer.
    let window_samples = (sample_rate / 100).max(1) as usize * usize::from(channels);
    for window in samples.chunks(window_samples) {
        let rms = (window.iter().map(|s| s * s).sum::<f32>() / window.len() as f32).sqrt();
        let frames = window.len() as u64 / u64::from(channels);
        let duration_us = frames * 1_000_000 / u64::from(sample_rate);
        if !rms.is_finite() || duration_us == 0 {
            activity.run_us = 0;
            continue;
        }
        let floor = *activity.noise_rms.get_or_insert(rms);
        if rms > (floor * 3.0).max(MIN_VOICE_RMS) {
            let previous_run = activity.run_us;
            activity.run_us = activity.run_us.saturating_add(duration_us);
            if activity.run_us >= MIN_VOICE_US {
                // Credit the onset once confirmed, then the rest of this run.
                let confirmed_us = if previous_run < MIN_VOICE_US {
                    activity.run_us
                } else {
                    duration_us
                };
                activity.voiced_us = activity.voiced_us.saturating_add(confirmed_us);
            }
        } else {
            activity.run_us = 0;
            let rise = (duration_us as f32 / 250_000.0).min(1.0);
            activity.noise_rms = Some(if rms < floor {
                rms
            } else {
                floor + (rms - floor) * rise
            });
        }
    }
    activity
}

struct CaptureStartupNotifier {
    sender: Option<
        oneshot::Sender<std::result::Result<crate::recording_deadline::CaptureReadyAt, String>>,
    >,
}

struct CaptureStartupWaiter {
    receiver:
        oneshot::Receiver<std::result::Result<crate::recording_deadline::CaptureReadyAt, String>>,
}

fn capture_startup_channel() -> (CaptureStartupNotifier, CaptureStartupWaiter) {
    let (sender, receiver) = oneshot::channel();
    (
        CaptureStartupNotifier {
            sender: Some(sender),
        },
        CaptureStartupWaiter { receiver },
    )
}

impl CaptureStartupNotifier {
    fn ready(&mut self, ready_at: crate::recording_deadline::CaptureReadyAt) {
        if let Some(sender) = self.sender.take() {
            let _ = sender.send(Ok(ready_at));
        }
    }

    fn failed(&mut self, message: String) {
        if let Some(sender) = self.sender.take() {
            let _ = sender.send(Err(message));
        }
    }
}

impl CaptureStartupWaiter {
    async fn wait(self) -> std::result::Result<crate::recording_deadline::CaptureReadyAt, String> {
        self.receiver.await.unwrap_or_else(|_| {
            Err("Audio capture thread ended before reporting readiness".to_string())
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CaptureState {
    Idle,
    Starting,
    Recording,
}

fn initial_capture_state() -> CaptureState {
    CaptureState::Starting
}

#[derive(Debug, Clone)]
pub struct AudioConfig {
    pub sample_rate: u32,
    pub channels: u16,
    pub chunk_duration_ms: u32,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            sample_rate: 16000,
            channels: 1,
            chunk_duration_ms: 20,
        }
    }
}

/// Maximum audio buffer size in samples before we stop accumulating.
/// ~24 MB of i16 samples ≈ 12.5 min at 16kHz mono, matching the STT provider limits.
const MAX_BUFFER_SAMPLES: usize = 12 * 1024 * 1024;
const AUDIO_CHANNEL_BUFFER_DURATION_MS: u32 = 60_000;

fn audio_channel_capacity(config: &AudioConfig) -> usize {
    let chunk_duration_ms = config.chunk_duration_ms.max(1);
    AUDIO_CHANNEL_BUFFER_DURATION_MS.div_ceil(chunk_duration_ms) as usize
}

fn heard_voice(voiced_us: u64) -> bool {
    voiced_us >= MIN_VOICE_US
}

/// Handle to control audio capture running on a dedicated thread.
/// This is Send + Sync safe because it only holds channels and atomic state.
pub struct AudioCaptureHandle {
    stop_tx: Option<std::sync::mpsc::Sender<()>>,
    startup_waiter: Option<CaptureStartupWaiter>,
    volume: Arc<Mutex<f32>>,
    voiced_us: Arc<AtomicU64>,
    state: Arc<Mutex<CaptureState>>,
}

impl AudioCaptureHandle {
    /// Start audio capture on a dedicated thread. Returns a handle and a receiver for audio chunks.
    pub fn start(config: AudioConfig) -> Result<(Self, mpsc::Receiver<Vec<u8>>)> {
        let (audio_tx, audio_rx) = mpsc::channel::<Vec<u8>>(audio_channel_capacity(&config));
        let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
        let volume = Arc::new(Mutex::new(0.0f32));
        let voiced_us = Arc::new(AtomicU64::new(0));
        let state = Arc::new(Mutex::new(initial_capture_state()));
        let (mut startup_notifier, startup_waiter) = capture_startup_channel();

        let vol_clone = volume.clone();
        let voiced_clone = voiced_us.clone();
        let state_clone = state.clone();
        let failed_state = state.clone();

        // Audio capture must run on a dedicated OS thread because cpal::Stream is !Send
        std::thread::spawn(move || {
            if let Err(e) = run_capture(
                config,
                audio_tx,
                stop_rx,
                vol_clone,
                voiced_clone,
                state_clone,
                &mut startup_notifier,
            ) {
                *failed_state
                    .lock()
                    .unwrap_or_else(|error| error.into_inner()) = CaptureState::Idle;
                startup_notifier.failed(e.to_string());
                tracing::error!("Audio capture thread error: {}", e);
            }
        });

        Ok((
            Self {
                stop_tx: Some(stop_tx),
                startup_waiter: Some(startup_waiter),
                volume,
                voiced_us,
                state,
            },
            audio_rx,
        ))
    }

    /// Whether the microphone picked up enough voice for the recording to hold speech.
    pub fn heard_voice(&self) -> bool {
        heard_voice(self.voiced_us.load(Ordering::Relaxed))
    }

    /// Wait until the platform backend has opened the input stream and
    /// `play()` has succeeded. The CPAL boundary is shared by CoreAudio,
    /// WASAPI, ALSA and PipeWire, so callers do not need platform delays.
    pub async fn wait_until_ready(&mut self) -> Result<crate::recording_deadline::CaptureReadyAt> {
        let waiter = self
            .startup_waiter
            .take()
            .ok_or_else(|| anyhow::anyhow!("Audio capture readiness was already consumed"))?;
        waiter.wait().await.map_err(anyhow::Error::msg)
    }

    pub fn stop(&mut self) {
        // Signal the capture thread to stop
        self.stop_tx = None;
        *self.volume.lock().unwrap_or_else(|e| e.into_inner()) = 0.0;
        *self.state.lock().unwrap_or_else(|e| e.into_inner()) = CaptureState::Idle;
    }

    pub fn get_volume(&self) -> f32 {
        *self.volume.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn state(&self) -> CaptureState {
        *self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Downsample audio from `from_rate` to `to_rate` (simple linear interpolation, mono).
fn downsample(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate {
        return samples.to_vec();
    }
    let ratio = from_rate as f64 / to_rate as f64;
    let out_len = (samples.len() as f64 / ratio) as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src_idx = i as f64 * ratio;
        let idx = src_idx as usize;
        let frac = src_idx - idx as f64;
        let s = if idx + 1 < samples.len() {
            samples[idx] as f64 * (1.0 - frac) + samples[idx + 1] as f64 * frac
        } else {
            samples[idx.min(samples.len() - 1)] as f64
        };
        out.push(s as f32);
    }
    out
}

/// Mix multi-channel audio down to mono by averaging channels.
fn to_mono(samples: &[f32], channels: u16) -> Vec<f32> {
    if channels <= 1 {
        return samples.to_vec();
    }
    let ch = channels as usize;
    samples
        .chunks(ch)
        .map(|frame| frame.iter().sum::<f32>() / ch as f32)
        .collect()
}

fn run_capture(
    config: AudioConfig,
    sender: mpsc::Sender<Vec<u8>>,
    stop_rx: std::sync::mpsc::Receiver<()>,
    volume: Arc<Mutex<f32>>,
    voiced_us: Arc<AtomicU64>,
    state: Arc<Mutex<CaptureState>>,
    startup_notifier: &mut CaptureStartupNotifier,
) -> Result<()> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| anyhow::anyhow!("No input device available"))?;

    tracing::info!("Using input device: {:?}", device.name());

    // Use the device's default config instead of forcing 16kHz mono
    let default_config = device.default_input_config()?;
    let device_sample_rate = default_config.sample_rate().0;
    let device_channels = default_config.channels();

    tracing::info!(
        "Device default config: {}Hz, {} channels, format: {:?}",
        device_sample_rate,
        device_channels,
        default_config.sample_format()
    );

    let stream_config = cpal::StreamConfig {
        channels: device_channels,
        sample_rate: cpal::SampleRate(device_sample_rate),
        buffer_size: cpal::BufferSize::Default,
    };

    let target_rate = config.sample_rate;
    let target_channels = config.channels;
    let samples_per_chunk = (target_rate * config.chunk_duration_ms / 1000) as usize;
    let buffer: Arc<Mutex<Vec<i16>>> = Arc::new(Mutex::new(Vec::with_capacity(samples_per_chunk)));
    let callback_voiced_us = voiced_us.clone();
    let mut voice_activity = VoiceActivity::default();

    let stream = device.build_input_stream(
        &stream_config,
        move |data: &[f32], _: &cpal::InputCallbackInfo| {
            let muted = super::cue::input_muted();
            let silence;
            let data = if muted {
                silence = vec![0.0f32; data.len()];
                silence.as_slice()
            } else {
                data
            };

            // Calculate RMS volume from raw data
            let rms = if data.is_empty() {
                0.0
            } else {
                (data.iter().map(|s| s * s).sum::<f32>() / data.len() as f32).sqrt()
            };
            if let Ok(mut v) = volume.lock() {
                *v = rms.min(1.0);
            }
            voice_activity = measure_voice(
                voice_activity,
                data,
                device_sample_rate,
                device_channels,
                muted,
            );
            callback_voiced_us.store(voice_activity.voiced_us, Ordering::Relaxed);

            // Convert to mono if needed
            let mono = if device_channels > target_channels {
                to_mono(data, device_channels)
            } else {
                data.to_vec()
            };

            // Downsample to target rate if needed
            let resampled = if device_sample_rate != target_rate {
                downsample(&mono, device_sample_rate, target_rate)
            } else {
                mono
            };

            // Convert f32 to i16 PCM and buffer
            let mut buf = buffer.lock().unwrap_or_else(|e| e.into_inner());
            for &sample in &resampled {
                if buf.len() >= MAX_BUFFER_SAMPLES {
                    break;
                }
                let s = (sample * 32767.0).clamp(-32768.0, 32767.0) as i16;
                buf.push(s);
            }

            // Send complete chunks
            while buf.len() >= samples_per_chunk {
                let chunk: Vec<i16> = buf.drain(..samples_per_chunk).collect();
                let bytes: Vec<u8> = chunk.iter().flat_map(|s| s.to_le_bytes()).collect();
                let _ = sender.try_send(bytes);
            }
        },
        |err| {
            tracing::error!("Audio capture error: {}", err);
        },
        None,
    )?;

    stream.play()?;
    let capture_ready_at = crate::recording_deadline::CaptureReadyAt::now();
    *state.lock().unwrap_or_else(|e| e.into_inner()) = CaptureState::Recording;
    startup_notifier.ready(capture_ready_at);
    tracing::info!(
        "Audio capture started (device: {}Hz {}ch -> target: {}Hz {}ch)",
        device_sample_rate,
        device_channels,
        target_rate,
        target_channels
    );

    // Block until stop signal (sender dropped)
    let _ = stop_rx.recv();

    // Stream is dropped here, stopping capture
    drop(stream);
    *state.lock().unwrap_or_else(|e| e.into_inner()) = CaptureState::Idle;
    tracing::info!(
        "Audio capture stopped ({} ms of voice)",
        voiced_us.load(Ordering::Relaxed) / 1000
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn level(activity: VoiceActivity, rms: f32, ms: usize) -> VoiceActivity {
        measure_voice(activity, &vec![rms; 16 * ms], 16_000, 1, false)
    }

    #[test]
    fn adaptive_voice_accepts_quiet_speech_across_microphone_gains() {
        for gain in [0.25, 1.0, 4.0, 20.0] {
            let background = level(VoiceActivity::default(), 0.0002 * gain, 500);
            assert!(!heard_voice(background.voiced_us));
            let speech = level(background, 0.003 * gain, 200);
            assert!(heard_voice(speech.voiced_us), "gain {gain}");
        }
    }

    #[test]
    fn adaptive_voice_rejects_silence_stationary_noise_and_repeated_clicks() {
        for rms in [0.0, 0.0002, 0.005, 0.02] {
            let noise = level(VoiceActivity::default(), rms, 1000);
            assert!(!heard_voice(noise.voiced_us), "noise {rms}");
        }
        let mut activity = level(VoiceActivity::default(), 0.0002, 100);
        for _ in 0..100 {
            activity = level(activity, 0.2, 10);
            activity = level(activity, 0.0002, 60);
        }
        assert_eq!(activity.voiced_us, 0);
    }

    #[test]
    fn adaptive_voice_follows_changing_noise_without_learning_speech_as_noise() {
        let mut activity = VoiceActivity::default();
        for i in 0..200 {
            activity = level(activity, 0.0002 * (1.0 + i as f32 * 0.005), 10);
        }
        assert!(!heard_voice(activity.voiced_us));
        let higher_floor = activity.noise_rms.unwrap();
        assert!(higher_floor > 0.0003);
        activity = level(activity, 0.0001, 100);
        assert!(activity.noise_rms.unwrap() < higher_floor);
        let floor = activity.noise_rms;
        activity = level(activity, 0.002, 1000);
        assert!(heard_voice(activity.voiced_us));
        assert_eq!(activity.noise_rms, floor);
    }

    #[test]
    fn adaptive_voice_preserves_the_floor_across_muted_cues_and_invalid_input() {
        let background = level(VoiceActivity::default(), 0.002, 100);
        let muted = measure_voice(background, &vec![0.0; 4800], 16_000, 1, true);
        assert_eq!(muted.noise_rms, background.noise_rms);
        assert!(!heard_voice(level(muted, 0.002, 200).voiced_us));
        for bad in [f32::NAN, f32::INFINITY] {
            let invalid = level(background, bad, 10);
            assert_eq!(invalid.noise_rms, background.noise_rms);
            assert_eq!(invalid.voiced_us, 0);
        }
        assert_eq!(measure_voice(background, &[], 0, 0, false).voiced_us, 0);
    }

    #[test]
    fn adaptive_voice_handles_onset_and_requires_a_sustained_signal() {
        let onset = level(VoiceActivity::default(), 0.0002, 10);
        assert!(!heard_voice(level(onset, 0.003, 49).voiced_us));
        assert!(heard_voice(level(onset, 0.003, 50).voiced_us));
        let spike = level(VoiceActivity::default(), 0.5, 10);
        let recovered = level(spike, 0.0002, 100);
        assert!(heard_voice(level(recovered, 0.003, 100).voiced_us));
    }

    #[test]
    fn adaptive_voice_duration_uses_frames_not_samples_and_ignores_buffer_size() {
        for (rate, channels) in [(16_000, 1), (44_100, 2), (48_000, 2)] {
            let count = rate as usize / 10 * usize::from(channels);
            let background = measure_voice(
                VoiceActivity::default(),
                &vec![0.0002; count],
                rate,
                channels,
                false,
            );
            let speech = measure_voice(background, &vec![0.003; count], rate, channels, false);
            assert!(heard_voice(speech.voiced_us));
            assert!(speech.voiced_us.abs_diff(100_000) < 100);
            let mut click = vec![0.0002; count];
            click[..count / 10].fill(0.2);
            assert!(!heard_voice(
                measure_voice(background, &click, rate, channels, false).voiced_us
            ));
        }
    }

    #[test]
    fn capture_does_not_report_recording_before_the_backend_is_ready() {
        assert_eq!(initial_capture_state(), CaptureState::Starting);
    }

    #[test]
    fn a_single_noise_spike_is_not_voice_but_a_short_word_is() {
        // Observed on the user's mic: silence peaks for one ~11 ms buffer, "嗯" lasts ~384 ms.
        assert!(!heard_voice(0));
        assert!(!heard_voice(11_000));
        assert!(heard_voice(384_000));
    }

    #[test]
    fn audio_queue_preserves_a_minute_while_the_provider_connects() {
        assert_eq!(audio_channel_capacity(&AudioConfig::default()), 3_000);
    }

    #[tokio::test]
    async fn capture_startup_waits_for_the_backend_ready_signal() {
        let (_notifier, waiter) = capture_startup_channel();

        assert!(
            tokio::time::timeout(Duration::from_millis(20), waiter.wait())
                .await
                .is_err(),
            "capture startup completed before the backend reported readiness"
        );
    }

    #[tokio::test]
    async fn capture_startup_completes_after_the_backend_is_ready() {
        let (mut notifier, waiter) = capture_startup_channel();
        let ready_at = crate::recording_deadline::CaptureReadyAt::now();
        notifier.ready(ready_at);

        let observed = waiter.wait().await.unwrap();
        assert_eq!(observed.unix_millis, ready_at.unix_millis);
        assert_eq!(observed.monotonic, ready_at.monotonic);
    }

    #[tokio::test]
    async fn capture_startup_propagates_backend_failure() {
        let (mut notifier, waiter) = capture_startup_channel();
        notifier.failed("input device unavailable".to_string());

        assert_eq!(
            waiter.wait().await,
            Err("input device unavailable".to_string())
        );
    }
}

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use tauri::{AppHandle, Emitter};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const SAMPLE_RATE: u32 = 16_000;
const CHANNELS: u16 = 1;

/// RMS energy threshold to consider audio as "speech"
const VAD_THRESHOLD: f32 = 0.015;
/// Silence duration (ms) to end a recording segment
const SILENCE_TIMEOUT_MS: u64 = 600;
/// Maximum recording duration (seconds)
const MAX_RECORD_SECS: f32 = 3.0;
/// Ring buffer length in samples (~10 s at 16 kHz)
const RING_BUF_SAMPLES: usize = SAMPLE_RATE as usize * 10;

// ---------------------------------------------------------------------------
// VoiceStatus event payload
// ---------------------------------------------------------------------------

#[derive(Clone, serde::Serialize)]
struct VoiceStatusPayload {
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
}

// ---------------------------------------------------------------------------
// CommandParser
// ---------------------------------------------------------------------------

pub struct CommandParser;

impl CommandParser {
    /// Match recognized text to a decision string.
    /// Returns `Some((decision, matched_keyword))` or `None`.
    pub fn parse(text: &str) -> Option<(&'static str, &'static str)> {
        let t = text.to_lowercase();

        // --- exact / longer phrases first (allow-always) ---
        for kw in &["总是允许", "always", "总是", "一直"] {
            if t.contains(kw) {
                return Some(("allow-always", kw));
            }
        }

        // --- deny ---
        for kw in &["拒绝", "deny", "不要", "不", "no"] {
            if t.contains(kw) {
                return Some(("deny", kw));
            }
        }

        // --- allow (allow-once) ---
        for kw in &["允许", "allow", "yes", "可以", "好"] {
            if t.contains(kw) {
                return Some(("allow", kw));
            }
        }

        None
    }
}

// ---------------------------------------------------------------------------
// Shared inner state
// ---------------------------------------------------------------------------

struct VoiceInner {
    listening: AtomicBool,
    paused: AtomicBool,
}

// ---------------------------------------------------------------------------
// VoiceManager
// ---------------------------------------------------------------------------

pub struct VoiceManager {
    inner: Arc<VoiceInner>,
    whisper_ctx: Arc<Mutex<Option<WhisperContext>>>,
    model_path: String,
}

impl VoiceManager {
    /// Create a new VoiceManager.
    /// `model_path` – path to the ggml whisper tiny model file.
    /// The whisper model is loaded eagerly; returns `Err` if loading fails.
    pub fn new(model_path: &str) -> Result<Self, String> {
        let ctx = WhisperContext::new_with_params(model_path, WhisperContextParameters::default())
            .map_err(|e| format!("Failed to load whisper model at {}: {}", model_path, e))?;

        Ok(Self {
            inner: Arc::new(VoiceInner {
                listening: AtomicBool::new(false),
                paused: AtomicBool::new(false),
            }),
            whisper_ctx: Arc::new(Mutex::new(Some(ctx))),
            model_path: model_path.to_string(),
        })
    }

    /// Start listening for voice commands.
    ///
    /// Audio is captured from the default input device at 16 kHz mono.
    /// VAD detects speech segments which are then fed to whisper for
    /// transcription. Recognised commands are delivered via `on_command`.
    ///
    /// `app_handle` is used to emit `voice-status` events to the frontend.
    pub fn start_listening<F>(&self, app_handle: AppHandle, on_command: F) -> Result<(), String>
    where
        F: Fn(&str, &str) + Send + 'static,
    {
        if self.inner.listening.load(Ordering::SeqCst) {
            return Err("Already listening".into());
        }
        self.inner.listening.store(true, Ordering::SeqCst);
        self.inner.paused.store(false, Ordering::SeqCst);

        let inner = self.inner.clone();
        let whisper_ctx = self.whisper_ctx.clone();
        let app = app_handle.clone();

        emit_status(&app, "listening", None);

        std::thread::spawn(move || {
            if let Err(e) = voice_loop(inner.clone(), whisper_ctx, app.clone(), on_command) {
                log::error!("Voice loop error: {}", e);
                inner.listening.store(false, Ordering::SeqCst);
                emit_status(&app, "idle", None);
            }
        });

        Ok(())
    }

    /// Stop listening and release the audio stream.
    pub fn stop_listening(&self) {
        self.inner.listening.store(false, Ordering::SeqCst);
        self.inner.paused.store(false, Ordering::SeqCst);
    }

    /// Whether the manager is currently listening.
    pub fn is_listening(&self) -> bool {
        self.inner.listening.load(Ordering::SeqCst)
    }

    /// Pause audio capture (e.g. during TTS playback).
    pub fn pause(&self) {
        self.inner.paused.store(true, Ordering::SeqCst);
    }

    /// Resume audio capture after a pause.
    pub fn resume(&self) {
        self.inner.paused.store(false, Ordering::SeqCst);
    }

    /// Whether capture is currently paused.
    pub fn is_paused(&self) -> bool {
        self.inner.paused.load(Ordering::SeqCst)
    }

    /// Path to the loaded whisper model.
    pub fn model_path(&self) -> &str {
        &self.model_path
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn emit_status(app: &AppHandle, status: &str, text: Option<String>) {
    let payload = VoiceStatusPayload {
        status: status.to_string(),
        text,
    };
    if let Err(e) = app.emit("voice-status", &payload) {
        log::warn!("Failed to emit voice-status: {}", e);
    }
}

fn compute_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
    (sum_sq / samples.len() as f32).sqrt()
}

// ---------------------------------------------------------------------------
// Core voice loop (runs on a dedicated thread)
// ---------------------------------------------------------------------------

fn voice_loop<F>(
    inner: Arc<VoiceInner>,
    whisper_ctx: Arc<Mutex<Option<WhisperContext>>>,
    app: AppHandle,
    on_command: F,
) -> Result<(), String>
where
    F: Fn(&str, &str) + Send + 'static,
{
    // --- Set up cpal input stream ---
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| "No input device available".to_string())?;

    log::info!("Using input device: {}", device.name().unwrap_or_default());

    let config = cpal::StreamConfig {
        channels: CHANNELS,
        sample_rate: cpal::SampleRate(SAMPLE_RATE),
        buffer_size: cpal::BufferSize::Default,
    };

    // Shared ring buffer between cpal callback and this thread
    let ring_buf: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::with_capacity(RING_BUF_SAMPLES)));

    let ring_clone = ring_buf.clone();
    let paused_flag = inner.clone();

    let stream = device
        .build_input_stream(
            &config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                // Skip samples while paused
                if paused_flag.paused.load(Ordering::Relaxed) {
                    return;
                }
                let mut buf = ring_clone.lock().unwrap();
                buf.extend_from_slice(data);
                // Keep ring buffer bounded
                if buf.len() > RING_BUF_SAMPLES {
                    let excess = buf.len() - RING_BUF_SAMPLES;
                    buf.drain(..excess);
                }
            },
            |err| {
                log::error!("cpal input stream error: {}", err);
            },
            None,
        )
        .map_err(|e| format!("Failed to build input stream: {}", e))?;

    stream
        .play()
        .map_err(|e| format!("Failed to start input stream: {}", e))?;

    // --- VAD + whisper loop ---
    let max_record_samples = (MAX_RECORD_SECS * SAMPLE_RATE as f32) as usize;
    let silence_samples = (SILENCE_TIMEOUT_MS as f32 / 1000.0 * SAMPLE_RATE as f32) as usize;
    // Process audio in chunks of ~20 ms
    let chunk_samples = (SAMPLE_RATE as usize) / 50; // 320 samples

    let mut recording = false;
    let mut recorded: Vec<f32> = Vec::new();
    let mut silence_count: usize = 0;
    let mut _speech_start: Option<Instant> = None;

    while inner.listening.load(Ordering::SeqCst) {
        // Sleep briefly to let audio accumulate
        std::thread::sleep(std::time::Duration::from_millis(20));

        if inner.paused.load(Ordering::Relaxed) {
            continue;
        }

        // Drain available samples from ring buffer
        let samples: Vec<f32> = {
            let mut buf = ring_buf.lock().unwrap();
            let drained: Vec<f32> = buf.drain(..).collect();
            drained
        };

        if samples.is_empty() {
            continue;
        }

        // Process in chunks
        for chunk in samples.chunks(chunk_samples) {
            let rms = compute_rms(chunk);

            if !recording {
                if rms > VAD_THRESHOLD {
                    // Speech onset
                    recording = true;
                    recorded.clear();
                    recorded.extend_from_slice(chunk);
                    silence_count = 0;
                    _speech_start = Some(Instant::now());
                    log::debug!("VAD: speech onset (rms={:.4})", rms);
                }
            } else {
                recorded.extend_from_slice(chunk);

                if rms < VAD_THRESHOLD {
                    silence_count += chunk.len();
                } else {
                    silence_count = 0;
                }

                let should_stop =
                    silence_count >= silence_samples || recorded.len() >= max_record_samples;

                if should_stop {
                    log::info!(
                        "VAD: segment complete ({} samples, {:.2}s)",
                        recorded.len(),
                        recorded.len() as f32 / SAMPLE_RATE as f32
                    );

                    // --- Run whisper inference ---
                    emit_status(&app, "processing", None);

                    let text = run_whisper(&whisper_ctx, &recorded);

                    match text {
                        Ok(ref t) if !t.trim().is_empty() => {
                            let trimmed = t.trim();
                            log::info!("Whisper recognized: \"{}\"", trimmed);
                            emit_status(&app, "recognized", Some(trimmed.to_string()));

                            if let Some((decision, _kw)) = CommandParser::parse(trimmed) {
                                log::info!("Command matched: {} (text: \"{}\")", decision, trimmed);
                                on_command(decision, trimmed);
                            }
                        }
                        Ok(_) => {
                            log::debug!("Whisper returned empty text, ignoring");
                        }
                        Err(e) => {
                            log::warn!("Whisper inference failed: {}", e);
                        }
                    }

                    // Reset VAD state
                    recording = false;
                    recorded.clear();
                    silence_count = 0;
                    _speech_start = None;

                    if inner.listening.load(Ordering::SeqCst) {
                        emit_status(&app, "listening", None);
                    }
                }
            }
        }
    }

    // Stream is dropped here, stopping capture
    drop(stream);
    emit_status(&app, "idle", None);
    log::info!("Voice loop stopped");
    Ok(())
}

// ---------------------------------------------------------------------------
// Whisper inference
// ---------------------------------------------------------------------------

fn run_whisper(ctx: &Arc<Mutex<Option<WhisperContext>>>, audio: &[f32]) -> Result<String, String> {
    let guard = ctx.lock().unwrap();
    let whisper = guard
        .as_ref()
        .ok_or_else(|| "Whisper model not loaded".to_string())?;

    let mut state = whisper
        .create_state()
        .map_err(|e| format!("Failed to create whisper state: {}", e))?;

    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_language(Some("auto"));
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_single_segment(true);
    params.set_no_context(true);
    // Suppress non-speech tokens for cleaner output
    params.set_suppress_blank(true);

    state
        .full(params, audio)
        .map_err(|e| format!("Whisper inference error: {}", e))?;

    let n_segments = state.full_n_segments().map_err(|e| format!("{}", e))?;
    let mut result = String::new();
    for i in 0..n_segments {
        if let Ok(seg) = state.full_get_segment_text(i) {
            result.push_str(&seg);
        }
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_parser_allow() {
        assert_eq!(CommandParser::parse("允许").map(|r| r.0), Some("allow"));
        assert_eq!(CommandParser::parse("allow").map(|r| r.0), Some("allow"));
        assert_eq!(CommandParser::parse("yes").map(|r| r.0), Some("allow"));
        assert_eq!(CommandParser::parse("好").map(|r| r.0), Some("allow"));
        assert_eq!(CommandParser::parse("可以").map(|r| r.0), Some("allow"));
    }

    #[test]
    fn test_command_parser_allow_always() {
        assert_eq!(
            CommandParser::parse("总是允许").map(|r| r.0),
            Some("allow-always")
        );
        assert_eq!(
            CommandParser::parse("always").map(|r| r.0),
            Some("allow-always")
        );
        assert_eq!(
            CommandParser::parse("一直").map(|r| r.0),
            Some("allow-always")
        );
        assert_eq!(
            CommandParser::parse("总是").map(|r| r.0),
            Some("allow-always")
        );
    }

    #[test]
    fn test_command_parser_deny() {
        assert_eq!(CommandParser::parse("拒绝").map(|r| r.0), Some("deny"));
        assert_eq!(CommandParser::parse("deny").map(|r| r.0), Some("deny"));
        assert_eq!(CommandParser::parse("no").map(|r| r.0), Some("deny"));
        assert_eq!(CommandParser::parse("不").map(|r| r.0), Some("deny"));
        assert_eq!(CommandParser::parse("不要").map(|r| r.0), Some("deny"));
    }

    #[test]
    fn test_command_parser_no_match() {
        assert!(CommandParser::parse("hello world").is_none());
        assert!(CommandParser::parse("").is_none());
    }

    #[test]
    fn test_command_parser_priority() {
        // "总是允许" should match allow-always, not allow
        assert_eq!(
            CommandParser::parse("总是允许这个操作").map(|r| r.0),
            Some("allow-always")
        );
    }

    #[test]
    fn test_rms() {
        let silence = vec![0.0f32; 100];
        assert_eq!(compute_rms(&silence), 0.0);

        let loud = vec![1.0f32; 100];
        assert!((compute_rms(&loud) - 1.0).abs() < 0.001);

        assert_eq!(compute_rms(&[]), 0.0);
    }
}

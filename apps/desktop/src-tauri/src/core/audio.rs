// SPDX-License-Identifier: AGPL-3.0-or-later
//! Audio sample buffer for voice-capture pipeline.
//!
//! `SampleBuffer` is the in-memory hand-off between the cpal capture callback
//! and the whisper.cpp inference task. Per ADR-010 (voice/STT toolchain),
//! captured audio never touches disk — this buffer zeroes its contents on
//! drop so samples don't linger in freed memory after a transcription run.
//!
//! This module also defines the [`AudioHost`] / [`AudioStream`] traits and
//! [`CaptureSession`] entry point. The traits exist so tests can drive the
//! capture pipeline with a deterministic mock host; the concrete cpal-backed
//! implementation lives in `crate::audio_backend` (outside `core/` because
//! it imports `cpal::*`, which `core/` is forbidden from doing).

use std::sync::{Arc, Mutex};

use zeroize::{Zeroize, ZeroizeOnDrop};

/// Append-only buffer of f32 audio samples that zeroes its storage on drop.
///
/// Used as the hand-off between the cpal capture callback (which calls
/// [`SampleBuffer::push`]) and the whisper.cpp inference task (which calls
/// [`SampleBuffer::drain_to_vec`] once recording stops). The drained `Vec`
/// becomes the caller's responsibility; the buffer's own storage is wiped
/// on `Drop` so samples don't linger in freed allocator memory.
///
/// Production callers must use [`SampleBuffer::with_capacity`] — `push` past
/// the pre-allocated capacity panics, because growing the underlying `Vec`
/// would copy samples to a new allocation and free the old one without
/// zeroing it, leaking plaintext audio. See ADR-010.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct SampleBuffer {
    samples: Vec<f32>,
}

impl SampleBuffer {
    /// Create an empty, zero-capacity buffer.
    ///
    /// **For tests only.** Calling [`SampleBuffer::push`] on a `new()` buffer
    /// always panics because the underlying `Vec` has no capacity. Production
    /// callers must use [`SampleBuffer::with_capacity`].
    pub fn new() -> Self {
        Self {
            samples: Vec::new(),
        }
    }

    /// Construct a buffer pre-allocated for `capacity` samples. Choose `capacity`
    /// large enough for the entire planned recording — `push` past `capacity`
    /// will panic, because growth would leak un-zeroed samples through realloc.
    /// See ADR-010.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            samples: Vec::with_capacity(capacity),
        }
    }

    /// Append samples to the end of the buffer.
    ///
    /// Panics if the resulting length would exceed the buffer's pre-allocated
    /// capacity, because growing the `Vec` would leak plaintext samples
    /// through the freed old allocation. See ADR-010.
    pub fn push(&mut self, samples: &[f32]) {
        assert!(
            self.samples.len() + samples.len() <= self.samples.capacity(),
            "SampleBuffer::push would exceed pre-allocated capacity ({} + {} > {}); \
             grow not allowed because realloc leaks audio. See ADR-010.",
            self.samples.len(),
            samples.len(),
            self.samples.capacity()
        );
        self.samples.extend_from_slice(samples);
    }

    /// Take ownership of all buffered samples, leaving the buffer empty.
    pub fn drain_to_vec(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.samples)
    }

    /// Zero and clear the buffer without returning its contents.
    pub fn clear(&mut self) {
        // `Vec::zeroize` already sets len=0 after wiping; no explicit clear needed.
        self.samples.zeroize();
    }

    /// Number of samples currently buffered.
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Remaining headroom before `push` would panic.
    pub fn remaining_capacity(&self) -> usize {
        self.samples.capacity() - self.samples.len()
    }
}

impl Default for SampleBuffer {
    fn default() -> Self {
        Self::new()
    }
}

// -----------------------------------------------------------------------------
// CaptureSession — abstraction over a real-time audio input stream.
// -----------------------------------------------------------------------------

/// Configuration of the input stream as reported by the host.
///
/// `CaptureSession` records at whatever native config the device prefers and
/// surfaces it here. Downmixing-to-mono and resampling-to-16kHz (whisper's
/// expected input) are the *caller's* job in a follow-up task — keeping the
/// raw config visible avoids a hidden lossy conversion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioInputConfig {
    pub sample_rate: u32,
    pub channels: u16,
}

/// Errors surfaced from audio capture. Hardware-coupling errors (cpal's many
/// `BuildStreamError` variants) collapse into [`AudioError::StreamBuild`] so
/// `core/` can handle them without depending on cpal's types.
#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("no default input device available")]
    NoInputDevice,
    #[error("failed to build input stream: {0}")]
    StreamBuild(String),
    #[error("requested duration would overflow buffer capacity")]
    DurationOverflow,
    #[error("invalid recording duration: {msg}")]
    InvalidDuration { msg: String },
}

/// Callback the audio host invokes with each chunk of interleaved-by-channel
/// f32 samples. Boxed so the host can store it across thread boundaries.
pub type SamplesCallback = Box<dyn FnMut(&[f32]) + Send + 'static>;

/// Abstraction over an audio host (cpal in production, a deterministic fake
/// in tests). Implementors live outside `core/` if they need to import
/// hardware crates.
pub trait AudioHost {
    /// The host's preferred input config — used by [`start_capture`] to size
    /// the pre-allocated buffer.
    fn default_input_config(&self) -> Result<AudioInputConfig, AudioError>;
    /// Begin streaming. The host calls `on_samples` with interleaved-by-channel
    /// f32 chunks. The returned [`AudioStream`] must keep the underlying
    /// stream alive; dropping it stops capture.
    fn start_input(
        &self,
        config: &AudioInputConfig,
        on_samples: SamplesCallback,
    ) -> Result<Box<dyn AudioStream>, AudioError>;
}

/// Opaque handle to a running input stream. Drop stops the stream.
pub trait AudioStream: Send + 'static {}

/// Active recording. Drop = stop without draining; `stop_and_drain` = stop +
/// take samples.
pub struct CaptureSession {
    config: AudioInputConfig,
    samples: Arc<Mutex<SampleBuffer>>,
    stream: Box<dyn AudioStream>,
}

/// Result of [`CaptureSession::stop_and_drain`].
pub struct CaptureResult {
    pub samples: Vec<f32>,
    pub config: AudioInputConfig,
}

/// Start capturing up to `max_seconds` of audio at the host's native config.
///
/// The buffer is pre-allocated for `max_seconds * sample_rate * channels`
/// samples. Overflow past that capacity is dropped silently rather than
/// panicking — callbacks come from a background thread and a panic there is
/// hard to recover from. Callers who care about exact length should pass
/// generous `max_seconds`.
pub fn start_capture(host: &dyn AudioHost, max_seconds: f32) -> Result<CaptureSession, AudioError> {
    let config = host.default_input_config()?;
    if !(max_seconds.is_finite() && max_seconds > 0.0) {
        return Err(AudioError::InvalidDuration {
            msg: format!("max_seconds must be finite and > 0, got {max_seconds}"),
        });
    }
    let max_samples = (max_seconds * config.sample_rate as f32) as usize * config.channels as usize;
    if max_samples == 0 {
        return Err(AudioError::InvalidDuration {
            msg: format!(
                "max_seconds={max_seconds} produces zero samples at sample_rate={} channels={}",
                config.sample_rate, config.channels
            ),
        });
    }
    let samples = Arc::new(Mutex::new(SampleBuffer::with_capacity(max_samples)));
    let buf_for_cb = Arc::clone(&samples);
    let stream = host.start_input(
        &config,
        Box::new(move |chunk: &[f32]| {
            // Best-effort: if the mutex is poisoned we drop samples rather than
            // panic on the callback thread. If the buffer is full we truncate
            // to the remaining capacity — same reason.
            if let Ok(mut buf) = buf_for_cb.lock() {
                let head = chunk.len().min(buf.remaining_capacity());
                if head > 0 {
                    buf.push(&chunk[..head]);
                }
            }
        }),
    )?;
    Ok(CaptureSession {
        config,
        samples,
        stream,
    })
}

impl CaptureSession {
    /// The native input config the host selected for this stream.
    pub fn config(&self) -> &AudioInputConfig {
        &self.config
    }

    /// Stop the stream, drain the buffer, return ownership of the samples.
    pub fn stop_and_drain(self) -> Result<CaptureResult, AudioError> {
        // Drop the stream first so no more callbacks fire while we lock the
        // samples mutex. Dropping a `Box<dyn AudioStream>` is what cpal needs
        // to halt the input thread.
        drop(self.stream);
        let mut buf = self
            .samples
            .lock()
            .map_err(|_| AudioError::StreamBuild("samples mutex poisoned during capture".into()))?;
        let samples = buf.drain_to_vec();
        Ok(CaptureResult {
            samples,
            config: self.config,
        })
    }
}

// -----------------------------------------------------------------------------
// Test-only mock host. Lives in core/ because the tests below need it; gated
// behind #[cfg(test)] so the production binary never sees it.
// -----------------------------------------------------------------------------

#[cfg(test)]
type MockFeed = Box<dyn FnOnce(&mut dyn FnMut(&[f32])) + Send>;

#[cfg(test)]
pub(crate) struct MockAudioHost {
    config: AudioInputConfig,
    feed: Mutex<Option<MockFeed>>,
}

#[cfg(test)]
impl MockAudioHost {
    pub fn new<F>(config: AudioInputConfig, feed: F) -> Self
    where
        F: FnOnce(&mut dyn FnMut(&[f32])) + Send + 'static,
    {
        Self {
            config,
            feed: Mutex::new(Some(Box::new(feed))),
        }
    }
}

#[cfg(test)]
struct MockStream;
#[cfg(test)]
impl AudioStream for MockStream {}

#[cfg(test)]
impl AudioHost for MockAudioHost {
    fn default_input_config(&self) -> Result<AudioInputConfig, AudioError> {
        Ok(self.config.clone())
    }

    fn start_input(
        &self,
        _config: &AudioInputConfig,
        mut on_samples: SamplesCallback,
    ) -> Result<Box<dyn AudioStream>, AudioError> {
        // Run the feed synchronously so the test sees data immediately.
        if let Some(feed) = self.feed.lock().unwrap().take() {
            feed(&mut |chunk| on_samples(chunk));
        }
        Ok(Box::new(MockStream))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_appended_then_drained_match() {
        let mut buf = SampleBuffer::with_capacity(8);
        buf.push(&[0.1, -0.2, 0.3, 0.4, -0.5]);
        let drained = buf.drain_to_vec();
        assert_eq!(drained, vec![0.1, -0.2, 0.3, 0.4, -0.5]);
        assert_eq!(buf.len(), 0, "drain should empty the buffer");
    }

    #[test]
    fn clear_empties_the_buffer() {
        let mut buf = SampleBuffer::with_capacity(8);
        buf.push(&[1.0, 2.0, 3.0]);
        buf.clear();
        assert_eq!(buf.len(), 0);
    }

    #[test]
    fn push_accumulates_across_calls() {
        let mut buf = SampleBuffer::with_capacity(8);
        buf.push(&[0.1, 0.2]);
        buf.push(&[0.3, 0.4, 0.5]);
        assert_eq!(buf.len(), 5);
        assert_eq!(buf.drain_to_vec(), vec![0.1, 0.2, 0.3, 0.4, 0.5]);
    }

    #[test]
    fn sample_buffer_implements_zeroize_on_drop() {
        fn assert_zeroize_on_drop<T: zeroize::ZeroizeOnDrop>() {}
        assert_zeroize_on_drop::<SampleBuffer>();
    }

    #[test]
    fn push_within_capacity_succeeds() {
        let mut buf = SampleBuffer::with_capacity(8);
        buf.push(&[1.0, 2.0, 3.0]);
        buf.push(&[4.0, 5.0]);
        assert_eq!(buf.len(), 5);
        assert_eq!(buf.remaining_capacity(), 3);
    }

    #[test]
    #[should_panic(expected = "exceed pre-allocated capacity")]
    fn push_exceeding_capacity_panics() {
        let mut buf = SampleBuffer::with_capacity(4);
        buf.push(&[1.0, 2.0, 3.0]);
        buf.push(&[4.0, 5.0]); // 3+2=5 > 4 → panic
    }

    // -------------------------------------------------------------------------
    // CaptureSession tests, driven by MockAudioHost.
    // -------------------------------------------------------------------------

    #[test]
    fn capture_session_collects_into_buffer_under_500ms() {
        use std::time::{Duration, Instant};

        // Mock host emits 4 chunks of 2000 samples = 8000 total at 8 kHz mono.
        let host = MockAudioHost::new(
            AudioInputConfig {
                sample_rate: 8000,
                channels: 1,
            },
            |on_samples| {
                for chunk_i in 0..4 {
                    let chunk: Vec<f32> = (0..2000)
                        .map(|i| ((chunk_i * 2000 + i) as f32) * 0.001)
                        .collect();
                    on_samples(&chunk);
                }
            },
        );

        let started = Instant::now();
        let session = start_capture(&host, /* max_seconds */ 2.0).unwrap();
        let result = session.stop_and_drain().unwrap();
        let elapsed = started.elapsed();

        assert_eq!(result.config.sample_rate, 8000);
        assert_eq!(result.config.channels, 1);
        assert_eq!(result.samples.len(), 8000);
        // First chunk: i=0..1999, scaled by 0.001 → samples[0]==0.0, samples[2000]==2.0.
        assert_eq!(result.samples[0], 0.0);
        assert!(
            (result.samples[2000] - 2.0).abs() < 1e-3,
            "samples[2000] should be ~2.0, got {}",
            result.samples[2000]
        );
        assert!(
            elapsed < Duration::from_millis(500),
            "mocked capture should be fast; took {elapsed:?}"
        );
    }

    #[test]
    fn capture_overflow_is_dropped_silently_not_panicked() {
        // 1 second × 1000 Hz × 1 ch = 1000 sample capacity; feed 5000.
        let host = MockAudioHost::new(
            AudioInputConfig {
                sample_rate: 1000,
                channels: 1,
            },
            |on_samples| {
                let big: Vec<f32> = vec![0.5; 5000];
                on_samples(&big);
            },
        );
        let session = start_capture(&host, 1.0).unwrap();
        let result = session.stop_and_drain().unwrap();
        assert_eq!(result.samples.len(), 1000);
    }

    #[test]
    fn capture_session_reports_config() {
        let host = MockAudioHost::new(
            AudioInputConfig {
                sample_rate: 44100,
                channels: 2,
            },
            |_on_samples| { /* no samples */ },
        );
        let session = start_capture(&host, 1.0).unwrap();
        assert_eq!(session.config().sample_rate, 44100);
        assert_eq!(session.config().channels, 2);
        let _ = session.stop_and_drain().unwrap();
    }

    #[test]
    fn capture_zero_duration_is_an_error() {
        let host = MockAudioHost::new(
            AudioInputConfig {
                sample_rate: 16000,
                channels: 1,
            },
            |_on_samples| {},
        );
        let result = start_capture(&host, 0.0);
        assert!(matches!(
            result.err(),
            Some(AudioError::InvalidDuration { .. })
        ));
    }
}

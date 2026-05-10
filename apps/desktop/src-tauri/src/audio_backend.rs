// SPDX-License-Identifier: AGPL-3.0-or-later
//! cpal-backed implementation of the [`crate::core::audio::AudioHost`] trait.
//!
//! Lives outside `core/` because it imports `cpal::*`, which `core/` is
//! forbidden from doing (hard rule, see `core/mod.rs`). The Tauri command
//! layer (M4-T6) will own a [`CpalHost`] in production and substitute a fake
//! [`crate::core::audio::AudioHost`] in tests.
//!
//! # Sample-format handling
//!
//! cpal exposes whatever the device reports (F32 / I16 / U16). We convert
//! everything to f32 in the callback so [`crate::core::audio::CaptureSession`]
//! sees a uniform stream regardless of hardware. Downmix-to-mono and
//! resample-to-16kHz happen in a follow-up task — not here.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};

use crate::core::audio::{AudioError, AudioHost, AudioInputConfig, AudioStream, SamplesCallback};

/// Production audio host wrapping `cpal::default_host()`.
///
/// Constructed by `commands.rs` in M4-T6; until then nothing in the binary
/// references it, so suppress the `dead_code` lint on this scaffolding.
#[allow(dead_code)]
pub struct CpalHost {
    inner: cpal::Host,
}

impl Default for CpalHost {
    fn default() -> Self {
        Self {
            inner: cpal::default_host(),
        }
    }
}

#[allow(dead_code)]
impl CpalHost {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Wraps a live `cpal::Stream`. Drop = stop the stream (cpal's own Drop impl
/// halts the input thread).
#[allow(dead_code)]
struct CpalStream {
    #[allow(dead_code)] // dropped to halt the input thread; never read.
    stream: cpal::Stream,
}

impl AudioStream for CpalStream {}

// SAFETY: cpal::Stream is !Send by default because some platform backends
// (notably Windows ASIO) require the stream stay on its origin thread. Linux
// ALSA's cpal::Stream is observably Send-safe — it's wrapped around a worker
// thread that owns the device handle, and dropping the Stream sends a stop
// signal to that thread. We gate this to Linux so M7 (Android) and any future
// Windows target trip a compile error and force per-platform review.
#[cfg(target_os = "linux")]
unsafe impl Send for CpalStream {}

#[cfg(not(target_os = "linux"))]
compile_error!(
    "audio_backend::CpalStream's Send safety has only been audited for Linux \
     ALSA. Re-evaluate per-platform before extending; see FIXME(M7) in \
     audio_backend.rs."
);

impl AudioHost for CpalHost {
    fn default_input_config(&self) -> Result<AudioInputConfig, AudioError> {
        let device = self
            .inner
            .default_input_device()
            .ok_or(AudioError::NoInputDevice)?;
        let supported = device
            .default_input_config()
            .map_err(|e| AudioError::StreamBuild(e.to_string()))?;
        Ok(AudioInputConfig {
            sample_rate: supported.sample_rate(),
            channels: supported.channels(),
        })
    }

    fn start_input(
        &self,
        config: &AudioInputConfig,
        mut on_samples: SamplesCallback,
    ) -> Result<Box<dyn AudioStream>, AudioError> {
        let device = self
            .inner
            .default_input_device()
            .ok_or(AudioError::NoInputDevice)?;
        let supported = device
            .default_input_config()
            .map_err(|e| AudioError::StreamBuild(e.to_string()))?;
        let stream_config = StreamConfig {
            channels: config.channels,
            sample_rate: config.sample_rate,
            buffer_size: cpal::BufferSize::Default,
        };
        let err_fn = |e| eprintln!("cpal stream error: {e}");
        let stream = match supported.sample_format() {
            SampleFormat::F32 => device
                .build_input_stream(
                    &stream_config,
                    move |data: &[f32], _: &cpal::InputCallbackInfo| on_samples(data),
                    err_fn,
                    None,
                )
                .map_err(|e| AudioError::StreamBuild(e.to_string()))?,
            SampleFormat::I16 => device
                .build_input_stream(
                    &stream_config,
                    move |data: &[i16], _: &cpal::InputCallbackInfo| {
                        let scaled: Vec<f32> =
                            data.iter().map(|s| *s as f32 / i16::MAX as f32).collect();
                        on_samples(&scaled);
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| AudioError::StreamBuild(e.to_string()))?,
            SampleFormat::U16 => device
                .build_input_stream(
                    &stream_config,
                    move |data: &[u16], _: &cpal::InputCallbackInfo| {
                        let scaled: Vec<f32> = data
                            .iter()
                            .map(|s| (*s as f32 - u16::MAX as f32 / 2.0) / (u16::MAX as f32 / 2.0))
                            .collect();
                        on_samples(&scaled);
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| AudioError::StreamBuild(e.to_string()))?,
            other => {
                return Err(AudioError::StreamBuild(format!(
                    "unsupported cpal sample format: {other:?}"
                )))
            }
        };
        stream
            .play()
            .map_err(|e| AudioError::StreamBuild(e.to_string()))?;
        Ok(Box::new(CpalStream { stream }))
    }
}

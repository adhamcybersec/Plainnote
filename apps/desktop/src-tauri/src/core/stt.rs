// SPDX-License-Identifier: AGPL-3.0-or-later
//! Local on-device speech-to-text via [`whisper-rs`] (whisper.cpp Rust binding).
//!
//! Per ADR-010 (voice/STT toolchain), Plainnote does not bundle any whisper
//! model file; the user supplies a `.bin` from the whisper.cpp project (or a
//! compatible source). The default file is `ggml-base.en.bin`; users can
//! point Settings at a larger model for better accuracy on novel proper
//! nouns.
//!
//! Inference runs on CPU. whisper-rs takes `&[f32]` mono samples at 16 kHz;
//! the caller is responsible for downmix and resample (M4-T2 returns
//! native-rate stereo from cpal; the conversion happens before this module
//! is called).
//!
//! `WhisperEngine` is `Send + Sync` so callers can move it between threads
//! (e.g. via `tauri::async_runtime::spawn_blocking`).

use std::path::{Path, PathBuf};

use thiserror::Error;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

#[derive(Debug, Error)]
pub enum SttError {
    #[error("model file not found at {path}")]
    ModelMissing { path: PathBuf },
    #[error("model path is not valid UTF-8: {path:?}")]
    NonUtf8ModelPath { path: PathBuf },
    #[error("failed to initialize whisper context: {0}")]
    WhisperInit(String),
    #[error("failed to create whisper state: {0}")]
    WhisperState(String),
    #[error("failed to run whisper inference: {0}")]
    WhisperRun(String),
    #[error("no audio samples to transcribe")]
    EmptyInput,
}

const APP_NAMESPACE: &str = "plainnote";
const MODELS_SUBDIR: &str = "models";
const DEFAULT_MODEL_FILE: &str = "ggml-base.en.bin";

/// Pure helper: build the models dir given a chosen XDG data dir. Kept
/// separate so tests can verify path composition without touching env vars.
fn default_models_dir_in(data_dir: &Path) -> PathBuf {
    data_dir.join(APP_NAMESPACE).join(MODELS_SUBDIR)
}

/// Directory where Plainnote looks for whisper.cpp model files. Resolved per
/// XDG Base Directory: `$XDG_DATA_HOME/plainnote/models/`, or
/// `~/.local/share/plainnote/models/` if XDG_DATA_HOME is not set.
///
/// On systems where neither resolves (extremely rare), falls back to
/// `./plainnote/models/` relative to the current working directory.
pub fn default_models_dir() -> PathBuf {
    default_models_dir_in(&dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")))
}

/// Default whisper model path: `<default_models_dir()>/ggml-base.en.bin`.
/// Users can override via Settings (M4-T9) to point at a different model.
pub fn default_model_path() -> PathBuf {
    default_models_dir().join(DEFAULT_MODEL_FILE)
}

/// Owns a loaded whisper.cpp model. `Send + Sync` (inherited from
/// `WhisperContext`'s internal `Arc<WhisperInnerContext>`).
///
/// Although safely shareable across threads, do NOT call `transcribe` from
/// multiple threads on a shared `&WhisperEngine` concurrently: each call
/// allocates ~96 MB of decoder state (per-call KV cache + decode buffers)
/// and concurrent calls multiply that footprint linearly. Plainnote's
/// usage is one transcribe per Record-Stop cycle on a single worker
/// thread (M4-T6 wires this via `tauri::async_runtime::spawn_blocking`).
pub struct WhisperEngine {
    ctx: WhisperContext,
}

impl WhisperEngine {
    pub fn from_model_file(path: &Path) -> Result<Self, SttError> {
        if !path.exists() || !path.is_file() {
            return Err(SttError::ModelMissing {
                path: path.to_path_buf(),
            });
        }
        let path_str = path.to_str().ok_or_else(|| SttError::NonUtf8ModelPath {
            path: path.to_path_buf(),
        })?;
        let ctx = WhisperContext::new_with_params(path_str, WhisperContextParameters::default())
            .map_err(|e| SttError::WhisperInit(e.to_string()))?;
        Ok(Self { ctx })
    }

    /// Convenience: load whisper from the XDG default model path.
    /// Returns `SttError::ModelMissing` if the user has not placed a model
    /// file at that location yet (the first-run UX path; see M4-T11).
    pub fn from_default_model() -> Result<Self, SttError> {
        Self::from_model_file(&default_model_path())
    }

    /// Transcribe `samples` (16 kHz mono f32). Returns the concatenated text
    /// of all segments. Empty samples → `SttError::EmptyInput` so callers can
    /// distinguish "no audio recorded" from "transcribed silence successfully."
    /// CPU-heavy: callers must run this on a blocking task pool.
    pub fn transcribe(&self, samples: &[f32]) -> Result<String, SttError> {
        if samples.is_empty() {
            return Err(SttError::EmptyInput);
        }
        let mut state = self
            .ctx
            .create_state()
            .map_err(|e| SttError::WhisperState(e.to_string()))?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some("en"));
        params.set_translate(false);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        state
            .full(params, samples)
            .map_err(|e| SttError::WhisperRun(e.to_string()))?;
        let n = state.full_n_segments();
        let mut transcript = String::new();
        for i in 0..n {
            let segment = state
                .get_segment(i)
                .ok_or_else(|| SttError::WhisperRun(format!("segment {i} missing")))?;
            let text = segment
                .to_str_lossy()
                .map_err(|e| SttError::WhisperRun(e.to_string()))?;
            transcript.push_str(&text);
        }
        Ok(transcript)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_model_path_ends_with_ggml_base_en() {
        let p = default_model_path();
        let last = p.file_name().expect("file name").to_string_lossy();
        assert_eq!(last, "ggml-base.en.bin");
    }

    #[test]
    fn default_models_dir_is_parent_of_default_model_path() {
        let dir = default_models_dir();
        let model = default_model_path();
        assert_eq!(model.parent().unwrap(), dir);
    }

    #[test]
    fn default_model_path_lives_under_plainnote_models() {
        let p = default_model_path();
        let s = p.to_string_lossy();
        assert!(
            s.ends_with("plainnote/models/ggml-base.en.bin"),
            "unexpected default path: {s}"
        );
    }

    #[test]
    fn default_models_dir_in_appends_namespace_and_subdir() {
        use std::path::Path;
        let dir = default_models_dir_in(Path::new("/tmp/xdg"));
        assert_eq!(dir, Path::new("/tmp/xdg/plainnote/models"));
    }

    #[test]
    fn whisper_engine_returns_model_missing_error() {
        use std::path::Path;
        let result = WhisperEngine::from_model_file(Path::new("/nonexistent/model.bin"));
        assert!(matches!(result, Err(SttError::ModelMissing { .. })));
    }

    #[test]
    fn whisper_engine_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<WhisperEngine>();
    }

    #[cfg(feature = "whisper-integration")]
    #[test]
    fn transcribe_rejects_empty_input() {
        let model_path: PathBuf = std::env::var("PLAINNOTE_TEST_MODEL_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                dirs::home_dir()
                    .expect("home dir")
                    .join("Downloads/ggml-base.en.bin")
            });
        if !model_path.exists() {
            eprintln!(
                "skipping transcribe_rejects_empty_input: no model at {}",
                model_path.display()
            );
            return;
        }
        let engine = WhisperEngine::from_model_file(&model_path).expect("model load");
        assert!(matches!(engine.transcribe(&[]), Err(SttError::EmptyInput)));
    }

    #[cfg(feature = "whisper-integration")]
    #[test]
    fn loads_real_model_and_transcribes_silence() {
        let model_path: PathBuf = std::env::var("PLAINNOTE_TEST_MODEL_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                dirs::home_dir()
                    .expect("home dir")
                    .join("Downloads/ggml-base.en.bin")
            });
        if !model_path.exists() {
            eprintln!(
                "skipping whisper-integration test: model not at {}",
                model_path.display()
            );
            return;
        }
        let engine = WhisperEngine::from_model_file(&model_path).expect("model load");
        // 1 second of synthesized silence at 16 kHz mono.
        let silence: Vec<f32> = vec![0.0_f32; 16_000];
        let transcript = engine.transcribe(&silence).expect("transcribe");
        eprintln!("silence transcript: {transcript:?}");
        // Whisper typically returns "[BLANK_AUDIO]" or "" for silence — both
        // acceptable. The point of this test is that the whole pipeline runs
        // without error.
    }
}

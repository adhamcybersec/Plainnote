// SPDX-License-Identifier: AGPL-3.0-or-later
//! Audio sample buffer for voice-capture pipeline.
//!
//! `SampleBuffer` is the in-memory hand-off between the cpal capture callback
//! and the whisper.cpp inference task. Per ADR-010 (voice/STT toolchain),
//! captured audio never touches disk — this buffer zeroes its contents on
//! drop so samples don't linger in freed memory after a transcription run.

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
}

// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Bounded, zero-parse reading of `.nxir` bytes.
//!
//! The app-host maps/receives the payload and reads it in place — no
//! deserialization into owned structures. All reads go through the traversal
//! limits below so a hostile payload cannot make the reader do unbounded work.
//!
//! On-disk form: the canonical **single segment** bytes (no stream framing) —
//! what [`crate::hashing::zeroed_canonical_bytes`]-style canonicalization
//! emits. One segment keeps mapping trivial and the format unambiguous.

use crate::{ui_ir_capnp::ui_program, IrError, SCHEMA_MAJOR};
use alloc::vec;
use alloc::vec::Vec;
use capnp::message::{Reader, ReaderOptions, ReaderSegments};

/// Traversal budget for a whole program message (in words, 8 bytes each).
/// Generous for real programs (a large app is well under 1 MiB) but bounded.
pub const TRAVERSAL_LIMIT_WORDS: usize = 4 * 1024 * 1024 / 8;
/// Nesting budget: the grammar bounds nesting structurally; 128 is far above
/// anything the compiler emits and far below stack danger.
pub const NESTING_LIMIT: i32 = 128;

/// Reader options every `.nxir` consumer must use.
#[must_use]
pub fn reader_options() -> ReaderOptions {
    let mut opts = ReaderOptions::new();
    opts.traversal_limit_in_words(Some(TRAVERSAL_LIMIT_WORDS));
    opts.nesting_limit(NESTING_LIMIT);
    opts
}

/// Owned, 8-byte-aligned `.nxir` byte storage.
///
/// Canonical bytes are capnp words — consumers reading them in place need
/// word alignment (`include_bytes!` embeds solve this with `repr(align(8))`
/// statics; RUNTIME-received payloads land here). Backing storage is a
/// `u64` vector, so alignment holds by construction.
pub struct AlignedBytes {
    words: Vec<u64>,
    len: usize,
}

impl AlignedBytes {
    /// Allocates zeroed aligned storage for `len` payload bytes.
    #[must_use]
    pub fn zeroed(len: usize) -> Self {
        Self { words: vec![0u64; len.div_ceil(8)], len }
    }

    /// The payload bytes (8-byte aligned, exactly `len` long).
    #[must_use]
    #[allow(unsafe_code)]
    pub fn as_bytes(&self) -> &[u8] {
        // SAFETY: the storage is `len.div_ceil(8)` u64 words, so `len` bytes
        // starting at the (8-aligned) base are in bounds and initialized;
        // u64 → u8 reinterpretation is always valid.
        unsafe { core::slice::from_raw_parts(self.words.as_ptr().cast::<u8>(), self.len) }
    }

    /// Mutable payload bytes (for filling from a transport read).
    #[must_use]
    #[allow(unsafe_code)]
    pub fn as_bytes_mut(&mut self) -> &mut [u8] {
        // SAFETY: same bounds/init argument as `as_bytes`; exclusive borrow.
        unsafe {
            core::slice::from_raw_parts_mut(self.words.as_mut_ptr().cast::<u8>(), self.len)
        }
    }
}

/// A single borrowed segment (canonical `.nxir` payload).
pub struct SingleSegment<'a>(pub &'a [u8]);

impl ReaderSegments for SingleSegment<'_> {
    fn get_segment(&self, idx: u32) -> Option<&[u8]> {
        if idx == 0 {
            Some(self.0)
        } else {
            None
        }
    }

    fn len(&self) -> usize {
        1
    }
}

/// A loaded (but not yet validated) program message over borrowed bytes.
pub struct ProgramReader<'a> {
    message: Reader<SingleSegment<'a>>,
}

impl<'a> ProgramReader<'a> {
    /// Wraps a canonical single-segment `.nxir` payload.
    ///
    /// # Errors
    /// [`IrError::Malformed`] if the bytes are not a valid message;
    /// [`IrError::UnsupportedMajor`] on a schema-major mismatch.
    pub fn from_canonical_bytes(bytes: &'a [u8]) -> Result<Self, IrError> {
        if bytes.is_empty() || bytes.len() % 8 != 0 {
            return Err(IrError::Malformed);
        }
        let message = Reader::new(SingleSegment(bytes), reader_options());
        let this = Self { message };
        let root = this.root()?;
        if root.get_schema_version_major() != SCHEMA_MAJOR {
            return Err(IrError::UnsupportedMajor);
        }
        Ok(this)
    }

    /// The typed root. Cheap; call freely.
    ///
    /// # Errors
    /// [`IrError::Malformed`] if the root pointer is invalid.
    pub fn root(&self) -> Result<ui_program::Reader<'_>, IrError> {
        self.message.get_root::<ui_program::Reader<'_>>().map_err(|_| IrError::Malformed)
    }
}

#[cfg(test)]
mod tests {
    use super::AlignedBytes;

    #[test]
    fn aligned_bytes_alignment_and_length() {
        for len in [0usize, 1, 7, 8, 9, 4096, 4097] {
            let mut buf = AlignedBytes::zeroed(len);
            assert_eq!(buf.as_bytes().len(), len);
            assert_eq!(buf.as_bytes_mut().len(), len);
            assert_eq!(buf.as_bytes().as_ptr() as usize % 8, 0, "len={len}");
            if len > 0 {
                buf.as_bytes_mut()[len - 1] = 0xAB;
                assert_eq!(buf.as_bytes()[len - 1], 0xAB);
            }
        }
    }
}

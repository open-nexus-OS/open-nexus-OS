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

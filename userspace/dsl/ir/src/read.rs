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

/// Traversal budget for ONE operation's reads of a program (in words, 8 bytes
/// each). Generous for real programs (a large app is well under 1 MiB) but
/// bounded — per operation, not per app: capnp's budget belongs to a reader
/// and only counts down, so an app keeps a [`Program`] and reads it through a
/// fresh [`ProgramReader`] for every emit, dispatch step or lookup. One reader
/// kept for an app's life ran dry after 28 menu toggles in settings and
/// answered every later read `Malformed` (TASK-0344).
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
        unsafe { core::slice::from_raw_parts_mut(self.words.as_mut_ptr().cast::<u8>(), self.len) }
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

/// A program's checked canonical bytes — what a holder keeps for as long as it
/// runs the program (an app: its whole life).
///
/// It reads nothing itself: each operation asks [`Program::reader`] for a
/// reader of its own, whose traversal budget bounds that operation's work and
/// ends with it. The bytes are borrowed, never copied.
#[derive(Clone, Copy)]
pub struct Program<'a> {
    bytes: &'a [u8],
}

impl<'a> Program<'a> {
    /// Checks a canonical single-segment `.nxir` payload once — word-sized, a
    /// readable root, the schema major — and keeps it.
    ///
    /// # Errors
    /// As [`ProgramReader::from_canonical_bytes`].
    pub fn from_canonical_bytes(bytes: &'a [u8]) -> Result<Self, IrError> {
        ProgramReader::from_canonical_bytes(bytes)?;
        Ok(Self { bytes })
    }

    /// A reader for ONE operation, with the whole [`TRAVERSAL_LIMIT_WORDS`]
    /// budget. Cheap: it wraps the same bytes; nothing is copied or allocated.
    #[must_use]
    pub fn reader(&self) -> ProgramReader<'a> {
        ProgramReader::over(self.bytes)
    }
}

/// A program message being read by ONE operation, over borrowed bytes.
///
/// Its traversal budget ([`TRAVERSAL_LIMIT_WORDS`]) is spent by every read
/// through it and never refilled — so it is made for an operation and dropped
/// with it. A holder that outlives operations keeps a [`Program`] instead.
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
        // `%` not `is_multiple_of`: the OS toolchain (nightly-2025-01-15) predates
        // the `unsigned_is_multiple_of` stabilization (stable 1.87).
        #[allow(unknown_lints, clippy::manual_is_multiple_of)]
        if bytes.is_empty() || bytes.len() % 8 != 0 {
            return Err(IrError::Malformed);
        }
        let this = Self::over(bytes);
        let root = this.root()?;
        if root.get_schema_version_major() != SCHEMA_MAJOR {
            return Err(IrError::UnsupportedMajor);
        }
        Ok(this)
    }

    /// A reader over bytes already checked, with a whole budget.
    fn over(bytes: &'a [u8]) -> Self {
        Self { message: Reader::new(SingleSegment(bytes), reader_options()) }
    }

    /// The typed root. Cheap; call freely — every read through it spends this
    /// reader's budget.
    ///
    /// # Errors
    /// [`IrError::Malformed`] if the root pointer is invalid or this reader's
    /// budget is spent.
    pub fn root(&self) -> Result<ui_program::Reader<'_>, IrError> {
        self.message.get_root::<ui_program::Reader<'_>>().map_err(|_| IrError::Malformed)
    }
}

#[cfg(test)]
mod tests {
    use super::{AlignedBytes, Program, TRAVERSAL_LIMIT_WORDS};
    use crate::ui_ir_capnp::ui_program;

    /// Three quarters of a budget: one read of it fits a reader, two do not.
    const BLOB: usize = TRAVERSAL_LIMIT_WORDS * 8 * 3 / 4;

    /// A program whose `sourceDigest` is `len` bytes — reading that blob spends
    /// `len / 8` words of a reader's budget: an operation of a known size.
    fn program_with_blob(len: usize) -> AlignedBytes {
        let words = u32::try_from(len / 8 + 64).expect("a test-sized segment");
        let mut message = capnp::message::Builder::new(
            capnp::message::HeapAllocator::new().first_segment_words(words),
        );
        let mut root = message.init_root::<ui_program::Builder<'_>>();
        root.set_schema_version_major(crate::SCHEMA_MAJOR);
        root.set_source_digest(&alloc::vec![0xAB; len]);
        let segments = message.get_segments_for_output();
        assert_eq!(segments.len(), 1, "one segment, as the canonical form is");
        let mut bytes = AlignedBytes::zeroed(segments[0].len());
        bytes.as_bytes_mut().copy_from_slice(segments[0]);
        bytes
    }

    /// Every operation reads with its own budget: ten operations, each reading
    /// three quarters of one. A budget shared over the program's life runs dry
    /// at the second — an app's did after 28 menu toggles (TASK-0344).
    #[test]
    fn every_operation_reads_the_program_with_its_own_budget() {
        let bytes = program_with_blob(BLOB);
        let program = Program::from_canonical_bytes(bytes.as_bytes()).expect("a program");
        for op in 0..10 {
            let reader = program.reader();
            let root = reader.root().expect("root");
            let digest = root.get_source_digest().map(<[u8]>::len);
            assert_eq!(digest.ok(), Some(BLOB), "operation {op} read the whole blob");
        }
    }

    /// The bound still holds — per operation: one operation reading past its
    /// budget is refused, however fresh the program's other readers are.
    #[test]
    fn test_reject_one_operation_past_its_traversal_budget() {
        let bytes = program_with_blob(BLOB);
        let program = Program::from_canonical_bytes(bytes.as_bytes()).expect("a program");
        let reader = program.reader();
        let root = reader.root().expect("root");
        assert!(root.get_source_digest().is_ok(), "the first read fits the budget");
        assert!(
            root.get_source_digest().is_err(),
            "a second read in the SAME operation exceeds its budget and is refused"
        );
        assert!(program.reader().root().is_ok(), "the next operation reads with a whole budget");
    }

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

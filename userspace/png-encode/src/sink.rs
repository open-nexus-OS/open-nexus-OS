// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: where an encoded PNG goes. The encoder hands the sink whole units, in order:
//! the signature with IHDR, then each IDAT chunk (at most `IDAT_PAYLOAD_MAX` payload bytes)
//! in its own call, then IEND. A sink error stops the encode and comes back as
//! `EncodeError::Sink`.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: tests/encode/stream.rs (write boundaries, failing sinks)

use alloc::collections::TryReserveError;
use alloc::vec::Vec;

/// A byte destination for the encoder (a file, an IPC stream, a buffer).
pub trait Sink {
    /// Why a write failed.
    type Error;

    /// Takes the next bytes of the PNG, all or nothing.
    ///
    /// # Errors
    /// Whatever the destination reports; the encode stops there.
    fn write(&mut self, bytes: &[u8]) -> Result<(), Self::Error>;
}

impl<S: Sink + ?Sized> Sink for &mut S {
    type Error = S::Error;

    fn write(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        (**self).write(bytes)
    }
}

/// Collects the PNG in memory (host tools, goldens). Growing reallocates: on a heap that
/// never frees, reserve the expected size first or write to a fixed destination instead.
impl Sink for Vec<u8> {
    type Error = TryReserveError;

    fn write(&mut self, bytes: &[u8]) -> Result<(), TryReserveError> {
        self.try_reserve(bytes.len())?;
        self.extend_from_slice(bytes);
        Ok(())
    }
}

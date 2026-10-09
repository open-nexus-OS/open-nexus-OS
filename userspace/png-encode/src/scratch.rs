// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the only place png-encode allocates. Buffers are reserved with
//! `try_reserve_exact` (a heap that cannot serve them yields `EncodeError::OutOfMemory`, not
//! an abort), zeroed, and boxed at their final size: a boxed slice or array cannot grow, so
//! nothing reallocates after `Encoder::new`.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: through every Encoder test (all scratch comes from here)

use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::EncodeError;

/// `len` zeroed elements, exactly.
pub(crate) fn zeroed_slice<T: Copy + Default>(len: usize) -> Result<Box<[T]>, EncodeError> {
    let mut buf = Vec::new();
    buf.try_reserve_exact(len).map_err(|_| EncodeError::OutOfMemory)?;
    buf.resize(len, T::default());
    Ok(buf.into_boxed_slice())
}

/// `N` zeroed elements as a fixed-size array, built on the heap (no stack temporary).
pub(crate) fn zeroed_array<T: Copy + Default, const N: usize>() -> Result<Box<[T; N]>, EncodeError>
{
    zeroed_slice::<T>(N)?.try_into().map_err(|_| EncodeError::OutOfMemory)
}

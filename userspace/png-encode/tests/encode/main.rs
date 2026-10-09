// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: integration tests of png-encode's public API, as one test binary: round trips
//! through the independent `png` decoder (CRC-32 and Adler-32 verified), streaming chunk
//! boundaries and encoder reuse, and one test_reject_* per bound.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 27 tests (roundtrip 10, stream 7, reject 10)
//!
//! TEST_SCOPE:
//!   - pixel-exact round trips: flat, gradients, noise, UI-like, odd widths, padded strides
//!   - compression ratios of a flat and a UI-like 1280x800 frame
//!   - sink writes = signature+IHDR, whole IDAT chunks (16 KiB payloads), IEND
//!   - reuse with and without `reset`; sink failures propagate and leave the encoder usable
//!   - rejects of every bound before the first byte is written
//!
//! DEPENDENCIES:
//!   - `png` (dev-dependency): the independent decoder

mod reject;
mod roundtrip;
mod stream;
mod support;

// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: UDP-specific validation helpers for netstackd facade operations
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Planned in netstackd host seam tests
//! ADR: docs/adr/0005-dsoftbus-architecture.md

#[must_use]
#[inline]
pub(crate) fn recv_max_bounded(max: usize) -> usize {
    core::cmp::min(max, 460)
}

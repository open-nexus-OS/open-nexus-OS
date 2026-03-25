// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: ICMP ping helper logic for netstackd facade wiring
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Planned in netstackd host seam tests
//! ADR: docs/adr/0005-dsoftbus-architecture.md

#[must_use]
#[inline]
pub(crate) fn cap_rtt_ms(rtt_ms: u64) -> u16 {
    core::cmp::min(rtt_ms, 65535) as u16
}

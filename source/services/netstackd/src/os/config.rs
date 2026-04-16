// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Netstackd internal runtime constants and configuration helpers
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Planned in netstackd host seam tests
//! ADR: docs/adr/0005-dsoftbus-architecture.md

pub(crate) const LOOPBACK_PORT: u16 = 34_567;
pub(crate) const LOOPBACK_PORT_B: u16 = 34_568;
pub(crate) const LOOPBACK_UDP_PORT: u16 = 37_020;
pub(crate) const LOOPBACK_UDP_QUIC_CLIENT_PORT: u16 = 34_569;
pub(crate) const TCP_READY_SPIN_BUDGET: u32 = 16;
pub(crate) const TCP_READY_STEP_MS: u64 = 2;

#[inline]
pub(crate) fn fallback_ipv4_config(
    is_qemu_smoke: bool,
    mac: [u8; 6],
) -> ([u8; 4], u8, Option<[u8; 4]>) {
    crate::os::entry_pure::fallback_ipv4_config(is_qemu_smoke, mac)
}

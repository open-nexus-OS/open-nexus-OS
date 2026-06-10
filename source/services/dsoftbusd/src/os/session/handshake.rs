// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Test-only Noise-XK handshake key derivation helper for deterministic dsoftbusd bring-up.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests
//! ADR: docs/adr/0005-dsoftbus-architecture.md

/// SECURITY: bring-up test keys, NOT production custody.
pub(crate) fn derive_test_secret(tag: u8, port: u16) -> [u8; 32] {
    let mut seed = [0u8; 32];
    seed[0] = tag;
    seed[1] = (port >> 8) as u8;
    seed[2] = (port & 0xff) as u8;
    for (i, byte) in seed.iter_mut().enumerate().skip(3) {
        *byte = ((tag as u16).wrapping_mul(port).wrapping_add(i as u16) & 0xff) as u8;
    }
    seed
}
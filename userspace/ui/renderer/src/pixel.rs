// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: BGRA8888 pixel value type and byte-order helpers.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 3 renderer integration tests, 24 ui_host_snap contract tests
//! ADR: docs/rfcs/RFC-0046-ui-v1a-host-cpu-renderer-snapshots-contract.md

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelBgra {
    pub b: u8,
    pub g: u8,
    pub r: u8,
    pub a: u8,
}

impl PixelBgra {
    #[must_use]
    pub const fn new(b: u8, g: u8, r: u8, a: u8) -> Self {
        Self { b, g, r, a }
    }

    #[must_use]
    pub const fn from_rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { b, g, r, a }
    }

    #[must_use]
    pub const fn bytes(self) -> [u8; 4] {
        [self.b, self.g, self.r, self.a]
    }
}

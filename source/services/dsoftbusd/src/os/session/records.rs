// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Fixed-size encrypted record contract constants for the cross-VM gateway (AEAD tag, request/response plain/ciphertext sizes).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: No tests
//! ADR: docs/adr/0005-dsoftbus-architecture.md
//! Fixed-size encrypted record contract for cross-VM gateway.

pub(crate) const TAGLEN: usize = 16;
pub(crate) const MAX_REQ: usize = 256;
pub(crate) const MAX_RSP: usize = 512;
pub(crate) const REQ_PLAIN: usize = 1 + 2 + MAX_REQ;
pub(crate) const RSP_PLAIN: usize = 1 + 2 + MAX_RSP;
pub(crate) const REQ_CIPH: usize = REQ_PLAIN + TAGLEN;
pub(crate) const RSP_CIPH: usize = RSP_PLAIN + TAGLEN;

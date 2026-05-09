// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Low-level framebuffer and `ramfb` helpers for `fbdevd`.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `fbdevd` host tests plus visible-bootstrap QEMU proofs.
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

pub mod framebuffer;
pub mod ramfb;

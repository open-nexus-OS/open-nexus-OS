// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: CpuMockBackend contract tests (golden reference, deterministic output).
//! OWNERS: @ui @runtime
//! STATUS: Experimental
//! TEST_SCOPE:
//!   - CpuMockBackend submit with valid/invalid commands
//!   - Resource creation and lifecycle
//!   - Framebuffer output validation (golden comparison)
//!   - Deterministic output for same input
//!
//! ADR: docs/adr/0031-three-layer-animation-architecture.md

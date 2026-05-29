// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Resource contract tests for nexus-gfx (Buffer, Image, Sampler).
//! OWNERS: @ui @runtime
//! STATUS: Experimental
//! TEST_COVERAGE: Contract tests for resource creation, bounds, and zero-copy VMO backing.
//!
//! TEST_SCOPE:
//!   - Buffer creation with valid/invalid sizes
//!   - Buffer write/read roundtrip
//!   - Resource exhaustion at bounds
//!   - VMO-backed Buffer import/export (zero-copy)
//!
//! ADR: docs/adr/0031-three-layer-animation-architecture.md

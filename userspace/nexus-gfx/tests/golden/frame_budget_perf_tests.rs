// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Performance tests — frame budget, render timing, jank detection.
//! OWNERS: @ui @runtime
//! STATUS: Placeholder
//! TEST_SCOPE:
//!   - Frame budget enforcement (8.3ms @ 120Hz target)
//!   - Render timing per phase (animate, layout, paint, present)
//!   - Jank detection (frame exceeds budget)
//!   - Regression gates (no frame > 2× budget in CI)
//!
//! ADR: docs/adr/0031-three-layer-animation-architecture.md

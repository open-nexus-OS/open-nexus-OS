// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Render encoder contract tests (draw calls, fragment bytes, tile validation).
//! OWNERS: @ui @runtime
//! STATUS: Experimental
//! TEST_SCOPE:
//!   - Fragment bytes set/get
//!   - Tile draw with valid/invalid rects
//!   - Encoder active state (reject after end_encoding)
//!
//! ADR: docs/adr/0031-three-layer-animation-architecture.md

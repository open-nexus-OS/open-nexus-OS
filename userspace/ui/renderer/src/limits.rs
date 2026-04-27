// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Bounds and format constants for the TASK-0054 host renderer.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 3 renderer integration tests, 24 ui_host_snap contract tests
//! ADR: docs/rfcs/RFC-0046-ui-v1a-host-cpu-renderer-snapshots-contract.md

pub const BYTES_PER_PIXEL: u32 = 4;
pub const STRIDE_ALIGNMENT: u32 = 64;
pub const MAX_FRAME_WIDTH: u32 = 4096;
pub const MAX_FRAME_HEIGHT: u32 = 4096;
pub const MAX_FRAME_PIXELS: u64 = 16_777_216;
pub const MAX_IMAGE_WIDTH: u32 = 4096;
pub const MAX_IMAGE_HEIGHT: u32 = 4096;
pub const MAX_IMAGE_PIXELS: u64 = 16_777_216;
pub const MAX_GLYPHS: usize = 1024;
pub const MAX_DAMAGE_RECTS: u16 = 64;

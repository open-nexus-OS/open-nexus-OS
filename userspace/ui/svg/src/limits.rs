// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

/// Maximum number of SVG elements (nodes) allowed in a document.
pub const MAX_SVG_NODES: usize = 4096;

/// Maximum number of path segments across all paths.
pub const MAX_PATH_SEGMENTS: usize = 16384;

/// Maximum width or height of an SVG document in pixels.
pub const MAX_SVG_DIMENSION: f32 = 2048.0;

/// Bytes per pixel in BGRA8888 output.
pub const OUTPUT_BYTES_PER_PIXEL: usize = 4;

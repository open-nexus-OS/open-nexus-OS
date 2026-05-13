// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

/// Maximum image dimension (width or height) in pixels.
pub const MAX_IMAGE_DIMENSION: u32 = 8192;

/// Maximum total pixels for a decoded image (width × height).
/// 8192 × 8192 = 67,108,864 pixels ≈ 256 MiB at RGBA8.
pub const MAX_DECODE_PIXELS: u64 = 67_108_864;

/// Decompression ratio limit: if compressed size × this factor < output pixels,
/// treat as a potential decompression bomb.
pub const DECOMPRESSION_BOMB_RATIO: u64 = 100;

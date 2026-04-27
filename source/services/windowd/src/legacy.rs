// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use alloc::vec::Vec;

/// Legacy deterministic frame helper retained only until old compositor
/// scaffold tests are migrated away from checksum behavior.
pub fn render_frame(width: usize, height: usize) -> Vec<u32> {
    let len = width.saturating_mul(height);
    let mut buffer = Vec::with_capacity(len);
    for y in 0..height {
        for x in 0..width {
            let pixel = ((x as u32) << 16) ^ (y as u32);
            buffer.push(pixel);
        }
    }
    buffer
}

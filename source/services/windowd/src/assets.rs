// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

/// Embedded cursor SVGs for the BreezeX cursor theme.
/// These are compiled into the OS image at build time.

/// BreezeX left_ptr cursor (arrow). Minimal SVG subset.
/// 32x32, hotspot at (5,5).
pub const CURSOR_LEFT_PTR_SVG: &str = r##"<svg width="32" height="32" xmlns="http://www.w3.org/2000/svg">
    <path d="M 5,5 L 25,18 L 18,20 L 22,28 L 17,30 L 13,22 Z" fill="#ffffff" stroke="#000000" stroke-width="1.5" />
</svg>"##;

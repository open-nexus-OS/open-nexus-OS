// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

/// Embedded cursor SVGs for the Mocu cursor theme (CC0).
/// These are compiled into the OS image at build time.

/// Mocu-style left_ptr cursor. Uses filled paths because the minimal SVG
/// rasterizer does not yet implement production-quality stroke rendering.
/// 48x48, hotspot at (4,3).
pub const CURSOR_LEFT_PTR_SVG: &str = r##"<svg width="48" height="48" xmlns="http://www.w3.org/2000/svg">
    <path d="M 4,3 L 42,27 L 30,30 L 38,44 L 29,47 L 21,33 L 11,41 Z" fill="#ffffff" />
    <path d="M 10,11 L 33,25 L 24,27 L 31,40 L 28,41 L 20,27 L 14,34 Z" fill="#1a1a2e" />
</svg>"##;

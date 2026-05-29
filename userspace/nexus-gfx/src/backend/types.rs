// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Backend-specific types (Rect, ResourceId).
//! OWNERS: @ui @runtime
//! STATUS: Experimental
//! API_STABILITY: Stable

/// Axis-aligned rectangle for damage regions and transfer areas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Opaque GPU resource identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResourceId(pub u32);

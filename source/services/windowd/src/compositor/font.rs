// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Compositor re-export of the shared 5×7 bitmap font. The glyph table
//! now lives at the crate root (`crate::bitmap_font`) so the always-compiled
//! `scene_graph` text primitive can share it with the OS-only compositor.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable

pub(crate) use crate::bitmap_font::bitmap_font_5x7;

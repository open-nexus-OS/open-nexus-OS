// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Runtime text rendering — since the RFC-0067-P5 promotion this is
//! a thin client of the shared text SSOT (`nexus-text-baked`): the baked A8
//! atlases, measurement and the row-based glyph blender moved there VERBATIM
//! so windowd, the app-host runtime, and future DSL shells draw text
//! identically. This module keeps the crate-internal names stable.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests live in nexus-text-baked (promoted with the code)

pub(crate) use nexus_text_baked::{advance, line_height, FontSize};

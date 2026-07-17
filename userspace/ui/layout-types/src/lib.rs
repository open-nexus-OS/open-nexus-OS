// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![no_std]
#![deny(unsafe_code)]
#![allow(clippy::too_many_arguments)]

//! CONTEXT: Layout type system for TASK-0058 / RFC-0057.
//! OWNERS: @ui
//! STATUS: Done
//! API_STABILITY: Unstable
//! TEST_COVERAGE: tests/ui_v3a_host/ + nexus-layout engine_tests
//! ADR: docs/adr/0030-layout-engine-deterministic-pretext.md
//!
//! no_std + alloc / RFC-0057.
//!
//! ADR: docs/rfcs/RFC-0057-ui-v3a-layout-engine-pretext-contract.md

extern crate alloc;

pub mod border;
pub mod color;
pub mod direction;
pub mod measure;
pub mod node;
pub mod text;
pub mod types;

pub use border::{Border, CornerRadius, EdgeBorder, PathPoint, PathShape, ShapeKind, VisualStyle};
pub use border::{BoxShadow, ShadowLevel, TextShadow};
pub use border::{GlassLevel, SurfaceMaterial};
pub use color::Rgba8;
pub use direction::{Align, Direction, Justify, Overflow, Position, ScrollAxis, ZIndex};
pub use measure::{LineLayout, LineMetrics, MeasureText, PreparedTextHandle};
pub use node::{
    FlexItem, Fraction, Grid, LayoutNode, Spacer, Stack, TextContent, TextInputNode, TextNode,
};
pub use text::{FontWeight, LineHeight, TextAlign, TextStyle, WhiteSpace};
pub use types::{EdgeInsets, FxPx, Rect};

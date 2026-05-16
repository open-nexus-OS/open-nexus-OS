// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![no_std]
#![deny(unsafe_code)]
#![allow(clippy::too_many_arguments)]

//! Layout type system for TASK-0058 / RFC-0057.
//! `no_std` + `alloc`. Consumers use layout types without pulling in algorithms.

#[macro_use]
extern crate alloc;

pub mod border;
pub mod color;
pub mod direction;
pub mod measure;
pub mod node;
pub mod text;
pub mod types;

pub use border::{Border, CornerRadius, EdgeBorder, VisualStyle};
pub use color::Rgba8;
pub use direction::{Align, Direction, Justify, Overflow, Position, ZIndex};
pub use measure::{LineLayout, LineMetrics, MeasureText, PreparedTextHandle};
pub use node::{FlexItem, Fraction, Grid, LayoutNode, Spacer, Stack, TextContent, TextNode};
pub use text::{FontWeight, LineHeight, TextAlign, TextStyle, WhiteSpace};
pub use types::{EdgeInsets, FxPx, Rect};

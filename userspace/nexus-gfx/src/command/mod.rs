// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Command encoding and validation (CommandBuffer, Encoders, Passes).
//! OWNERS: @ui @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable

pub mod blit_encoder;
pub mod buffer;
#[cfg(test)]
mod buffer_wire_tests;
pub mod compute_encoder;
pub mod layer;
/// The `CompositeLayer` record on the scene wire: its source and its slot (RFC-0114 §6).
pub mod layer_wire;
pub mod pass;
pub mod render_encoder;
/// The `LayerTransform` record on the scene wire (RFC-0101 §8).
pub(crate) mod transform_wire;
pub mod validation;

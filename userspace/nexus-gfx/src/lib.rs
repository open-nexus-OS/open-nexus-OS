// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: NexusGfx SDK v1 minimal: Metal-like graphics API vocabulary.
//! OWNERS: @ui @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! RFC: docs/rfcs/RFC-0059-ui-v5a-animation-nexusgfx-sdk-gpu-driver-contract.md

#![cfg_attr(target_os = "none", no_std)]

extern crate alloc;

pub mod buffer;
pub mod command_buffer;
pub mod device;
pub mod error;
pub mod fence;
pub mod queue;
pub mod render_encoder;
pub mod types;

pub use buffer::Buffer;
pub use command_buffer::{CommandBuffer, CommittedBuffer};
pub use device::Device;
pub use error::GfxError;
pub use fence::Fence;
pub use queue::Queue;
pub use render_encoder::RenderCommandEncoder;
pub use types::{BufferUsage, PixelFormat, RenderPassDesc, ResourceId, TileRect};

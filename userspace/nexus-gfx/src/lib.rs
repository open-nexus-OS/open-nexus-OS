// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: NexusGfx SDK v1 minimal: Metal-like graphics API vocabulary.
//! OWNERS: @ui @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! RFC: docs/rfcs/RFC-0059-ui-v5a-animation-nexusgfx-sdk-gpu-driver-contract.md

extern crate alloc;

pub mod device;
pub mod queue;
pub mod command_buffer;
pub mod render_encoder;
pub mod buffer;
pub mod fence;
pub mod types;
pub mod error;

pub use device::Device;
pub use queue::Queue;
pub use command_buffer::{CommandBuffer, CommittedBuffer};
pub use render_encoder::RenderCommandEncoder;
pub use buffer::Buffer;
pub use fence::Fence;
pub use types::{BufferUsage, PixelFormat, RenderPassDesc, ResourceId, TileRect};
pub use error::GfxError;

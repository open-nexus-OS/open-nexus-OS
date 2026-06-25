// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![allow(clippy::expect_used, clippy::needless_lifetimes, clippy::too_many_arguments)]

//! CONTEXT: NexusGfx SDK — explicit, command-based graphics API for Open Nexus OS.
//! OWNERS: @ui @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! RFC: docs/rfcs/RFC-0059-ui-v5a-animation-nexusgfx-sdk-gpu-driver-contract.md
//!
//! Module structure (scales to ~120KLoC):
//!   core/       — Device, Queue, Fence, Error, Types (fundament)
//!   resource/   — Buffer, Image, Sampler, Heap, Descriptor
//!   command/    — CommandBuffer, Render/Compute/Blit encoders, Pass, Validation
//!   pipeline/   — RenderPipeline, ComputePipeline, VertexDescriptor, PipelineCache
//!   shader/     — ShaderModule, ShaderFunction, ShaderLibrary, Reflection
//!   backend/    — GfxBackend trait, CpuMockBackend (re-exports from gfx-backend)
//!   sync/       — TimelineSemaphore, Event, Barrier
//!   transfer/   — VMO import/export, DMA sharing, Layout transitions
//!   perf/       — Performance counters, GPU trace, Frame budget
//!   cache/      — Texture atlas, Render target pool, Descriptor set cache

#![cfg_attr(target_os = "none", no_std)]

extern crate alloc;

// ── Core ──────────────────────────────────────────────────────────
pub mod core;

// ── Resource ──────────────────────────────────────────────────────
pub mod resource;

// ── Command ───────────────────────────────────────────────────────
pub mod command;

// ── Pipeline ──────────────────────────────────────────────────────
pub mod pipeline;

// ── Shader ────────────────────────────────────────────────────────
pub mod shader;

// ── Backend ───────────────────────────────────────────────────────
pub mod backend;

// ── Sync ──────────────────────────────────────────────────────────
pub mod sync;

// ── Transfer ──────────────────────────────────────────────────────
pub mod transfer;

// ── Perf ──────────────────────────────────────────────────────────
pub mod perf;

// ── Cache ─────────────────────────────────────────────────────────
pub mod cache;

// ── Backward-compatible re-exports (v0 API surface) ───────────────
pub use core::device::Device;
pub use core::error::GfxError;
pub use core::fence::Fence;
pub use core::queue::Queue;
pub use core::types::{BufferUsage, PixelFormat, RenderPassDesc, ResourceId, TileRect};

pub use command::buffer::{CommandBuffer, CommittedBuffer, RgbaColor};
pub use command::layer::{BackdropCache, Layer, LayerBackdrop, LayerShadow};
pub use command::render_encoder::RenderCommandEncoder;

pub use perf::timer::PipelineTimer;
pub use resource::buffer::Buffer;

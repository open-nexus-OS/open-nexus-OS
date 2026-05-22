// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Types: PixelFormat, BufferUsage, ResourceId, TileRect, RenderPassDesc.

use alloc::vec::Vec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat { Bgra8888, Rgba8888 }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BufferUsage { pub render_target: bool, pub shader_read: bool }

impl Default for BufferUsage { fn default() -> Self { Self { render_target: true, shader_read: false } } }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResourceId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileRect { pub x: u32, pub y: u32, pub width: u32, pub height: u32 }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderPassDesc { pub color_attachments: Vec<ResourceId>, pub width: u32, pub height: u32 }

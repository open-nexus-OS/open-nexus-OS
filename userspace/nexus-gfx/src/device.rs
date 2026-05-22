// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::buffer::Buffer;
use crate::command_buffer::CommandBuffer;
use crate::queue::Queue;
use crate::types::BufferUsage;

/// Capability-gated device handle. Factory for Queue, Buffer, CommandBuffer.
#[derive(Debug, Clone)]
pub struct Device;

impl Device {
    pub fn new() -> Self { Device }

    pub fn new_queue(&self) -> Queue { Queue }

    pub fn new_buffer(&self, size: usize, usage: BufferUsage) -> Buffer {
        let data = if usage.render_target { vec![0u8; size] } else { Vec::with_capacity(size) };
        Buffer { data, usage }
    }

    pub fn new_command_buffer(&self) -> CommandBuffer { CommandBuffer::new() }
}

impl Default for Device { fn default() -> Self { Self::new() } }

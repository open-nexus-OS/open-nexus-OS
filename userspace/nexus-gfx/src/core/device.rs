// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::command::buffer::CommandBuffer;
use crate::core::queue::Queue;
use crate::core::types::BufferUsage;
use crate::resource::buffer::Buffer;
use alloc::{vec, vec::Vec};

/// Capability-gated device handle. Factory for Queue, Buffer, CommandBuffer.
#[derive(Debug, Clone)]
pub struct Device;

impl Device {
    pub fn new() -> Self {
        Device
    }

    pub fn new_queue(&self) -> Queue {
        Queue::new()
    }

    pub fn new_buffer(&self, size: usize, usage: BufferUsage) -> Buffer {
        let data = if usage.render_target {
            vec![0u8; size]
        } else {
            Vec::with_capacity(size)
        };
        Buffer { data, usage }
    }

    pub fn new_command_buffer(&self) -> CommandBuffer {
        CommandBuffer::new()
    }
}

impl Default for Device {
    fn default() -> Self {
        Self::new()
    }
}

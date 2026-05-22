// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::command_buffer::CommandBuffer;
use crate::fence::Fence;

/// Single submission queue. submit() consumes the CommandBuffer.
#[derive(Debug, Clone, Copy)]
pub struct Queue;

impl Queue {
    pub fn submit(&self, _cmd: CommandBuffer) -> Fence {
        // In v1, the CPU mock backend processes commands synchronously in submit.
        // The real GPU driver will enqueue and return a fence for async completion.
        Fence::new_signaled()
    }
}

// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use alloc::vec::Vec;
use crate::render_encoder::RenderCommandEncoder;
use crate::types::{RenderPassDesc, TileRect};

/// Internal command representation — not exposed to users.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    SetFragmentBytes { offset: usize, data: Vec<u8> },
    DrawTiles { tiles: Vec<TileRect> },
}

/// A recorded command buffer. Use begin_render_pass() to start a render pass.
#[derive(Debug, Clone)]
pub struct CommandBuffer {
    pub(crate) commands: Vec<Command>,
}

impl CommandBuffer {
    pub fn new() -> Self { Self { commands: Vec::new() } }

    /// Begin a render pass. Returns a RenderCommandEncoder for recording.
    /// The encoder borrows the CommandBuffer mutably.
    pub fn begin_render_pass(&mut self, _desc: RenderPassDesc) -> RenderCommandEncoder<'_> {
        RenderCommandEncoder::new(self)
    }

    /// Seal the command buffer. No more commands can be recorded.
    /// Returns a CommittedBuffer ready for submission.
    pub fn commit(self) -> CommittedBuffer {
        CommittedBuffer { commands: self.commands }
    }
}

impl Default for CommandBuffer {
    fn default() -> Self { Self::new() }
}

/// A sealed command buffer — immutable, ready for backend submission.
#[derive(Debug, Clone, PartialEq)]
pub struct CommittedBuffer {
    pub(crate) commands: Vec<Command>,
}

impl CommittedBuffer {
    /// Access the commands for backend inspection/execution.
    pub fn commands(&self) -> &[Command] { &self.commands }
    pub fn command_count(&self) -> usize { self.commands.len() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committed_buffer_is_sealed() {
        let mut cmd = CommandBuffer::new();
        {
            let mut enc = cmd.begin_render_pass(RenderPassDesc { color_attachments: vec![], width: 64, height: 64 });
            enc.set_fragment_bytes(0, &[1, 2, 3, 4]);
            enc.draw_tiles(&[TileRect { x: 0, y: 0, width: 10, height: 10 }]);
            enc.end_encoding();
        }
        let committed = cmd.commit();
        assert_eq!(committed.command_count(), 2);
    }

    #[test]
    fn command_buffer_deterministic() {
        let mut a = CommandBuffer::new();
        let mut b = CommandBuffer::new();
        {
            let mut ea = a.begin_render_pass(RenderPassDesc { color_attachments: vec![], width: 64, height: 64 });
            let mut eb = b.begin_render_pass(RenderPassDesc { color_attachments: vec![], width: 64, height: 64 });
            ea.set_fragment_bytes(0, &[1, 2, 3]);
            eb.set_fragment_bytes(0, &[1, 2, 3]);
            ea.end_encoding();
            eb.end_encoding();
        }
        assert_eq!(a.commit(), b.commit());
    }
}

// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::command_buffer::{Command, CommandBuffer};
use crate::types::TileRect;

/// Records rendering commands into a CommandBuffer.
/// Created by CommandBuffer::begin_render_pass().
pub struct RenderCommandEncoder<'a> {
    cmd: &'a mut CommandBuffer,
    active: bool,
}

impl<'a> RenderCommandEncoder<'a> {
    pub(crate) fn new(cmd: &'a mut CommandBuffer) -> Self {
        Self { cmd, active: true }
    }

    /// Set fragment shader uniform data at a byte offset.
    /// The data is copied into the command buffer.
    pub fn set_fragment_bytes(&mut self, offset: usize, data: &[u8]) {
        assert!(self.active, "encoder already ended");
        self.cmd.commands.push(Command::SetFragmentBytes { offset, data: data.to_vec() });
    }

    /// Queue a draw of the given tiles.
    pub fn draw_tiles(&mut self, tiles: &[TileRect]) {
        assert!(self.active, "encoder already ended");
        self.cmd.commands.push(Command::DrawTiles { tiles: tiles.to_vec() });
    }

    /// End encoding. The encoder is consumed.
    pub fn end_encoding(self) {
        // drop impl marks inactive
        let _ = self;
    }
}

impl<'a> Drop for RenderCommandEncoder<'a> {
    fn drop(&mut self) { self.active = false; }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::RenderPassDesc;

    #[test]
    fn encoder_records_commands() {
        let mut cmd = CommandBuffer::new();
        {
            let mut enc = cmd.begin_render_pass(RenderPassDesc { color_attachments: vec![], width: 64, height: 64 });
            enc.set_fragment_bytes(0, &[42]);
            enc.draw_tiles(&[TileRect { x: 0, y: 0, width: 10, height: 10 }]);
            enc.end_encoding();
        }
        let c = cmd.commit();
        assert_eq!(c.command_count(), 2);
    }

    #[test]
    fn end_encoding_consumes_encoder() {
        let mut cmd = CommandBuffer::new();
        let enc = cmd.begin_render_pass(RenderPassDesc { color_attachments: vec![], width: 64, height: 64 });
        enc.end_encoding();
        // enc is consumed — cannot be used after. Type system enforces this.
    }
}

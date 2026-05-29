// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::command::buffer::{Command, CommandBuffer};
use crate::core::types::TileRect;
use crate::core::error::GfxError;

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
        self.try_set_fragment_bytes(offset, data)
            .expect("invalid fragment bytes");
    }

    pub fn try_set_fragment_bytes(&mut self, offset: usize, data: &[u8]) -> Result<(), GfxError> {
        if !self.active {
            return Err(GfxError::CommandRejected);
        }
        self.cmd.push_command(Command::SetFragmentBytes {
            offset,
            data: data.to_vec(),
        })
    }

    /// Queue a draw of the given tiles.
    pub fn draw_tiles(&mut self, tiles: &[TileRect]) {
        self.try_draw_tiles(tiles).expect("invalid tile draw");
    }

    pub fn try_draw_tiles(&mut self, tiles: &[TileRect]) -> Result<(), GfxError> {
        if !self.active {
            return Err(GfxError::CommandRejected);
        }
        self.cmd.push_command(Command::DrawTiles {
            tiles: tiles.to_vec(),
        })
    }

    /// End encoding. The encoder is consumed.
    pub fn end_encoding(self) {
        // drop impl marks inactive
        let _ = self;
    }
}

impl<'a> Drop for RenderCommandEncoder<'a> {
    fn drop(&mut self) {
        self.active = false;
    }
}

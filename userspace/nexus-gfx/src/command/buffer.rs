// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::command::render_encoder::RenderCommandEncoder;
use crate::core::error::GfxError;
use crate::core::types::{RenderPassDesc, TileRect};
use alloc::vec::Vec;

pub const MAX_COMMANDS: usize = 1024;
pub const MAX_FRAGMENT_BYTES: usize = 4096;
pub const MAX_TILE_RECTS: usize = 1024;
pub const MAX_RENDER_EXTENT: u32 = 8192;

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
    render_extent: Option<(u32, u32)>,
}

impl CommandBuffer {
    pub fn new() -> Self {
        Self { commands: Vec::new(), render_extent: None }
    }

    /// Begin a render pass. Returns a RenderCommandEncoder for recording.
    /// The encoder borrows the CommandBuffer mutably.
    pub fn begin_render_pass(&mut self, desc: RenderPassDesc) -> RenderCommandEncoder<'_> {
        self.try_begin_render_pass(desc).expect("invalid render pass")
    }

    pub fn try_begin_render_pass(
        &mut self,
        desc: RenderPassDesc,
    ) -> Result<RenderCommandEncoder<'_>, GfxError> {
        validate_render_pass(&desc)?;
        self.render_extent = Some((desc.width, desc.height));
        Ok(RenderCommandEncoder::new(self))
    }

    /// Seal the command buffer. No more commands can be recorded.
    /// Returns a CommittedBuffer ready for submission.
    pub fn commit(self) -> CommittedBuffer {
        self.try_commit().expect("invalid command buffer")
    }

    pub fn try_commit(self) -> Result<CommittedBuffer, GfxError> {
        validate_commands(&self.commands, self.render_extent)?;
        Ok(CommittedBuffer { commands: self.commands })
    }

    pub(crate) fn push_command(&mut self, command: Command) -> Result<(), GfxError> {
        if self.commands.len() >= MAX_COMMANDS {
            return Err(GfxError::ResourceExhausted);
        }
        validate_command(&command, self.render_extent)?;
        self.commands.push(command);
        Ok(())
    }
}

impl Default for CommandBuffer {
    fn default() -> Self {
        Self::new()
    }
}

/// A sealed command buffer — immutable, ready for backend submission.
#[derive(Debug, Clone, PartialEq)]
pub struct CommittedBuffer {
    pub(crate) commands: Vec<Command>,
}

impl CommittedBuffer {
    /// Access the commands for backend inspection/execution.
    pub fn commands(&self) -> &[Command] {
        &self.commands
    }
    pub fn command_count(&self) -> usize {
        self.commands.len()
    }

    pub fn validate(&self) -> Result<(), GfxError> {
        validate_commands(&self.commands, None)
    }
}

fn validate_render_pass(desc: &RenderPassDesc) -> Result<(), GfxError> {
    if desc.width == 0
        || desc.height == 0
        || desc.width > MAX_RENDER_EXTENT
        || desc.height > MAX_RENDER_EXTENT
    {
        return Err(GfxError::InvalidArgument);
    }
    Ok(())
}

fn validate_commands(
    commands: &[Command],
    render_extent: Option<(u32, u32)>,
) -> Result<(), GfxError> {
    if commands.len() > MAX_COMMANDS {
        return Err(GfxError::ResourceExhausted);
    }
    for command in commands {
        validate_command(command, render_extent)?;
    }
    Ok(())
}

fn validate_command(command: &Command, render_extent: Option<(u32, u32)>) -> Result<(), GfxError> {
    match command {
        Command::SetFragmentBytes { offset, data } => {
            let end = offset.checked_add(data.len()).ok_or(GfxError::InvalidArgument)?;
            if data.len() > MAX_FRAGMENT_BYTES || end > MAX_FRAGMENT_BYTES {
                return Err(GfxError::ResourceExhausted);
            }
        }
        Command::DrawTiles { tiles } => {
            if tiles.is_empty() || tiles.len() > MAX_TILE_RECTS {
                return Err(GfxError::InvalidArgument);
            }
            for tile in tiles {
                validate_tile(*tile, render_extent)?;
            }
        }
    }
    Ok(())
}

fn validate_tile(tile: TileRect, render_extent: Option<(u32, u32)>) -> Result<(), GfxError> {
    if tile.width == 0 || tile.height == 0 {
        return Err(GfxError::InvalidArgument);
    }
    let end_x = tile.x.checked_add(tile.width).ok_or(GfxError::InvalidArgument)?;
    let end_y = tile.y.checked_add(tile.height).ok_or(GfxError::InvalidArgument)?;
    if let Some((width, height)) = render_extent {
        if end_x > width || end_y > height {
            return Err(GfxError::InvalidArgument);
        }
    }
    Ok(())
}

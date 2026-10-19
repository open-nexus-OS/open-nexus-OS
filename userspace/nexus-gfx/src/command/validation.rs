// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Command validation — deterministic, bounded: a render pass's extent, a buffer's
//! command count, each command's sizes and rectangles (inside the render extent when one is
//! known). Every encode and every decode of a present runs through here before a backend sees a
//! command.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `buffer_wire_tests.rs`, the encoder tests

use super::buffer::{Command, MAX_COMMANDS, MAX_FRAGMENT_BYTES, MAX_RENDER_EXTENT, MAX_TILE_RECTS};
use crate::core::error::GfxError;
use crate::core::types::{RenderPassDesc, TileRect};

pub(super) fn validate_render_pass(desc: &RenderPassDesc) -> Result<(), GfxError> {
    if desc.width == 0
        || desc.height == 0
        || desc.width > MAX_RENDER_EXTENT
        || desc.height > MAX_RENDER_EXTENT
    {
        return Err(GfxError::InvalidArgument);
    }
    Ok(())
}

pub(super) fn validate_commands(
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

pub(super) fn validate_command(
    command: &Command,
    render_extent: Option<(u32, u32)>,
) -> Result<(), GfxError> {
    match command {
        Command::SetFragmentBytes { offset, data } => {
            let end = offset.checked_add(data.len()).ok_or(GfxError::InvalidArgument)?;
            if data.len() > MAX_FRAGMENT_BYTES || end > MAX_FRAGMENT_BYTES {
                return Err(GfxError::ResourceExhausted);
            }
            Ok(())
        }
        Command::DrawTiles { tiles, .. } => validate_tile_list(tiles, render_extent),
        Command::BlitSurface { width, height, .. } => {
            if *width == 0 || *height == 0 {
                return Err(GfxError::InvalidArgument);
            }
            Ok(())
        }
        Command::FillSdfRoundedRect { rect, .. }
        | Command::BlurBackdrop { rect, .. }
        | Command::FillSdfGradient { rect, .. } => validate_tile(*rect, render_extent),
        Command::DropShadow { rect, blur, .. } => {
            // The shape rect must be in-extent; the blur halo may exceed it
            // (the executor clamps). Bound blur so the halo stays sane.
            if *blur > 64 {
                return Err(GfxError::InvalidArgument);
            }
            validate_tile(*rect, render_extent)
        }
        Command::CompositeLayer {
            width,
            height,
            opacity,
            shadow_blur,
            shadow_alpha,
            backdrop_blur,
            ..
        } => {
            if *width == 0 || *height == 0 {
                return Err(GfxError::InvalidArgument);
            }
            if *opacity > 255 || *shadow_alpha > 255 || *shadow_blur > 64 || *backdrop_blur > 64 {
                return Err(GfxError::InvalidArgument);
            }
            Ok(())
        }
        Command::BlendCursor { width, height, .. } => {
            if *width == 0 || *height == 0 {
                return Err(GfxError::InvalidArgument);
            }
            Ok(())
        }
        Command::BlitAbsolute { width, height, .. } => {
            if *width == 0 || *height == 0 {
                return Err(GfxError::InvalidArgument);
            }
            Ok(())
        }
        // A rectangle of the screen: never empty, never wrapping; inside the render pass when
        // its extent is known (the display clamps it to its plane besides).
        Command::Damage { rect, .. } => validate_tile(*rect, render_extent),
    }
}

fn validate_tile_list(
    tiles: &[TileRect],
    render_extent: Option<(u32, u32)>,
) -> Result<(), GfxError> {
    if tiles.is_empty() || tiles.len() > MAX_TILE_RECTS {
        return Err(GfxError::InvalidArgument);
    }
    for tile in tiles {
        validate_tile(*tile, render_extent)?;
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

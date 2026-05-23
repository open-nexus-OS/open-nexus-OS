// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::types::{Rect, ResourceId};
use nexus_gfx::command_buffer::CommittedBuffer;
use nexus_gfx::fence::Fence;
use nexus_gfx::types::PixelFormat;

pub trait GfxBackend {
    fn submit(&mut self, cmd: CommittedBuffer) -> Result<Fence, crate::error::GfxError>;
    fn create_resource(
        &mut self,
        w: u32,
        h: u32,
        fmt: PixelFormat,
    ) -> Result<ResourceId, crate::error::GfxError>;
    fn transfer_to_host(
        &mut self,
        res: ResourceId,
        rect: Rect,
    ) -> Result<(), crate::error::GfxError>;
    fn set_scanout(&mut self, res: ResourceId) -> Result<(), crate::error::GfxError>;
    fn move_cursor(&mut self, x: i32, y: i32) -> Result<(), crate::error::GfxError>;
}

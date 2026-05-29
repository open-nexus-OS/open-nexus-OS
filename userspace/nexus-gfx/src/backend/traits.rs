// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: GfxBackend trait — the interface all GPU backends must implement.
//! OWNERS: @ui @runtime
//! STATUS: Experimental
//! API_STABILITY: Stable

use crate::backend::error::GfxError;
use crate::backend::types::{Rect, ResourceId};
use crate::command::buffer::CommittedBuffer;
use crate::core::fence::Fence;
use crate::core::types::PixelFormat;

pub trait GfxBackend {
    fn submit(&mut self, cmd: CommittedBuffer) -> Result<Fence, GfxError>;
    fn create_resource(
        &mut self,
        w: u32,
        h: u32,
        fmt: PixelFormat,
    ) -> Result<ResourceId, GfxError>;
    fn transfer_to_host(
        &mut self,
        res: ResourceId,
        rect: Rect,
    ) -> Result<(), GfxError>;
    fn set_scanout(&mut self, res: ResourceId) -> Result<(), GfxError>;
    fn move_cursor(&mut self, x: i32, y: i32) -> Result<(), GfxError>;
}

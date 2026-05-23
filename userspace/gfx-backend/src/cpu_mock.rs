// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::error::GfxError;
use crate::traits::GfxBackend;
use crate::types::{Rect, ResourceId};
use alloc::{vec, vec::Vec};
use nexus_gfx::command_buffer::{Command, CommittedBuffer};
use nexus_gfx::fence::Fence;
use nexus_gfx::types::PixelFormat;

struct CpuResource {
    width: u32,
    height: u32,
    format: PixelFormat,
    data: Vec<u8>,
}

pub struct CpuMockBackend {
    framebuffer: Vec<u8>,
    width: u32,
    height: u32,
    resources: Vec<CpuResource>,
    next_id: u32,
}

impl CpuMockBackend {
    pub fn new(w: u32, h: u32) -> Self {
        Self {
            framebuffer: vec![0u8; w as usize * h as usize * 4],
            width: w,
            height: h,
            resources: vec![],
            next_id: 1,
        }
    }
    pub fn framebuffer(&self) -> &[u8] {
        &self.framebuffer
    }

    fn execute(&mut self, cmds: &[Command]) -> Result<(), GfxError> {
        for cmd in cmds {
            match cmd {
                Command::SetFragmentBytes { offset, data } => {
                    if let Some(r) = self.resources.first_mut() {
                        let end = offset.saturating_add(data.len());
                        if end > r.data.len() || matches!(r.format, PixelFormat::Rgba8888) {
                            return Err(GfxError::CommandRejected);
                        }
                        r.data[*offset..end].copy_from_slice(data);
                    }
                }
                Command::DrawTiles { tiles } => {
                    let white: [u8; 4] = [0xff; 4];
                    let fw = self.width as usize;
                    for t in tiles {
                        for y in t.y..(t.y + t.height).min(self.height) {
                            for x in t.x..(t.x + t.width).min(self.width) {
                                let i = (y as usize * fw + x as usize) * 4;
                                if i + 4 <= self.framebuffer.len() {
                                    self.framebuffer[i..i + 4].copy_from_slice(&white);
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

impl GfxBackend for CpuMockBackend {
    fn submit(&mut self, cmd: CommittedBuffer) -> Result<Fence, GfxError> {
        cmd.validate()?;
        self.execute(cmd.commands())?;
        Ok(Fence::new_signaled())
    }
    fn create_resource(
        &mut self,
        w: u32,
        h: u32,
        fmt: PixelFormat,
    ) -> Result<ResourceId, GfxError> {
        if w == 0 || h == 0 {
            return Err(GfxError::InvalidArgument);
        }
        let id = ResourceId(self.next_id);
        self.next_id += 1;
        self.resources.push(CpuResource {
            width: w,
            height: h,
            format: fmt,
            data: vec![0u8; w as usize * h as usize * 4],
        });
        Ok(id)
    }
    fn transfer_to_host(&mut self, r: ResourceId, rect: Rect) -> Result<(), GfxError> {
        let Some(resource) = self.resources.get((r.0.saturating_sub(1)) as usize) else {
            return Err(GfxError::InvalidArgument);
        };
        let end_x = rect
            .x
            .checked_add(rect.width)
            .ok_or(GfxError::InvalidArgument)?;
        let end_y = rect
            .y
            .checked_add(rect.height)
            .ok_or(GfxError::InvalidArgument)?;
        if rect.width == 0 || rect.height == 0 || end_x > resource.width || end_y > resource.height
        {
            return Err(GfxError::InvalidArgument);
        }
        Ok(())
    }
    fn set_scanout(&mut self, r: ResourceId) -> Result<(), GfxError> {
        if self
            .resources
            .get((r.0.saturating_sub(1)) as usize)
            .is_none()
        {
            return Err(GfxError::InvalidArgument);
        }
        Ok(())
    }
    fn move_cursor(&mut self, _x: i32, _y: i32) -> Result<(), GfxError> {
        Ok(())
    }
}

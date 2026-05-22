// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use alloc::vec::Vec;
use nexus_gfx::command_buffer::{Command, CommittedBuffer};
use nexus_gfx::fence::Fence;
use nexus_gfx::types::PixelFormat;
use crate::error::GfxError;
use crate::traits::GfxBackend;
use crate::types::{Rect, ResourceId};

struct CpuResource { width: u32, height: u32, format: PixelFormat, data: Vec<u8> }

pub struct CpuMockBackend {
    framebuffer: Vec<u8>, width: u32, height: u32,
    resources: Vec<CpuResource>, next_id: u32,
}

impl CpuMockBackend {
    pub fn new(w: u32, h: u32) -> Self {
        Self { framebuffer: vec![0u8; w as usize * h as usize * 4], width: w, height: h, resources: vec![], next_id: 1 }
    }
    pub fn framebuffer(&self) -> &[u8] { &self.framebuffer }

    fn execute(&mut self, cmds: &[Command]) {
        for cmd in cmds {
            match cmd {
                Command::SetFragmentBytes { offset, data } => {
                    if let Some(r) = self.resources.first_mut() {
                        let end = offset.saturating_add(data.len());
                        if end > r.data.len() { r.data.resize(end, 0); }
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
                                if i + 4 <= self.framebuffer.len() { self.framebuffer[i..i + 4].copy_from_slice(&white); }
                            }
                        }
                    }
                }
            }
        }
    }
}

impl GfxBackend for CpuMockBackend {
    fn submit(&mut self, cmd: CommittedBuffer) -> Result<Fence, GfxError> { self.execute(cmd.commands()); Ok(Fence::new_signaled()) }
    fn create_resource(&mut self, w: u32, h: u32, fmt: PixelFormat) -> Result<ResourceId, GfxError> {
        if w == 0 || h == 0 { return Err(GfxError::InvalidArgument); }
        let id = ResourceId(self.next_id); self.next_id += 1;
        self.resources.push(CpuResource { width: w, height: h, format: fmt, data: vec![0u8; w as usize * h as usize * 4] });
        Ok(id)
    }
    fn transfer_to_host(&mut self, _r: ResourceId, _rect: Rect) -> Result<(), GfxError> { Ok(()) }
    fn set_scanout(&mut self, _r: ResourceId) -> Result<(), GfxError> { Ok(()) }
    fn move_cursor(&mut self, _x: i32, _y: i32) -> Result<(), GfxError> { Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn create_resource_rejects_zero() {
        let mut b = CpuMockBackend::new(64, 64);
        assert!(b.create_resource(0, 64, PixelFormat::Bgra8888).is_err());
    }
    #[test]
    fn create_resource_succeeds() {
        let mut b = CpuMockBackend::new(64, 64);
        assert_eq!(b.create_resource(32, 32, PixelFormat::Bgra8888).unwrap().0, 1);
    }
    #[test]
    fn submit_empty_succeeds() {
        let mut b = CpuMockBackend::new(64, 64);
        use nexus_gfx::CommandBuffer;
        let empty = CommandBuffer::new().commit();
        assert!(b.submit(empty).unwrap().signaled());
    }
    #[test]
    fn draw_tiles_modifies_framebuffer() {
        let mut b = CpuMockBackend::new(64, 64);
        use nexus_gfx::CommandBuffer;
        use nexus_gfx::RenderPassDesc;
        use nexus_gfx::TileRect;
        let mut cmd = CommandBuffer::new();
        {
            let mut enc = cmd.begin_render_pass(RenderPassDesc { color_attachments: vec![], width: 64, height: 64 });
            enc.draw_tiles(&[TileRect { x: 0, y: 0, width: 10, height: 10 }]);
            enc.end_encoding();
        }
        b.submit(cmd.commit()).unwrap();
        // Framebuffer should have white pixels in top-left 10x10
        assert_eq!(b.framebuffer()[0], 0xff);
        assert_eq!(b.framebuffer()[1], 0xff);
        assert_eq!(b.framebuffer()[2], 0xff);
        assert_eq!(b.framebuffer()[3], 0xff);
    }
}

// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The QEMU path's pipelines (TASK-0215 5c-2 P6, ADR-0072 §4–6): the compositor's WGSL
//! programs, compiled by our toolchain into the host renderer's shader text and carried as a
//! pipeline library (`resources/gpu/pipelines/compositor-virgl.nxgp`) — parsed and checked
//! before a byte reaches the device, created in the virgl context once, bound per draw: the
//! quad's vertex program, the pipeline's fragment program, its blend, its push constants as the
//! fragment stage's constant buffer, its texture through the one bilinear sampler. Every draw of
//! the virgl path is [`VirtioGpuBackend::emit_draw`]; gpud writes no shader text of its own.
//! OWNERS: @gpu @ui
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: the probe's readback proofs (`gpud: virgl draw ok`, `gpud: virgl gradient ok`,
//!   `gpud: layer composite ok`) and the visible QEMU lanes; the library's text is golden-tested
//!   by `tools/nxgpu/tests/tgsi.rs`.

#![cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]

use nexus_gfx::backend::error::GfxError;
use nexus_gfx::backend::gpu::quads::Quad;
use nexus_gfx::shader::ir::builder::Blend;
use nexus_gfx::shader::ir::{Resource, Stage};
use nexus_gfx::shader::{library, tgsi};

use crate::backend::VirtioGpuBackend;
use crate::protocol::{
    VirtioGpuCtxAttachResource, VirtioGpuResourceCreate3d, VirtioGpuSubmit3d,
    VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE, VIRTIO_GPU_CMD_RESOURCE_CREATE_3D,
    VIRTIO_GPU_CMD_SUBMIT_3D,
};
use crate::virgl::{
    Submit3d, PIPE_BIND_VERTEX_BUFFER, PIPE_BUFFER, PIPE_FORMAT_R32G32B32A32_FLOAT,
    PIPE_FORMAT_R8_UNORM, PIPE_PRIM_TRIANGLES, PIPE_SHADER_FRAGMENT, PIPE_SHADER_VERTEX,
    VIRGL_OBJECT_BLEND, VIRGL_OBJECT_DSA, VIRGL_OBJECT_RASTERIZER, VIRGL_OBJECT_VERTEX_ELEMENTS,
};

/// The library (`nxgpu library resources/gpu/pipelines/compositor-virgl.toml`).
const LIBRARY: &[u8] = include_bytes!("../../../../resources/gpu/pipelines/compositor-virgl.nxgp");

// Object handles — the context has one object table, so a handle is unique across every
// subsystem of gpud (the surfaces, sampler views and resources keep their own modules' ranges).
/// Blending off: the library's `replace`.
const H_BLEND_REPLACE: u32 = 0x20;
const H_DSA: u32 = 0x21;
const H_RASTERIZER: u32 = 0x22;
/// One vec4 position attribute, stride 16: the quad's corners.
const H_VERTEX_ELEMENTS: u32 = 0x23;
/// Premultiplied "over": the library's `premultiplied`.
const H_BLEND_PREMULTIPLIED: u32 = 0x62;
/// The one sampler: bilinear, clamped to the edge.
const H_SAMPLER: u32 = 0x34;
/// A pipeline's program: this plus its library id.
const SHADER_BASE: u32 = 0x100;
/// The largest library id a program handle takes.
const MAX_ID: u32 = 0xFF;
/// The quad's buffer (resource id): two triangles over clip space −1..1.
const QUAD_RES: u32 = 0xFA;
/// The most shader text one `SUBMIT_3D` carries: a control command is 4 KiB with its 32-byte
/// header, a chunk's command six dwords more than its text; whole dwords.
const CHUNK: usize = 4000;
/// The most blur radii the table holds.
const MAX_BLURS: usize = 8;

/// A fragment pipeline as the device holds it.
#[derive(Clone, Copy, Default)]
pub(crate) struct Pipeline {
    /// Its program's shader handle; 0 until created.
    shader: u32,
    /// Its blend object.
    blend: u32,
    /// The push constants its program reads, in dwords.
    push: u8,
    /// Whether it samples a texture (at slot 0).
    samples: bool,
}

/// The blur's two passes at one radius (`blur.wgsl`).
#[derive(Clone, Copy, Default)]
pub(crate) struct BlurPair {
    pub(crate) radius: u32,
    pub(crate) horizontal: Pipeline,
    pub(crate) vertical: Pipeline,
}

/// The pipelines gpud draws with, resolved by their library names when they are created.
#[derive(Default)]
pub(crate) struct Pipelines {
    pub(crate) ready: bool,
    /// The quad's buffer and the fixed objects exist (a retry after a failed program creates
    /// them once: a resource id is created once per context).
    fixed: bool,
    /// The vertex program every draw runs.
    quad: u32,
    /// The wallpaper; the splash's pulse over it; a client's surface (a rectangle of a texture,
    /// straight alpha, opacity, corners); a premultiplied sprite; a layer's shadow; the pointer
    /// before its sprite.
    pub(crate) layer_opaque: Pipeline,
    pub(crate) layer_tinted: Pipeline,
    pub(crate) layer_atlas: Pipeline,
    pub(crate) layer_premultiplied: Pipeline,
    pub(crate) shadow: Pipeline,
    pub(crate) gradient: Pipeline,
    /// The blur pairs the library holds.
    blurs: [BlurPair; MAX_BLURS],
    blur_count: usize,
}

impl Pipelines {
    /// The blur pair nearest `radius` (windowd's glass asks 8, 20, 30 or 40 — the library's).
    pub(crate) fn blur(&self, radius: u32) -> Option<BlurPair> {
        self.blurs[..self.blur_count].iter().copied().min_by_key(|b| b.radius.abs_diff(radius))
    }

    /// Where the pipeline named `name` goes; `None` for one gpud does not draw with.
    fn slot(&mut self, name: &str) -> Option<&mut Pipeline> {
        let blur = |prefix: &str| name.strip_prefix(prefix).and_then(|r| r.parse::<u32>().ok());
        let (radius, vertical) = match (blur("blur-horizontal-"), blur("blur-vertical-")) {
            (Some(r), _) => (r, false),
            (_, Some(r)) => (r, true),
            _ => {
                return match name {
                    "layer-opaque" => Some(&mut self.layer_opaque),
                    "layer-tinted" => Some(&mut self.layer_tinted),
                    "layer-atlas" => Some(&mut self.layer_atlas),
                    "layer-premultiplied" => Some(&mut self.layer_premultiplied),
                    "shadow" => Some(&mut self.shadow),
                    "gradient" => Some(&mut self.gradient),
                    _ => None,
                }
            }
        };
        let at = match self.blurs[..self.blur_count].iter().position(|b| b.radius == radius) {
            Some(at) => at,
            None if self.blur_count < MAX_BLURS => {
                self.blurs[self.blur_count].radius = radius;
                self.blur_count += 1;
                self.blur_count - 1
            }
            None => return None,
        };
        let pair = &mut self.blurs[at];
        Some(if vertical { &mut pair.vertical } else { &mut pair.horizontal })
    }

    /// Whether every pipeline gpud draws with is there.
    fn complete(&self) -> bool {
        let fragment = [
            self.layer_opaque,
            self.layer_tinted,
            self.layer_atlas,
            self.layer_premultiplied,
            self.shadow,
            self.gradient,
        ];
        self.quad != 0
            && fragment.iter().all(|p| p.shader != 0)
            && self.blur_count > 0
            && self.blurs[..self.blur_count]
                .iter()
                .all(|b| b.horizontal.shader != 0 && b.vertical.shader != 0)
    }
}

impl VirtioGpuBackend {
    /// Create the library's pipelines in the virgl context — its programs (each checked, a long
    /// one in chunks), the blends they name, the fixed-function state, the sampler and the quad —
    /// and resolve the ones gpud draws with by name. Idempotent; synchronous outside a batch, in
    /// ring order inside one. A library refused in part, or missing a pipeline gpud draws with, is
    /// refused whole (the renderer drops a program it cannot parse without a word, so the probe's
    /// draws prove the programs).
    pub(crate) fn virgl_pipelines_init(&mut self) -> Result<(), GfxError> {
        if self.pipelines.ready {
            return Ok(());
        }
        let lib =
            library::parse(LIBRARY, tgsi::CODE_TARGET, 0).map_err(|_| GfxError::InvalidArgument)?;
        if !lib.refused.is_empty() {
            return Err(GfxError::InvalidArgument);
        }
        if !self.pipelines.fixed {
            self.create_fixed_state()?;
            self.pipelines.fixed = true;
        }
        let mut table = Pipelines { fixed: true, ..Pipelines::default() };
        for e in &lib.accepted {
            if e.id > MAX_ID {
                return Err(GfxError::InvalidArgument);
            }
            let text = tgsi::check_text(e.program, e.interface.stage)
                .map_err(|_| GfxError::InvalidArgument)?;
            let handle = SHADER_BASE + e.id;
            if e.interface.stage == Stage::Vertex {
                if e.name == "quad" {
                    self.create_shader(handle, PIPE_SHADER_VERTEX, text.as_bytes())?;
                    table.quad = handle;
                }
                continue;
            }
            let blend = match tgsi::State::from_bytes(e.state).map(|s| s.blend) {
                Ok(Blend::Replace) => H_BLEND_REPLACE,
                Ok(Blend::OverPremultiplied) => H_BLEND_PREMULTIPLIED,
                _ => return Err(GfxError::Unsupported),
            };
            let pipeline = Pipeline {
                shader: handle,
                blend,
                push: (e.interface.push_bytes / 4).min(16) as u8,
                samples: e
                    .interface
                    .resources
                    .iter()
                    .any(|r| matches!(r, Resource::Texture { .. })),
            };
            let Some(slot) = table.slot(e.name) else { continue };
            self.create_shader(handle, PIPE_SHADER_FRAGMENT, text.as_bytes())?;
            *slot = pipeline;
        }
        if !table.complete() {
            return Err(GfxError::InvalidArgument);
        }
        table.ready = true;
        self.pipelines = table;
        Ok(())
    }

    /// The quad's buffer and the objects every draw binds: the blends the library names, depth
    /// and stencil off, a solid rasterizer, the quad's one attribute, the sampler.
    fn create_fixed_state(&mut self) -> Result<(), GfxError> {
        let create = VirtioGpuResourceCreate3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_RESOURCE_CREATE_3D),
            resource_id: QUAD_RES,
            target: PIPE_BUFFER,
            format: PIPE_FORMAT_R8_UNORM,
            bind: PIPE_BIND_VERTEX_BUFFER,
            width: 96,
            height: 1,
            depth: 1,
            array_size: 1,
            last_level: 0,
            nr_samples: 0,
            flags: 0,
            _padding: 0,
        };
        self.ctrl_submit_struct(&create)?;
        let attach = VirtioGpuCtxAttachResource {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE),
            resource_id: QUAD_RES,
            _padding: 0,
        };
        self.ctrl_submit_struct(&attach)?;
        // Clip-space corners (z 0, w 1): uv = xy/2 + 1/2 is 0 at the viewport's top left.
        #[rustfmt::skip]
        let corners: [f32; 24] = [
            -1.0, -1.0, 0.0, 1.0,   1.0, -1.0, 0.0, 1.0,   -1.0, 1.0, 0.0, 1.0,
             1.0, -1.0, 0.0, 1.0,   1.0,  1.0, 0.0, 1.0,   -1.0, 1.0, 0.0, 1.0,
        ];
        let mut bytes = [0u8; 96];
        for (b, v) in bytes.chunks_exact_mut(4).zip(corners) {
            b.copy_from_slice(&v.to_le_bytes());
        }
        let mut s = Submit3d::new();
        s.emit_resource_inline_write(QUAD_RES, &bytes);
        s.emit_create_blend_default(H_BLEND_REPLACE);
        s.emit_create_blend_premult(H_BLEND_PREMULTIPLIED);
        s.emit_create_dsa_default(H_DSA);
        s.emit_create_rasterizer_default(H_RASTERIZER);
        s.emit_create_vertex_elements(
            H_VERTEX_ELEMENTS,
            &[(0, 0, 0, PIPE_FORMAT_R32G32B32A32_FLOAT)],
        );
        s.emit_create_sampler_state_bilinear(H_SAMPLER);
        self.submit3d(&s)
    }

    /// One program's text, NUL-terminated, in chunks of at most [`CHUNK`] bytes, each its own
    /// `SUBMIT_3D` — the renderer holds a long program's first chunk until the rest arrive.
    fn create_shader(&mut self, handle: u32, stage: u32, text: &[u8]) -> Result<(), GfxError> {
        let total = text.len() as u32 + 1;
        let mut offset = 0;
        loop {
            let end = (offset + CHUNK).min(text.len());
            let last = end == text.len();
            let mut s = Submit3d::new();
            s.emit_create_shader_chunk(
                handle,
                stage,
                total,
                offset as u32,
                &text[offset..end],
                last,
            );
            self.submit3d(&s)?;
            if last {
                return Ok(());
            }
            offset = end;
        }
    }

    /// One draw of `p` over `q` — its rectangle in `target`'s pixels, its push constants — reading
    /// the sampler view `texture` when the pipeline samples (ignored otherwise). The full state
    /// each time: the context's state is global and every pass binds its own.
    pub(crate) fn emit_draw(
        &self,
        s: &mut Submit3d,
        p: Pipeline,
        target: u32,
        q: &Quad,
        texture: u32,
    ) {
        s.emit_bind_object(VIRGL_OBJECT_BLEND, p.blend);
        s.emit_bind_object(VIRGL_OBJECT_DSA, H_DSA);
        s.emit_bind_object(VIRGL_OBJECT_RASTERIZER, H_RASTERIZER);
        s.emit_bind_object(VIRGL_OBJECT_VERTEX_ELEMENTS, H_VERTEX_ELEMENTS);
        s.emit_set_framebuffer_state(0, &[target]);
        s.emit_set_viewport_box(q.x, q.y, q.w, q.h);
        if p.samples {
            s.emit_set_sampler_views(PIPE_SHADER_FRAGMENT, 0, &[texture]);
            s.emit_bind_sampler_states(PIPE_SHADER_FRAGMENT, 0, &[H_SAMPLER]);
        }
        if p.push > 0 {
            s.emit_set_constant_buffer(PIPE_SHADER_FRAGMENT, &q.push[..usize::from(p.push)]);
        }
        s.emit_bind_shader(self.pipelines.quad, PIPE_SHADER_VERTEX);
        s.emit_bind_shader(p.shader, PIPE_SHADER_FRAGMENT);
        s.emit_set_vertex_buffers(&[(16, 0, QUAD_RES)]);
        s.emit_draw_vbo(0, 6, PIPE_PRIM_TRIANGLES);
    }

    /// [`Self::emit_draw`] as its own stream.
    pub(crate) fn draw(
        &mut self,
        p: Pipeline,
        target: u32,
        q: &Quad,
        texture: u32,
    ) -> Result<(), GfxError> {
        let mut s = Submit3d::new();
        self.emit_draw(&mut s, p, target, q, texture);
        self.submit3d(&s)
    }

    /// A command stream as one `SUBMIT_3D` in gpud's virgl context (coalesced into the pending
    /// stream inside a batch).
    pub(crate) fn submit3d(&mut self, s: &Submit3d) -> Result<(), GfxError> {
        let bytes = s.as_bytes();
        let hdr = VirtioGpuSubmit3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_SUBMIT_3D),
            size: bytes.len() as u32,
            _padding: 0,
        };
        self.ctrl_submit_header_tail(&hdr, bytes)
    }
}

// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Virgl 3D command-stream encoders (TASK-0063 / RFC-0063 Phase 2).
//!
//! virglrenderer interprets the payload of `VIRTIO_GPU_CMD_SUBMIT_3D` as a
//! sequence of Gallium-like commands (`VIRGL_CCMD_*`). There is no Mesa guest
//! driver here, so this module hand-encodes that dword stream. The encoders are
//! pure (alloc-only, `no_std`-compatible) and fully unit-tested at the byte
//! level so the wire format can be validated without QEMU; `backend.rs` feeds
//! the resulting bytes to the control queue under the `virgl` feature.
//!
//! Command header layout (`virgl_protocol.h` `VIRGL_CMD0`):
//!   `dword0 = cmd | (object_type << 8) | (payload_len_in_dwords << 16)`
//!
//! OWNERS: @ui @runtime
//! STATUS: Experimental — encoders + golden tests; GPU bring-up is iterative.
//! API_STABILITY: Unstable

extern crate alloc;
use alloc::vec::Vec;

// ── Command opcodes (VIRGL_CCMD_*) ──────────────────────────────────
pub const VIRGL_CCMD_NOP: u32 = 0;
pub const VIRGL_CCMD_CREATE_OBJECT: u32 = 1;
pub const VIRGL_CCMD_BIND_OBJECT: u32 = 2;
pub const VIRGL_CCMD_DESTROY_OBJECT: u32 = 3;
pub const VIRGL_CCMD_SET_VIEWPORT_STATE: u32 = 4;
pub const VIRGL_CCMD_SET_FRAMEBUFFER_STATE: u32 = 5;
pub const VIRGL_CCMD_SET_VERTEX_BUFFERS: u32 = 6;
pub const VIRGL_CCMD_CLEAR: u32 = 7;
pub const VIRGL_CCMD_DRAW_VBO: u32 = 8;
pub const VIRGL_CCMD_RESOURCE_INLINE_WRITE: u32 = 9;
pub const VIRGL_CCMD_SET_SAMPLER_VIEWS: u32 = 10;
pub const VIRGL_CCMD_SET_CONSTANT_BUFFER: u32 = 12;
pub const VIRGL_CCMD_BIND_SAMPLER_STATES: u32 = 18;
pub const VIRGL_CCMD_BIND_SHADER: u32 = 31;

// ── Object types (VIRGL_OBJECT_*) ───────────────────────────────────
pub const VIRGL_OBJECT_NULL: u32 = 0;
pub const VIRGL_OBJECT_BLEND: u32 = 1;
pub const VIRGL_OBJECT_RASTERIZER: u32 = 2;
pub const VIRGL_OBJECT_DSA: u32 = 3;
pub const VIRGL_OBJECT_SHADER: u32 = 4;
pub const VIRGL_OBJECT_VERTEX_ELEMENTS: u32 = 5;
pub const VIRGL_OBJECT_SAMPLER_VIEW: u32 = 6;
pub const VIRGL_OBJECT_SAMPLER_STATE: u32 = 7;
pub const VIRGL_OBJECT_SURFACE: u32 = 8;

/// Shader stage selector for `CREATE_OBJECT(SHADER)` (PIPE_SHADER_*).
pub const PIPE_SHADER_VERTEX: u32 = 0;
pub const PIPE_SHADER_FRAGMENT: u32 = 1;

/// `PIPE_CLEAR_COLOR0` — clear the first color buffer.
pub const PIPE_CLEAR_COLOR0: u32 = 1 << 2;

// ── Gallium resource enums (used by RESOURCE_CREATE_3D / CREATE_OBJECT) ──
/// `PIPE_BUFFER` (vertex/index/constant buffers).
pub const PIPE_BUFFER: u32 = 0;
/// `PIPE_TEXTURE_2D`.
pub const PIPE_TEXTURE_2D: u32 = 2;
/// `PIPE_FORMAT_B8G8R8A8_UNORM` — matches the BGRA framebuffer layout.
pub const PIPE_FORMAT_B8G8R8A8_UNORM: u32 = 1;
/// `VIRGL_FORMAT_R32G32B32A32_FLOAT` (gallium p_format ordering).
pub const PIPE_FORMAT_R32G32B32A32_FLOAT: u32 = 31;
/// `VIRGL_FORMAT_R8_UNORM` — the conventional format for raw buffers.
pub const PIPE_FORMAT_R8_UNORM: u32 = 64;
/// `PIPE_BIND_RENDER_TARGET` (bit 1).
pub const PIPE_BIND_RENDER_TARGET: u32 = 1 << 1;
/// `PIPE_BIND_SAMPLER_VIEW` (bit 3).
pub const PIPE_BIND_SAMPLER_VIEW: u32 = 1 << 3;
/// `PIPE_BIND_VERTEX_BUFFER` (bit 4).
pub const PIPE_BIND_VERTEX_BUFFER: u32 = 1 << 4;
/// `VIRGL_RES_BIND_SCANOUT` (bit 18) — a resource the host display may present
/// directly (QEMU `dpy_gl_scanout_texture` on virtio-gpu-gl).
pub const PIPE_BIND_SCANOUT: u32 = 1 << 18;
/// `PIPE_PRIM_TRIANGLES`.
pub const PIPE_PRIM_TRIANGLES: u32 = 4;

/// Encode a virgl command header dword.
///
/// `len` is the number of payload dwords that follow the header (not counting
/// the header itself).
#[inline]
pub const fn cmd0(cmd: u32, object_type: u32, len: u32) -> u32 {
    cmd | (object_type << 8) | (len << 16)
}

/// Accumulates a `SUBMIT_3D` command stream as little-endian `u32` dwords.
///
/// Each `emit_*` appends one complete command (header + payload). `as_bytes`
/// produces the little-endian byte buffer that goes into the control-queue
/// command after the `VirtioGpuSubmit3d` header.
#[derive(Default)]
pub struct Submit3d {
    words: Vec<u32>,
}

impl Submit3d {
    pub fn new() -> Self {
        Self { words: Vec::new() }
    }

    /// Number of dwords currently in the stream.
    pub fn len_dwords(&self) -> usize {
        self.words.len()
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    /// Raw dword view (for tests / inspection).
    pub fn words(&self) -> &[u32] {
        &self.words
    }

    /// Little-endian serialization of the stream.
    pub fn as_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.words.len() * 4);
        for w in &self.words {
            out.extend_from_slice(&w.to_le_bytes());
        }
        out
    }

    fn push_header(&mut self, cmd: u32, object_type: u32, payload_len: u32) {
        self.words.push(cmd0(cmd, object_type, payload_len));
    }

    /// `VIRGL_CCMD_NOP` — a no-op command with a single dummy payload dword.
    ///
    /// Used to validate the `SUBMIT_3D` framing + context routing end-to-end
    /// against virglrenderer before the full blur pipeline exists: a malformed
    /// stream is rejected, a well-formed one returns `RESP_OK_NODATA`.
    pub fn emit_nop(&mut self) {
        self.push_header(VIRGL_CCMD_NOP, 0, 1);
        self.words.push(0);
    }

    /// `VIRGL_CCMD_CLEAR` — clear the bound framebuffer.
    ///
    /// Payload (8 dwords): buffers mask, RGBA as 4 f32 bit-patterns,
    /// depth as an f64 (2 dwords, low then high), stencil.
    pub fn emit_clear(&mut self, buffers: u32, rgba: [f32; 4], depth: f64, stencil: u32) {
        self.push_header(VIRGL_CCMD_CLEAR, 0, 8);
        self.words.push(buffers);
        for c in rgba {
            self.words.push(c.to_bits());
        }
        let d = depth.to_bits();
        self.words.push((d & 0xFFFF_FFFF) as u32);
        self.words.push((d >> 32) as u32);
        self.words.push(stencil);
    }

    /// `VIRGL_CCMD_SET_FRAMEBUFFER_STATE`.
    ///
    /// Payload: `nr_cbufs`, zsurf handle, then one surface handle per color
    /// buffer. A zsurf/handle of 0 means "none".
    pub fn emit_set_framebuffer_state(&mut self, zsurf_handle: u32, color_surfaces: &[u32]) {
        let nr = color_surfaces.len() as u32;
        self.push_header(VIRGL_CCMD_SET_FRAMEBUFFER_STATE, 0, 2 + nr);
        self.words.push(nr);
        self.words.push(zsurf_handle);
        self.words.extend_from_slice(color_surfaces);
    }

    /// `VIRGL_CCMD_SET_VIEWPORT_STATE` for a single viewport (index 0).
    ///
    /// Payload (7 dwords): start_slot, then scale[xyz] and translate[xyz]
    /// as f32 bit-patterns. For a pixel-space viewport of `w × h` the
    /// half-extents are the scale and the center is the translate.
    pub fn emit_set_viewport(&mut self, width: f32, height: f32) {
        self.emit_set_viewport_box(0.0, 0.0, width, height);
    }

    /// `SET_VIEWPORT_STATE` mapping NDC −1..1 onto the pixel box
    /// `(x, y, w, h)` — clip-space clipping at ±1 bounds the rasterized
    /// region exactly to the box (an exact-quad draw needs no scissor).
    pub fn emit_set_viewport_box(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.push_header(VIRGL_CCMD_SET_VIEWPORT_STATE, 0, 7);
        self.words.push(0); // start_slot
        let scale = [w / 2.0, h / 2.0, 0.5];
        let translate = [x + w / 2.0, y + h / 2.0, 0.5];
        for s in scale {
            self.words.push(s.to_bits());
        }
        for t in translate {
            self.words.push(t.to_bits());
        }
    }

    /// `CREATE_OBJECT(SHADER)` — create a vertex/fragment shader from TGSI
    /// assembly **text** (virglrenderer parses it with `tgsi_text_translate`,
    /// so no binary token encoding is needed).
    ///
    /// Layout (matches Mesa `virgl_encode_shader_state`, single non-chunked
    /// pass): handle, shader type, offlen, num_tokens, num_stream_outputs (0),
    /// then the NUL-terminated text packed little-endian and zero-padded to a
    /// dword boundary.
    ///
    /// `offlen` on the first (here: only) chunk is the TOTAL text length in
    /// bytes including the NUL — virglrenderer compares it against the bytes
    /// present in this command to decide whether continuation chunks follow
    /// (bit 31 = continuation flag, unset here). Sending 0 desyncs the decoder.
    pub fn emit_create_shader(&mut self, handle: u32, shader_type: u32, tgsi_text: &str) {
        let src = tgsi_text.as_bytes();
        let mut buf = Vec::with_capacity(src.len() + 4);
        buf.extend_from_slice(src);
        buf.push(0); // NUL terminator
        while buf.len() % 4 != 0 {
            buf.push(0); // pad to dword
        }
        let text_dwords = buf.len() / 4;
        // 5 fixed header dwords (handle, type, offlen, num_tokens, so_outputs).
        self.push_header(
            VIRGL_CCMD_CREATE_OBJECT,
            VIRGL_OBJECT_SHADER,
            (5 + text_dwords) as u32,
        );
        self.words.push(handle);
        self.words.push(shader_type);
        // offlen: full text length in bytes incl. NUL; no continuation bit.
        self.words.push((src.len() + 1) as u32);
        // num_tokens: sizes virglrenderer's TGSI token buffer. The text form is
        // far larger than the binary tokens, so bytes/4 + slack is a safe bound.
        self.words.push((src.len() / 4 + 16) as u32);
        self.words.push(0); // num stream-output declarations
        for chunk in buf.chunks_exact(4) {
            self.words.push(u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
        }
    }

    /// `CREATE_OBJECT(SURFACE)` — wrap a resource as a render-target / sampler
    /// surface view.
    ///
    /// Payload (5 dwords): object handle, backing resource handle, format, and
    /// for a 2D texture the mip level and the packed layer range (0 for a
    /// single-layer level-0 view).
    pub fn emit_create_surface(&mut self, handle: u32, res_handle: u32, format: u32) {
        self.push_header(VIRGL_CCMD_CREATE_OBJECT, VIRGL_OBJECT_SURFACE, 5);
        self.words.push(handle);
        self.words.push(res_handle);
        self.words.push(format);
        self.words.push(0); // texture mip level
        self.words.push(0); // packed first/last layer
    }

    /// `VIRGL_CCMD_BIND_OBJECT` — bind a previously created object by handle.
    pub fn emit_bind_object(&mut self, object_type: u32, handle: u32) {
        self.push_header(VIRGL_CCMD_BIND_OBJECT, object_type, 1);
        self.words.push(handle);
    }

    /// `VIRGL_CCMD_BIND_SHADER` — bind a shader by handle for a stage.
    pub fn emit_bind_shader(&mut self, handle: u32, shader_type: u32) {
        self.push_header(VIRGL_CCMD_BIND_SHADER, 0, 2);
        self.words.push(handle);
        self.words.push(shader_type);
    }

    /// `CREATE_OBJECT(BLEND)` — default state: blending disabled, full RGBA
    /// colormask on every render target.
    ///
    /// Payload (11 dwords): handle, S0 (independent/logicop/dither flags = 0),
    /// S1 (logicop func = 0), then 8 per-RT dwords (enable=0, colormask=0xF<<27).
    pub fn emit_create_blend_default(&mut self, handle: u32) {
        self.push_header(VIRGL_CCMD_CREATE_OBJECT, VIRGL_OBJECT_BLEND, 11);
        self.words.push(handle);
        self.words.push(0); // S0
        self.words.push(0); // S1
        for _ in 0..8 {
            self.words.push(0xF << 27); // RT: blend off, colormask RGBA
        }
    }

    /// Standard "over" alpha blending on RT0:
    /// rgb = src.rgb·src.a + dst.rgb·(1−src.a); a = src.a + dst.a·(1−src.a).
    /// RT dword layout (VIRGL_OBJ_BLEND_S2): enable(0) | rgb_func(1..3) |
    /// rgb_src(4..8) | rgb_dst(9..13) | a_func(14..16) | a_src(17..21) |
    /// a_dst(22..26) | colormask(27..30).
    pub fn emit_create_blend_alpha(&mut self, handle: u32) {
        const PIPE_BLEND_ADD: u32 = 0;
        const PIPE_BLENDFACTOR_ONE: u32 = 0x1;
        const PIPE_BLENDFACTOR_SRC_ALPHA: u32 = 0x3;
        const PIPE_BLENDFACTOR_INV_SRC_ALPHA: u32 = 0x13;
        let rt0 = 1
            | (PIPE_BLEND_ADD << 1)
            | (PIPE_BLENDFACTOR_SRC_ALPHA << 4)
            | (PIPE_BLENDFACTOR_INV_SRC_ALPHA << 9)
            | (PIPE_BLEND_ADD << 14)
            | (PIPE_BLENDFACTOR_ONE << 17)
            | (PIPE_BLENDFACTOR_INV_SRC_ALPHA << 22)
            | (0xF << 27);
        self.push_header(VIRGL_CCMD_CREATE_OBJECT, VIRGL_OBJECT_BLEND, 11);
        self.words.push(handle);
        self.words.push(0); // S0
        self.words.push(0); // S1
        self.words.push(rt0);
        for _ in 0..7 {
            self.words.push(0xF << 27);
        }
    }

    /// `CREATE_OBJECT(DSA)` — depth/stencil/alpha all disabled.
    ///
    /// Payload (5 dwords): handle, S0, two stencil dwords, alpha_ref (f32).
    pub fn emit_create_dsa_default(&mut self, handle: u32) {
        self.push_header(VIRGL_CCMD_CREATE_OBJECT, VIRGL_OBJECT_DSA, 5);
        self.words.push(handle);
        self.words.push(0); // S0: depth disabled
        self.words.push(0); // stencil[0]
        self.words.push(0); // stencil[1]
        self.words.push(0.0f32.to_bits()); // alpha_ref
    }

    /// `CREATE_OBJECT(RASTERIZER)` — solid fill, no culling, scissor off.
    ///
    /// Payload (9 dwords): handle, S0 bitfield, point_size, sprite_coord_enable,
    /// S3, line_width, offset_units, offset_scale, offset_clamp.
    pub fn emit_create_rasterizer_default(&mut self, handle: u32) {
        self.push_header(VIRGL_CCMD_CREATE_OBJECT, VIRGL_OBJECT_RASTERIZER, 9);
        self.words.push(handle);
        // S0: depth_clip = bit1. Everything else 0 = flat defaults
        // (cull NONE, fill FRONT/BACK solid, scissor off).
        self.words.push(1 << 1);
        self.words.push(1.0f32.to_bits()); // point_size
        self.words.push(0); // sprite_coord_enable
        self.words.push(0); // S3
        self.words.push(1.0f32.to_bits()); // line_width
        self.words.push(0.0f32.to_bits()); // offset_units
        self.words.push(0.0f32.to_bits()); // offset_scale
        self.words.push(0.0f32.to_bits()); // offset_clamp
    }

    /// `CREATE_OBJECT(VERTEX_ELEMENTS)` — one entry per vertex attribute.
    ///
    /// Per element (4 dwords): src_offset, instance_divisor,
    /// vertex_buffer_index, src_format.
    pub fn emit_create_vertex_elements(&mut self, handle: u32, elements: &[(u32, u32, u32, u32)]) {
        self.push_header(
            VIRGL_CCMD_CREATE_OBJECT,
            VIRGL_OBJECT_VERTEX_ELEMENTS,
            1 + 4 * elements.len() as u32,
        );
        self.words.push(handle);
        for &(src_offset, divisor, buffer_index, format) in elements {
            self.words.push(src_offset);
            self.words.push(divisor);
            self.words.push(buffer_index);
            self.words.push(format);
        }
    }

    /// `CREATE_OBJECT(SAMPLER_VIEW)` — texture view for shader sampling.
    ///
    /// Payload (6 dwords): handle, resource handle, format, val0/val1
    /// (layer/level ranges — 0 for a single-level 2D texture), swizzle
    /// (identity = r,g,b,a packed 3 bits each).
    pub fn emit_create_sampler_view(&mut self, handle: u32, res_handle: u32, format: u32) {
        self.push_header(VIRGL_CCMD_CREATE_OBJECT, VIRGL_OBJECT_SAMPLER_VIEW, 6);
        self.words.push(handle);
        self.words.push(res_handle);
        self.words.push(format);
        self.words.push(0); // val0: first/last layer
        self.words.push(0); // val1: first/last level
        self.words.push(0x688); // swizzle identity: 0 | 1<<3 | 2<<6 | 3<<9
    }

    /// `CREATE_OBJECT(SAMPLER_STATE)` — nearest filtering, clamp-to-edge.
    ///
    /// Payload (9 dwords): handle, S0 bitfield, lod_bias, min_lod, max_lod,
    /// border color RGBA (4 f32).
    pub fn emit_create_sampler_state_default(&mut self, handle: u32) {
        self.push_header(VIRGL_CCMD_CREATE_OBJECT, VIRGL_OBJECT_SAMPLER_STATE, 9);
        self.words.push(handle);
        // S0: wrap_s/t/r = CLAMP_TO_EDGE(2) at bits 0-2/3-5/6-8;
        // min/mag img filter NEAREST(0) at bits 9-10/13-14;
        // min mip filter NONE(2) at bits 11-12.
        self.words.push(2 | (2 << 3) | (2 << 6) | (2 << 11));
        self.words.push(0.0f32.to_bits()); // lod_bias
        self.words.push(0.0f32.to_bits()); // min_lod
        self.words.push(0.0f32.to_bits()); // max_lod
        for _ in 0..4 {
            self.words.push(0.0f32.to_bits()); // border color
        }
    }

    /// `VIRGL_CCMD_SET_VERTEX_BUFFERS` — per buffer: stride, offset, resource.
    pub fn emit_set_vertex_buffers(&mut self, buffers: &[(u32, u32, u32)]) {
        self.push_header(VIRGL_CCMD_SET_VERTEX_BUFFERS, 0, 3 * buffers.len() as u32);
        for &(stride, offset, res_handle) in buffers {
            self.words.push(stride);
            self.words.push(offset);
            self.words.push(res_handle);
        }
    }

    /// `VIRGL_CCMD_SET_SAMPLER_VIEWS` — bind sampler views for a shader stage.
    pub fn emit_set_sampler_views(&mut self, shader_type: u32, start_slot: u32, handles: &[u32]) {
        self.push_header(VIRGL_CCMD_SET_SAMPLER_VIEWS, 0, 2 + handles.len() as u32);
        self.words.push(shader_type);
        self.words.push(start_slot);
        self.words.extend_from_slice(handles);
    }

    /// `VIRGL_CCMD_BIND_SAMPLER_STATES` — bind sampler states for a stage.
    pub fn emit_bind_sampler_states(&mut self, shader_type: u32, start_slot: u32, handles: &[u32]) {
        self.push_header(VIRGL_CCMD_BIND_SAMPLER_STATES, 0, 2 + handles.len() as u32);
        self.words.push(shader_type);
        self.words.push(start_slot);
        self.words.extend_from_slice(handles);
    }

    /// `VIRGL_CCMD_SET_CONSTANT_BUFFER` — inline constants for a shader stage.
    pub fn emit_set_constant_buffer(&mut self, shader_type: u32, values: &[f32]) {
        self.push_header(VIRGL_CCMD_SET_CONSTANT_BUFFER, 0, 2 + values.len() as u32);
        self.words.push(shader_type);
        self.words.push(0); // index
        for v in values {
            self.words.push(v.to_bits());
        }
    }

    /// `VIRGL_CCMD_DRAW_VBO` — non-indexed draw.
    ///
    /// Payload (12 dwords): start, count, mode, indexed, instance_count,
    /// index_bias, start_instance, primitive_restart, restart_index,
    /// min_index, max_index, cso.
    pub fn emit_draw_vbo(&mut self, start: u32, count: u32, mode: u32) {
        self.push_header(VIRGL_CCMD_DRAW_VBO, 0, 12);
        self.words.push(start);
        self.words.push(count);
        self.words.push(mode);
        self.words.push(0); // indexed
        self.words.push(1); // instance_count
        self.words.push(0); // index_bias
        self.words.push(0); // start_instance
        self.words.push(0); // primitive_restart
        self.words.push(0); // restart_index
        self.words.push(0); // min_index
        self.words.push(count.saturating_sub(1)); // max_index
        self.words.push(0); // cso
    }

    /// `VIRGL_CCMD_RESOURCE_INLINE_WRITE` — upload raw bytes into a resource
    /// region inline in the command stream (used for small vertex buffers).
    ///
    /// Header payload (11 dwords): res_handle, level, usage, stride,
    /// layer_stride, box x/y/z/w/h/d — then the data, dword-padded.
    pub fn emit_resource_inline_write(&mut self, res_handle: u32, data: &[u8]) {
        let mut buf = Vec::with_capacity(data.len() + 4);
        buf.extend_from_slice(data);
        while buf.len() % 4 != 0 {
            buf.push(0);
        }
        let data_dwords = buf.len() / 4;
        self.push_header(
            VIRGL_CCMD_RESOURCE_INLINE_WRITE,
            0,
            (11 + data_dwords) as u32,
        );
        self.words.push(res_handle);
        self.words.push(0); // level
        self.words.push(0); // usage
        self.words.push(0); // stride (tightly packed)
        self.words.push(0); // layer_stride
        self.words.push(0); // box.x
        self.words.push(0); // box.y
        self.words.push(0); // box.z
        self.words.push(data.len() as u32); // box.w (bytes for PIPE_BUFFER)
        self.words.push(1); // box.h
        self.words.push(1); // box.d
        for chunk in buf.chunks_exact(4) {
            self.words.push(u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
        }
    }

    /// `VIRGL_CCMD_DESTROY_OBJECT`.
    pub fn emit_destroy_object(&mut self, handle: u32) {
        self.push_header(VIRGL_CCMD_DESTROY_OBJECT, 0, 1);
        self.words.push(handle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_packs_cmd_object_len() {
        // CREATE_OBJECT of a SHADER with a 5-dword payload.
        let h = cmd0(VIRGL_CCMD_CREATE_OBJECT, VIRGL_OBJECT_SHADER, 5);
        assert_eq!(h & 0xFF, VIRGL_CCMD_CREATE_OBJECT);
        assert_eq!((h >> 8) & 0xFF, VIRGL_OBJECT_SHADER);
        assert_eq!(h >> 16, 5);
        // Concretely: 1 | (4<<8) | (5<<16) = 0x0005_0401.
        assert_eq!(h, 0x0005_0401);
    }

    #[test]
    fn clear_stream_is_exact() {
        let mut s = Submit3d::new();
        s.emit_clear(PIPE_CLEAR_COLOR0, [1.0, 0.0, 0.0, 1.0], 1.0, 0);
        // header + 8 payload dwords.
        assert_eq!(s.len_dwords(), 9);
        let w = s.words();
        assert_eq!(w[0], cmd0(VIRGL_CCMD_CLEAR, 0, 8));
        assert_eq!(w[1], PIPE_CLEAR_COLOR0);
        assert_eq!(w[2], 1.0f32.to_bits()); // R
        assert_eq!(w[3], 0.0f32.to_bits()); // G
        assert_eq!(w[4], 0.0f32.to_bits()); // B
        assert_eq!(w[5], 1.0f32.to_bits()); // A
        let d = 1.0f64.to_bits();
        assert_eq!(w[6], (d & 0xFFFF_FFFF) as u32);
        assert_eq!(w[7], (d >> 32) as u32);
        assert_eq!(w[8], 0); // stencil
        // Bytes are little-endian and 4× the dword count.
        assert_eq!(s.as_bytes().len(), 36);
        assert_eq!(&s.as_bytes()[0..4], &cmd0(VIRGL_CCMD_CLEAR, 0, 8).to_le_bytes());
    }

    #[test]
    fn framebuffer_state_one_color_buffer() {
        let mut s = Submit3d::new();
        s.emit_set_framebuffer_state(0, &[7]);
        assert_eq!(s.len_dwords(), 4);
        let w = s.words();
        assert_eq!(w[0], cmd0(VIRGL_CCMD_SET_FRAMEBUFFER_STATE, 0, 3));
        assert_eq!(w[1], 1); // nr_cbufs
        assert_eq!(w[2], 0); // zsurf = none
        assert_eq!(w[3], 7); // color surface handle
    }

    #[test]
    fn viewport_pixel_space_half_extents() {
        let mut s = Submit3d::new();
        s.emit_set_viewport(1280.0, 800.0);
        assert_eq!(s.len_dwords(), 8);
        let w = s.words();
        assert_eq!(w[0], cmd0(VIRGL_CCMD_SET_VIEWPORT_STATE, 0, 7));
        assert_eq!(w[1], 0); // start_slot
        assert_eq!(w[2], 640.0f32.to_bits()); // scale x = w/2
        assert_eq!(w[3], 400.0f32.to_bits()); // scale y = h/2
        assert_eq!(w[5], 640.0f32.to_bits()); // translate x = w/2
    }

    #[test]
    fn bind_and_destroy_objects() {
        let mut s = Submit3d::new();
        s.emit_bind_object(VIRGL_OBJECT_SHADER, 42);
        s.emit_destroy_object(42);
        let w = s.words();
        assert_eq!(w[0], cmd0(VIRGL_CCMD_BIND_OBJECT, VIRGL_OBJECT_SHADER, 1));
        assert_eq!(w[1], 42);
        assert_eq!(w[2], cmd0(VIRGL_CCMD_DESTROY_OBJECT, 0, 1));
        assert_eq!(w[3], 42);
    }

    #[test]
    fn create_shader_packs_text() {
        let mut s = Submit3d::new();
        // 7-char text → +NUL = 8 bytes = 2 dwords; 5 fixed + 2 = len 7.
        s.emit_create_shader(3, PIPE_SHADER_FRAGMENT, "FRAG\nEN");
        let w = s.words();
        assert_eq!(w[0], cmd0(VIRGL_CCMD_CREATE_OBJECT, VIRGL_OBJECT_SHADER, 7));
        assert_eq!(w[1], 3); // handle
        assert_eq!(w[2], PIPE_SHADER_FRAGMENT);
        assert_eq!(w[3], 8); // offlen = strlen+1 bytes, no CONT bit
        assert_eq!(w[4], 7 / 4 + 16); // num_tokens bound
        assert_eq!(w[5], 0); // so outputs
        // "FRAG" = 0x46 0x52 0x41 0x47 little-endian.
        assert_eq!(w[6], u32::from_le_bytes([b'F', b'R', b'A', b'G']));
        assert_eq!(w[7], u32::from_le_bytes([b'\n', b'E', b'N', 0]));
        assert_eq!(s.len_dwords(), 8);
    }

    #[test]
    fn create_surface_payload() {
        let mut s = Submit3d::new();
        s.emit_create_surface(10, 5, PIPE_FORMAT_B8G8R8A8_UNORM);
        assert_eq!(s.len_dwords(), 6);
        let w = s.words();
        assert_eq!(w[0], cmd0(VIRGL_CCMD_CREATE_OBJECT, VIRGL_OBJECT_SURFACE, 5));
        assert_eq!(w[1], 10); // object handle
        assert_eq!(w[2], 5); // resource handle
        assert_eq!(w[3], PIPE_FORMAT_B8G8R8A8_UNORM);
        assert_eq!(w[4], 0); // level
        assert_eq!(w[5], 0); // layers
    }

    #[test]
    fn rt_clear_sequence_dword_count() {
        // The Increment-A stream: create surface, bind it as fb, clear.
        let mut s = Submit3d::new();
        s.emit_create_surface(1, 2, PIPE_FORMAT_B8G8R8A8_UNORM); // 6
        s.emit_set_framebuffer_state(0, &[1]); // 4
        s.emit_clear(PIPE_CLEAR_COLOR0, [1.0, 0.0, 0.0, 1.0], 1.0, 0); // 9
        assert_eq!(s.len_dwords(), 19);
        assert_eq!(s.words()[0], cmd0(VIRGL_CCMD_CREATE_OBJECT, VIRGL_OBJECT_SURFACE, 5));
        assert_eq!(s.words()[6], cmd0(VIRGL_CCMD_SET_FRAMEBUFFER_STATE, 0, 3));
        assert_eq!(s.words()[10], cmd0(VIRGL_CCMD_CLEAR, 0, 8));
    }

    #[test]
    fn multiple_commands_concatenate_in_order() {
        let mut s = Submit3d::new();
        s.emit_set_framebuffer_state(0, &[1]);
        s.emit_clear(PIPE_CLEAR_COLOR0, [0.0; 4], 1.0, 0);
        // 4 (fb state) + 9 (clear) = 13 dwords, framebuffer first.
        assert_eq!(s.len_dwords(), 13);
        assert_eq!(s.words()[0], cmd0(VIRGL_CCMD_SET_FRAMEBUFFER_STATE, 0, 3));
        assert_eq!(s.words()[4], cmd0(VIRGL_CCMD_CLEAR, 0, 8));
    }
}

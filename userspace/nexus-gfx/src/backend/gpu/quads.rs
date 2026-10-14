// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Dynamic geometry (TASK-0215 5b, RFC-0101 §8d): N textured, tinted quads drawn with the
//! captured render pass's programs. What a pass needs beyond the bundle, learned from the
//! captures of 2026-10-14 (`docs/board/measurements/2026-10-13-gpu-render-queue/README.md`,
//! "5b"): a vertex buffer of 6 N vertices named by a copy of the vertex PDS data; per draw a
//! 16-byte tint (the fragment program's push constants), 16 zero bytes behind it, a 16-byte
//! descriptor naming those, a constant word, a 48-byte copy of the fragment PDS uniform data
//! with three addresses patched, copies of the fragment PDS shader and varying programs (or the
//! first draw's, shared), and a PPP block naming them — the first draw's the captured 26-word
//! block with its PDS words patched, the others' seven words; and the geometry control stream:
//! the captured PDS state, the PPP update, the VDM state naming the vertex data copy, an index
//! list, then three records per further draw, the terminate block, `TERMINATE`. **Every pass
//! lands in fresh memory** (the open driver allocates afresh each frame): the GPU's caches keep
//! lines of what a pass read, so nothing a pass read is rewritten for the next (H10, 2026-10-14).
//! The emitter is pure: it returns the bytes and where they go, a [`Placer`] deciding where —
//! the device bumps through the buffers' unused tails, the goldens use the capture's offsets.

use alloc::vec::Vec;

use super::csb::{pds, ppp, vdm};

/// A quad in plane pixels with its tint (the fragment program multiplies the texel by it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quad {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub tint: [f32; 4],
}

/// The captured pass's shape the emitter builds on (the first-quad render capture).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Template<'a> {
    /// The `PDS_STATE_UPDATE` record (three words) and the `VDM_STATE_UPDATE` record (five
    /// words, its second the vertex PDS data address — patched per pass) as captured.
    pub pds_state_words: [u32; 3],
    pub vdm_state_words: [u32; 5],
    /// The first draw's 26-word PPP block (104 bytes); its STATE_PDS words at 5..11 are patched.
    pub ppp26: &'a [u8],
    /// The terminate PPP block's address and word count (`PPP_STATE_UPDATE` 3 words).
    pub terminate_ppp: (u64, u32),
    /// The vertex PDS data (64 bytes) and the byte offsets of its two vertex-buffer addresses
    /// (the position attribute at +0, the texture coordinate at +8).
    pub vs_pds_data: &'a [u8],
    pub vs_patched: [usize; 2],
    /// The fragment PDS uniform data (48 bytes), the master for the copies, and the byte
    /// offsets of its three patched descriptors: the tint, the tint's descriptor, the constant.
    pub uniform_data: &'a [u8],
    pub patched: [usize; 3],
    /// The constant word block's content (16 bytes), copied per draw.
    pub constant_block: &'a [u8],
    /// The fragment PDS shader and varying programs (64-byte lines), copied per pass.
    pub shader_program: &'a [u8],
    pub varying_program: &'a [u8],
    /// The arenas: the general arena's address, the vertex buffer's, the stream buffer's.
    pub general_va: u64,
    pub vertex_va: u64,
    pub stream_va: u64,
    /// The fragment PDS state of the first draw (sizes; the bases are replaced by the copies').
    pub pds: ppp::PdsState,
    /// The camera size of the bare VDM state.
    pub cam_size: u32,
    /// The index list's primitive topology (6 = triangle list).
    pub topology: u32,
}

/// What a block is, for a [`Placer`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Need {
    /// 16 bytes in the general arena.
    Tint,
    /// 16 zero bytes in the general arena, which the descriptor names.
    Zero,
    /// 16 bytes in the general arena: `{zero block's address, 4, 0}`.
    Descriptor,
    /// 16 bytes in the general arena: the constant word.
    Constant,
    /// 28 bytes in the general arena: a seven-word PPP block (draws after the first).
    Ppp,
    /// 104 bytes in the general arena: the 26-word PPP block (the first draw).
    Ppp26,
    /// 48 bytes in the PDS heap: the uniform data copy.
    Uniform,
    /// 64 bytes in the PDS heap: a copy of the fragment PDS shader program.
    Shader,
    /// 64 bytes in the PDS heap: a copy of the fragment PDS varying program.
    Varying,
    /// 64 bytes in the PDS heap: the vertex PDS data copy (once per pass).
    VsPdsData,
    /// The control stream, `len` bytes in the stream buffer (once per pass).
    Stream(u32),
    /// The vertices, `len` bytes in the vertex buffer (once per pass).
    Vertices(u32),
}

/// Where a block goes: `Some(offset)` into its buffer — the general arena (bytes from
/// `general_va`), the PDS heap (heap offset), the stream buffer, the vertex buffer — or `None`
/// when there is no room.
pub trait Placer {
    fn place(&mut self, draw: usize, need: Need) -> Option<u32>;
}

/// The device's placer: 64-byte lines bumped through the buffers' unused tails.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bump {
    pub general_free: u32,
    pub general_len: u32,
    pub pds_free: u32,
    pub pds_len: u32,
    pub stream_free: u32,
    pub stream_len: u32,
    pub vertex_free: u32,
    pub vertex_len: u32,
}

const BLOCK: u32 = 64;

impl Placer for Bump {
    fn place(&mut self, _draw: usize, need: Need) -> Option<u32> {
        let (free, len, bytes) = match need {
            Need::Uniform | Need::Shader | Need::Varying | Need::VsPdsData => {
                (&mut self.pds_free, self.pds_len, BLOCK)
            }
            Need::Ppp26 => (&mut self.general_free, self.general_len, 2 * BLOCK),
            Need::Stream(n) => (&mut self.stream_free, self.stream_len, n),
            Need::Vertices(n) => (&mut self.vertex_free, self.vertex_len, n),
            _ => (&mut self.general_free, self.general_len, BLOCK),
        };
        let at = free.next_multiple_of(BLOCK);
        let end = at.checked_add(bytes.next_multiple_of(BLOCK))?;
        if end > len {
            return None;
        }
        *free = end;
        Some(at)
    }
}

/// A write the device performs before the kick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Write {
    /// Offset into the buffer (general: bytes from `general_va`; PDS: heap offset).
    pub offset: u32,
    pub bytes: Vec<u8>,
}

/// What `emit` produced.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Emitted {
    /// The vertices and their offset in the vertex buffer.
    pub vertices: Vec<u8>,
    pub vertex_offset: u32,
    /// The control stream and its offset in the stream buffer; the geometry job's
    /// `vdm_ctrl_stream_base` must name `stream_va + stream_offset`.
    pub stream: Vec<u8>,
    pub stream_offset: u32,
    pub vdm_ctrl_stream_base: u64,
    pub general: Vec<Write>,
    pub pds: Vec<Write>,
    pub draws: u32,
}

/// Why a scene was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmitError {
    NoQuads,
    /// A buffer would overflow.
    Space,
    /// A template field is malformed (sizes, offsets).
    Template,
}

const VERTEX_BYTES: usize = 16;
const VERTS_PER_QUAD: usize = 6;
const UNIFORM_DATA_BYTES: usize = 48;
const PROGRAM_BYTES: usize = 64;
const PPP26_BYTES: usize = 104;
/// Word offsets of the STATE_PDS words inside the 26-word block: shaderbase, varyingbase,
/// uniformdatabase.
const PPP26_SHADERBASE: usize = 5;
const PPP26_VARYINGBASE: usize = 9;
const PPP26_UNIFORMDATABASE: usize = 10;

/// Bytes of a pass's control stream for `draws` draws.
pub const fn stream_bytes(draws: usize) -> usize {
    12 + 8 + 20 + 8 + (draws - 1) * 24 + 8 + 4
}

/// The six vertices of one quad: position in pixels, texture coordinates.
pub fn quad_vertices(q: &Quad) -> [[f32; 4]; VERTS_PER_QUAD] {
    let (x0, y0, x1, y1) = (q.x, q.y, q.x + q.w, q.y + q.h);
    [
        [x0, y0, 0.0, 0.0],
        [x1, y0, 1.0, 0.0],
        [x1, y1, 1.0, 1.0],
        [x0, y0, 0.0, 0.0],
        [x1, y1, 1.0, 1.0],
        [x0, y1, 0.0, 1.0],
    ]
}

fn push_words(out: &mut Vec<u8>, words: &[u32]) {
    for w in words {
        out.extend_from_slice(&w.to_le_bytes());
    }
}

fn put_word(b: &mut [u8], word: usize, v: u32) {
    b[word * 4..word * 4 + 4].copy_from_slice(&v.to_le_bytes());
}

fn tint_bytes(t: [f32; 4]) -> Vec<u8> {
    let mut b = Vec::with_capacity(16);
    for v in t {
        b.extend_from_slice(&v.to_le_bytes());
    }
    b
}

/// The 16-byte descriptor the uniform data names: `{address of the zero block, 4, 0}`.
fn descriptor(zero_va: u64) -> Vec<u8> {
    let mut b = Vec::with_capacity(16);
    b.extend_from_slice(&zero_va.to_le_bytes());
    b.extend_from_slice(&4u32.to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes());
    b
}

/// Emits the pass for `quads`; `place` decides where everything goes; `share_programs` names
/// the first draw's program copies from every draw instead of copying them per draw.
pub fn emit(
    t: &Template<'_>,
    quads: &[Quad],
    place: &mut dyn Placer,
    share_programs: bool,
) -> Result<Emitted, EmitError> {
    if quads.is_empty() {
        return Err(EmitError::NoQuads);
    }
    if t.uniform_data.len() != UNIFORM_DATA_BYTES
        || t.constant_block.len() != 16
        || t.patched.iter().any(|&p| p + 8 > UNIFORM_DATA_BYTES)
        || t.shader_program.len() != PROGRAM_BYTES
        || t.varying_program.len() != PROGRAM_BYTES
        || t.vs_pds_data.len() != PROGRAM_BYTES
        || t.vs_patched.iter().any(|&p| p + 8 > PROGRAM_BYTES)
        || t.ppp26.len() != PPP26_BYTES
    {
        return Err(EmitError::Template);
    }
    let n = quads.len();
    let mut out = Emitted { draws: n as u32, ..Emitted::default() };
    let mut at = |draw: usize, need| place.place(draw, need).ok_or(EmitError::Space);
    // The pass's own: the stream, the vertices, the vertex PDS data naming them.
    out.stream_offset = at(0, Need::Stream(stream_bytes(n) as u32))?;
    out.vdm_ctrl_stream_base = t.stream_va + u64::from(out.stream_offset);
    out.vertex_offset = at(0, Need::Vertices((n * VERTS_PER_QUAD * VERTEX_BYTES) as u32))?;
    for q in quads {
        for v in quad_vertices(q) {
            for f in v {
                out.vertices.extend_from_slice(&f.to_le_bytes());
            }
        }
    }
    let vs_off = at(0, Need::VsPdsData)?;
    let vertex_base = t.vertex_va + u64::from(out.vertex_offset);
    let mut vs = t.vs_pds_data.to_vec();
    pds::set_doutd_sbase(&mut vs[t.vs_patched[0]..], vertex_base);
    pds::set_doutd_sbase(&mut vs[t.vs_patched[1]..], vertex_base + 8);
    out.pds.push(Write { offset: vs_off, bytes: vs });
    push_words(&mut out.stream, &t.pds_state_words);
    let mut shared: Option<(u32, u32)> = None;
    for (i, q) in quads.iter().enumerate() {
        let tint_off = at(i, Need::Tint)?;
        let zero_off = at(i, Need::Zero)?;
        let desc_off = at(i, Need::Descriptor)?;
        let const_off = at(i, Need::Constant)?;
        let uniform_off = at(i, Need::Uniform)?;
        let tint_va = t.general_va + u64::from(tint_off);
        let zero_va = t.general_va + u64::from(zero_off);
        out.general.push(Write { offset: tint_off, bytes: tint_bytes(q.tint) });
        out.general.push(Write { offset: zero_off, bytes: alloc::vec![0u8; 16] });
        out.general.push(Write { offset: desc_off, bytes: descriptor(zero_va) });
        out.general.push(Write { offset: const_off, bytes: t.constant_block.to_vec() });
        let mut uniform = t.uniform_data.to_vec();
        pds::set_doutd_sbase(&mut uniform[t.patched[0]..], tint_va);
        pds::set_doutd_sbase(&mut uniform[t.patched[1]..], t.general_va + u64::from(desc_off));
        pds::set_doutd_sbase(&mut uniform[t.patched[2]..], t.general_va + u64::from(const_off));
        out.pds.push(Write { offset: uniform_off, bytes: uniform });
        // The programs: a copy per draw, or the first draw's shared.
        let (shader_off, varying_off) = match shared {
            Some(s) if share_programs => s,
            _ => {
                let s = (at(i, Need::Shader)?, at(i, Need::Varying)?);
                out.pds.push(Write { offset: s.0, bytes: t.shader_program.to_vec() });
                out.pds.push(Write { offset: s.1, bytes: t.varying_program.to_vec() });
                shared = Some(s);
                s
            }
        };
        let state = ppp::PdsState {
            shaderbase: shader_off,
            varyingbase: varying_off,
            uniformdatabase: uniform_off,
            ..t.pds
        };
        if i == 0 {
            // The first draw: the captured 26-word block with its PDS words patched, the VDM
            // state naming the vertex data copy, the index list.
            let ppp_off = at(0, Need::Ppp26)?;
            let mut block = t.ppp26.to_vec();
            put_word(&mut block, PPP26_SHADERBASE, state.shaderbase & !0xf);
            put_word(&mut block, PPP26_VARYINGBASE, state.varyingbase & !0xf);
            put_word(&mut block, PPP26_UNIFORMDATABASE, state.uniformdatabase & !0xf);
            out.general.push(Write { offset: ppp_off, bytes: block });
            push_words(&mut out.stream, &vdm::ppp_state(26, t.general_va + u64::from(ppp_off)));
            let mut vdm_state = t.vdm_state_words;
            vdm_state[1] = vs_off & !0xf;
            push_words(&mut out.stream, &vdm_state);
            let (words, len) = vdm::index_list(VERTS_PER_QUAD as u32, None, t.topology);
            push_words(&mut out.stream, &words[..len]);
        } else {
            let ppp_off = at(i, Need::Ppp)?;
            let mut block = Vec::with_capacity(28);
            push_words(&mut block, &state.block());
            out.general.push(Write { offset: ppp_off, bytes: block });
            push_words(&mut out.stream, &vdm::ppp_state(7, t.general_va + u64::from(ppp_off)));
            push_words(&mut out.stream, &[vdm::vdm_state_bare(t.cam_size)]);
            let (words, len) = vdm::index_list(
                VERTS_PER_QUAD as u32,
                Some((i * VERTS_PER_QUAD) as u32),
                t.topology,
            );
            push_words(&mut out.stream, &words[..len]);
        }
    }
    push_words(&mut out.stream, &vdm::ppp_state(t.terminate_ppp.1, t.terminate_ppp.0));
    push_words(&mut out.stream, &[vdm::TERMINATE]);
    debug_assert_eq!(out.stream.len(), stream_bytes(n));
    Ok(out)
}

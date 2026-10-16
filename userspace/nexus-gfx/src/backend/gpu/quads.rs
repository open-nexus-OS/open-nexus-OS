// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Dynamic geometry (TASK-0215 5b, RFC-0101 §8d), drawn with a library pipeline (5c-2 P4d,
//! RFC-0101 §7b): N textured, tinted quads. What a pass needs beyond the bundle, learned from the
//! captures of 2026-10-14 (`docs/board/measurements/2026-10-13-gpu-render-queue/README.md`,
//! "5b"): a vertex buffer of 6 N vertices named by a copy of the vertex PDS data; per draw its
//! push constants (as many as the pipeline's state load reads) and the pipeline's state-load
//! data naming them and the texture's descriptor; a PPP block naming the pipeline's setup programs — the first draw's
//! the captured 26-word block with its pass type and PDS words the pipeline's, the others' seven
//! words; and the geometry control stream: the captured PDS state, the PPP update, the VDM state
//! naming the vertex data copy, an index list, then three records per further draw, the
//! terminate block, `TERMINATE`. **Every pass lands in fresh memory** (the open driver allocates
//! afresh each frame): the GPU's caches keep lines of what a pass read, so nothing a pass read is
//! rewritten for the next (H10, 2026-10-14) — the pipeline's setup programs, placed once and
//! never rewritten, are read by every pass. The emitter is pure and allocates nothing: a
//! [`Placer`] decides where each block goes — the device bumps through the buffers' unused
//! tails, the goldens use the capture's offsets — and each block is made in place and handed to
//! a [`Sink`] — the device writes it into the bound buffer, the goldens record it.

use alloc::vec::Vec;

use super::csb::{pds, ppp, vdm};
use super::pipelines::Placed;
use crate::shader::rogue::setup::{Load, MAX_STATE_DWORDS};

/// A quad in plane pixels and the push constants its pipeline reads (up to sixteen floats).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quad {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub push: [f32; 16],
}

impl Quad {
    /// A layer (`resources/gpu/shaders/layer.wgsl`, its `Layer` block): the tint (colour and
    /// opacity), the size and minus half of it, half the size less the corner radius and the
    /// radius.
    pub const fn layer(x: f32, y: f32, w: f32, h: f32, tint: [f32; 4], radius: f32) -> Self {
        let (hw, hh) = (w * 0.5, h * 0.5);
        let mut push = [0.0; 16];
        push[0] = tint[0];
        push[1] = tint[1];
        push[2] = tint[2];
        push[3] = tint[3];
        push[4] = w;
        push[5] = h;
        push[6] = -hw;
        push[7] = -hh;
        push[8] = hw - radius;
        push[9] = hh - radius;
        push[10] = radius;
        Self { x, y, w, h, push }
    }

    /// A layer's tint (its first four push constants).
    pub const fn tint(&self) -> [f32; 4] {
        [self.push[0], self.push[1], self.push[2], self.push[3]]
    }

    /// The drop shadow of a layer at (`x`, `y`, `w`, `h`) with corner `radius`
    /// (`resources/gpu/shaders/shadow.wgsl`, its `Shadow` block): the quad covers the shape
    /// grown by the blur and moved by the offset; the push constants are the straight colour,
    /// the quad's size and minus half of it, half the shape less the radius with the radius and
    /// one over the blur, the offset.
    #[allow(clippy::too_many_arguments)]
    pub const fn shadow(
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radius: f32,
        blur: f32,
        offset: [f32; 2],
        colour: [f32; 4],
    ) -> Self {
        let (qw, qh) = (w + 2.0 * blur, h + 2.0 * blur);
        let mut push = [0.0; 16];
        push[0] = colour[0];
        push[1] = colour[1];
        push[2] = colour[2];
        push[3] = colour[3];
        push[4] = qw;
        push[5] = qh;
        push[6] = -qw * 0.5;
        push[7] = -qh * 0.5;
        push[8] = w * 0.5 - radius;
        push[9] = h * 0.5 - radius;
        push[10] = radius;
        push[11] = 1.0 / blur;
        push[12] = offset[0];
        push[13] = offset[1];
        Self { x: x - blur + offset[0], y: y - blur + offset[1], w: qw, h: qh, push }
    }

    /// A blur pass over (`x`, `y`, `w`, `h`) (`resources/gpu/shaders/blur.wgsl`, its `Blur`
    /// block): `uv · source.xy + source.zw` is an output pixel's centre in the source texture
    /// (normalised), `step` a tap's step there, `saturation` the vertical pass's (1 leaves the
    /// colour).
    pub const fn blur(
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        source: [f32; 4],
        step: [f32; 2],
        saturation: f32,
    ) -> Self {
        let mut push = [0.0; 16];
        push[0] = source[0];
        push[1] = source[1];
        push[2] = source[2];
        push[3] = source[3];
        push[4] = step[0];
        push[5] = step[1];
        push[6] = saturation;
        Self { x, y, w, h, push }
    }
}

/// The captured pass's shape the emitter builds on (the first-quad render capture): its vertex
/// half and the blocks around the draws; the fragment half is the pipeline's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Template<'a> {
    /// The `PDS_STATE_UPDATE` record (three words) and the `VDM_STATE_UPDATE` record (five
    /// words, its second the vertex PDS data address — patched per pass) as captured.
    pub pds_state_words: [u32; 3],
    pub vdm_state_words: [u32; 5],
    /// The first draw's 26-word PPP block (104 bytes); its ISP word's pass type and its
    /// STATE_PDS words at 5..=10 are the pipeline's.
    pub ppp26: &'a [u8],
    /// The terminate PPP block's address and word count (`PPP_STATE_UPDATE` 3 words).
    pub terminate_ppp: (u64, u32),
    /// The vertex PDS data (64 bytes) and the byte offsets of its two vertex-buffer addresses
    /// (the position attribute at +0, the texture coordinate at +8).
    pub vs_pds_data: &'a [u8],
    pub vs_patched: [usize; 2],
    /// The texture's descriptor block (texture and sampler state) every quad samples.
    pub texture_va: u64,
    /// The arenas: the general arena's address, the vertex buffer's, the stream buffer's.
    pub general_va: u64,
    pub vertex_va: u64,
    pub stream_va: u64,
    /// The camera size of the bare VDM state.
    pub cam_size: u32,
    /// The index list's primitive topology (6 = triangle list).
    pub topology: u32,
}

/// What a block is, for a [`Placer`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Need {
    /// `len` bytes in the general arena: a draw's push constants.
    Push(u32),
    /// 28 bytes in the general arena: a seven-word PPP block (draws after the first).
    Ppp,
    /// 104 bytes in the general arena: the 26-word PPP block (the first draw).
    Ppp26,
    /// `len` bytes in the PDS heap: a draw's state-load data.
    StateData(u32),
    /// 64 bytes in the PDS heap: the vertex PDS data copy (once per pass).
    VsPdsData,
    /// A texture's descriptor block in the general arena (`texstate`, once per pass and
    /// texture): what a pass's state load names as its texture.
    Descriptor,
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
const DESCRIPTOR_BYTES: u32 = 4 * super::texstate::DESCRIPTOR_DWORDS as u32;

impl Placer for Bump {
    fn place(&mut self, _draw: usize, need: Need) -> Option<u32> {
        let (free, len, bytes) = match need {
            Need::VsPdsData => (&mut self.pds_free, self.pds_len, BLOCK),
            Need::StateData(n) => (&mut self.pds_free, self.pds_len, n),
            Need::Push(n) => (&mut self.general_free, self.general_len, n),
            Need::Ppp26 => (&mut self.general_free, self.general_len, 2 * BLOCK),
            Need::Stream(n) => (&mut self.stream_free, self.stream_len, n),
            Need::Vertices(n) => (&mut self.vertex_free, self.vertex_len, n),
            Need::Descriptor => (&mut self.general_free, self.general_len, DESCRIPTOR_BYTES),
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

/// The buffer a block of a pass goes into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Region {
    /// The general arena (offsets from `Template::general_va`).
    General,
    /// The PDS heap (heap offsets).
    Pds,
    /// The stream buffer.
    Stream,
    /// The vertex buffer.
    Vertex,
}

/// Where a pass's bytes go as they are made: the device writes each block into its bound buffer
/// (and cleans it), a host test records them. Emitting allocates nothing — gpud's heap never
/// frees, and a frame emits passes for gpud's whole life.
pub trait Sink {
    /// Writes `bytes` at `offset` of `region`; false when it cannot.
    fn write(&mut self, region: Region, offset: u32, bytes: &[u8]) -> bool;
}

/// A block a pass wrote, as a [`Recorder`] keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Write {
    /// Offset into the buffer (general: bytes from `general_va`; PDS: heap offset).
    pub offset: u32,
    pub bytes: Vec<u8>,
}

/// A sink that keeps what a pass wrote (host tests and tools): the vertices and the stream joined
/// — each written in order, every write where the last ended —, the general arena's and the PDS
/// heap's blocks one by one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Recorder {
    pub vertices: Vec<u8>,
    pub stream: Vec<u8>,
    pub general: Vec<Write>,
    pub pds: Vec<Write>,
    /// Where the vertices and the stream began.
    starts: [Option<u32>; 2],
}

impl Sink for Recorder {
    fn write(&mut self, region: Region, offset: u32, bytes: &[u8]) -> bool {
        let joined = |buf: &mut Vec<u8>, start: &mut Option<u32>| {
            let at = *start.get_or_insert(offset);
            if offset.checked_sub(at) != u32::try_from(buf.len()).ok() {
                return false;
            }
            buf.extend_from_slice(bytes);
            true
        };
        match region {
            Region::Vertex => joined(&mut self.vertices, &mut self.starts[0]),
            Region::Stream => joined(&mut self.stream, &mut self.starts[1]),
            Region::General => {
                self.general.push(Write { offset, bytes: bytes.to_vec() });
                true
            }
            Region::Pds => {
                self.pds.push(Write { offset, bytes: bytes.to_vec() });
                true
            }
        }
    }
}

/// What `emit` produced beside its writes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Emitted {
    /// The vertices' offset in the vertex buffer.
    pub vertex_offset: u32,
    /// The control stream's offset in the stream buffer; the geometry job's
    /// `vdm_ctrl_stream_base` must name `stream_va + stream_offset`.
    pub stream_offset: u32,
    pub vdm_ctrl_stream_base: u64,
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
    /// The pipeline's setup cannot describe a draw (its state-load data).
    Pipeline,
    /// The sink refused a write.
    Sink,
}

const VERTEX_BYTES: usize = 16;
const VERTS_PER_QUAD: usize = 6;
const QUAD_BYTES: usize = VERTS_PER_QUAD * VERTEX_BYTES;
const VS_PDS_DATA_BYTES: usize = 64;
const PPP26_BYTES: usize = 104;
/// Word offsets inside the 26-word block: the ISP word (its pass type), the six STATE_PDS words.
const PPP26_ISPA: usize = 2;
const PPP26_PDS: usize = 5;
/// The most push-constant dwords a draw carries.
const MAX_PUSH_DWORDS: usize = 16;

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

/// Up to `N` bytes made in place: a block of a pass before it goes to the sink.
struct Bytes<const N: usize> {
    b: [u8; N],
    len: usize,
}

impl<const N: usize> Bytes<N> {
    const fn new() -> Self {
        Self { b: [0; N], len: 0 }
    }

    /// Appends `bytes`; false when they do not fit (the sizes below are the blocks' own).
    #[must_use]
    fn put(&mut self, bytes: &[u8]) -> bool {
        let end = self.len + bytes.len();
        match self.b.get_mut(self.len..end) {
            Some(dst) => {
                dst.copy_from_slice(bytes);
                self.len = end;
                true
            }
            None => false,
        }
    }

    #[must_use]
    fn words(&mut self, words: &[u32]) -> bool {
        words.iter().all(|w| self.put(&w.to_le_bytes()))
    }

    fn bytes(&self) -> &[u8] {
        &self.b[..self.len]
    }
}

fn fits(ok: bool) -> Result<(), EmitError> {
    if ok {
        Ok(())
    } else {
        Err(EmitError::Template)
    }
}

/// A pass's blocks on their way to the sink, with the control stream's running end.
struct Out<'a> {
    sink: &'a mut dyn Sink,
    stream_at: u32,
}

impl Out<'_> {
    fn write(&mut self, region: Region, offset: u32, bytes: &[u8]) -> Result<(), EmitError> {
        if self.sink.write(region, offset, bytes) {
            Ok(())
        } else {
            Err(EmitError::Sink)
        }
    }

    /// The control stream's next words.
    fn record(&mut self, words: &[u32]) -> Result<(), EmitError> {
        let mut b = Bytes::<48>::new();
        fits(b.words(words))?;
        self.write(Region::Stream, self.stream_at, b.bytes())?;
        self.stream_at += b.len as u32;
        Ok(())
    }
}

fn put_word(b: &mut [u8], word: usize, v: u32) {
    b[word * 4..word * 4 + 4].copy_from_slice(&v.to_le_bytes());
}

/// Emits the pass for `quads` drawn with `pipeline`: `place` decides where each block goes,
/// `sink` receives it as it is made.
pub fn emit(
    t: &Template<'_>,
    pipeline: &Placed,
    quads: &[Quad],
    place: &mut dyn Placer,
    sink: &mut dyn Sink,
) -> Result<Emitted, EmitError> {
    if quads.is_empty() {
        return Err(EmitError::NoQuads);
    }
    if t.vs_pds_data.len() != VS_PDS_DATA_BYTES
        || t.vs_patched.iter().any(|&p| p + 8 > VS_PDS_DATA_BYTES)
        || t.ppp26.len() != PPP26_BYTES
    {
        return Err(EmitError::Template);
    }
    let n = quads.len();
    let mut at = |draw: usize, need| place.place(draw, need).ok_or(EmitError::Space);
    // The pass's own: the stream, the vertices, the vertex PDS data naming them.
    let stream_offset = at(0, Need::Stream(stream_bytes(n) as u32))?;
    let mut out = Out { sink, stream_at: stream_offset };
    let vertex_offset = at(0, Need::Vertices((n * QUAD_BYTES) as u32))?;
    for (i, q) in quads.iter().enumerate() {
        let mut v = Bytes::<QUAD_BYTES>::new();
        for f in quad_vertices(q).iter().flatten() {
            fits(v.put(&f.to_le_bytes()))?;
        }
        out.write(Region::Vertex, vertex_offset + (i * QUAD_BYTES) as u32, v.bytes())?;
    }
    let vs_off = at(0, Need::VsPdsData)?;
    let vertex_base = t.vertex_va + u64::from(vertex_offset);
    let mut vs = [0u8; VS_PDS_DATA_BYTES];
    vs.copy_from_slice(t.vs_pds_data);
    pds::set_doutd_sbase(&mut vs[t.vs_patched[0]..], vertex_base);
    pds::set_doutd_sbase(&mut vs[t.vs_patched[1]..], vertex_base + 8);
    out.write(Region::Pds, vs_off, &vs)?;
    out.record(&t.pds_state_words)?;
    let data_bytes = (4 * pipeline.setup.state_data_dwords()) as u32;
    // The push constants the pipeline's state load reads: their first dwords of each quad's.
    let push_dwords = pipeline
        .setup
        .dmas
        .iter()
        .filter_map(|d| match d.load {
            Load::Push { from } => Some(usize::from(from) + usize::from(d.dwords)),
            Load::TextureDescriptor { .. } | Load::Constants => None,
        })
        .max()
        .unwrap_or(0);
    if push_dwords > MAX_PUSH_DWORDS {
        return Err(EmitError::Pipeline);
    }
    // The constant block where the pipeline's load placed it (a block no placement names is a
    // pipeline the table did not place).
    let constants_va = pipeline.constants_va();
    if pipeline.setup.dmas.iter().any(|d| d.load == Load::Constants) && constants_va.is_none() {
        return Err(EmitError::Pipeline);
    }
    for (i, q) in quads.iter().enumerate() {
        // The draw's push constants, and the state load's data naming them and the texture.
        let mut push_va = 0;
        if push_dwords > 0 {
            let push_off = at(i, Need::Push((4 * push_dwords) as u32))?;
            push_va = t.general_va + u64::from(push_off);
            let mut b = Bytes::<{ 4 * MAX_PUSH_DWORDS }>::new();
            for f in &q.push[..push_dwords] {
                fits(b.put(&f.to_le_bytes()))?;
            }
            out.write(Region::General, push_off, b.bytes())?;
        }
        let data_off = at(i, Need::StateData(data_bytes))?;
        let data = pipeline
            .setup
            .state_data(|load| match load {
                Load::TextureDescriptor { .. } => t.texture_va,
                Load::Push { from } => push_va + 4 * u64::from(from),
                Load::Constants => constants_va.unwrap_or(0),
            })
            .map_err(|_| EmitError::Pipeline)?;
        let mut b = Bytes::<{ 4 * MAX_STATE_DWORDS }>::new();
        fits(b.words(&data))?;
        out.write(Region::Pds, data_off, b.bytes())?;
        let state = pipeline.pds_state(data_off);
        if i == 0 {
            // The first draw: the captured 26-word block with the pipeline's pass type and PDS
            // words, the VDM state naming the vertex data copy, the index list.
            let ppp_off = at(0, Need::Ppp26)?;
            let mut block = [0u8; PPP26_BYTES];
            block.copy_from_slice(t.ppp26);
            let ispa = u32::from_le_bytes([block[8], block[9], block[10], block[11]]);
            put_word(&mut block, PPP26_ISPA, ppp::with_pass_type(ispa, pipeline.pass_type));
            for (k, w) in state.words().iter().enumerate() {
                put_word(&mut block, PPP26_PDS + k, *w);
            }
            out.write(Region::General, ppp_off, &block)?;
            out.record(&vdm::ppp_state(26, t.general_va + u64::from(ppp_off)))?;
            let mut vdm_state = t.vdm_state_words;
            vdm_state[1] = vs_off & !0xf;
            out.record(&vdm_state)?;
            let (words, len) = vdm::index_list(VERTS_PER_QUAD as u32, None, t.topology);
            out.record(&words[..len])?;
        } else {
            let ppp_off = at(i, Need::Ppp)?;
            let mut block = Bytes::<28>::new();
            fits(block.words(&state.block()))?;
            out.write(Region::General, ppp_off, block.bytes())?;
            out.record(&vdm::ppp_state(7, t.general_va + u64::from(ppp_off)))?;
            out.record(&[vdm::vdm_state_bare(t.cam_size)])?;
            let first = (i * VERTS_PER_QUAD) as u32;
            let (words, len) = vdm::index_list(VERTS_PER_QUAD as u32, Some(first), t.topology);
            out.record(&words[..len])?;
        }
    }
    out.record(&vdm::ppp_state(t.terminate_ppp.1, t.terminate_ppp.0))?;
    out.record(&[vdm::TERMINATE])?;
    debug_assert_eq!((out.stream_at - stream_offset) as usize, stream_bytes(n));
    Ok(Emitted {
        vertex_offset,
        stream_offset,
        vdm_ctrl_stream_base: t.stream_va + u64::from(stream_offset),
        draws: n as u32,
    })
}

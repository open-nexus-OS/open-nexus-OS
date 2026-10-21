// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Dynamic geometry (TASK-0215 5b, RFC-0101 §8d), drawn with library pipelines (5c-2 P4d,
//! RFC-0101 §7b): textured, tinted quads. What a pass needs beyond the bundle, learned from the
//! captures of 2026-10-14 (`docs/board/measurements/2026-10-13-gpu-render-queue/README.md`,
//! "5b"): a vertex buffer of 6 N vertices named by a copy of the vertex PDS data; per draw its
//! push constants (as many as its pipeline's state load reads) and the state-load data naming
//! them and its texture's descriptor; a PPP block naming its pipeline's setup programs — the
//! first draw's the captured 26-word block with its pass type and PDS words, the others' seven
//! words, ten with the ISP words where the pass type changes (5c-3 P1: a pass of mixed
//! pipelines and textures, [`emit_draws`]); and the geometry control stream: the captured PDS
//! state, the PPP update, the VDM state naming the vertex data copy, an index list, then three
//! records per further draw, the terminate block, `TERMINATE`. **A pass lands in memory no pass
//! in flight reads** (the open driver allocates afresh each frame): the GPU's caches kept lines
//! of what a pass read when the next one rewrote them (H10, 2026-10-14) — the pipelines' setup
//! programs, placed once and never rewritten, are read by every pass; the frame's pass memory is
//! [`Rings`], a pass's lines taken until the timeline reached it. The emitter is pure and
//! allocates nothing: a [`Placer`] decides where each block goes — the device's rings or bump
//! tails, the goldens' captured offsets — and each block is made in place and handed to a
//! [`Sink`] — the device writes it into the bound buffer, the goldens record it.

use alloc::vec::Vec;

mod emit;
mod rings;

pub use emit::{emit, emit_draws, quad_vertices, stream_bytes, Draw, EmitError, Emitted};
pub use rings::{Arena, RingError, Rings, MAX_IN_FLIGHT};

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
    /// radius, the whole texture as the surface's rectangle.
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
        push[12] = 1.0;
        push[13] = 1.0;
        Self { x, y, w, h, push }
    }

    /// A part (`px`, `py`, `pw`, `ph`) of the layer at (`x`, `y`, `w`, `h`) — a rounded layer's
    /// corner square (the compositor draws the rest of the layer unrounded, TASK-0215 5c-3 P4b):
    /// the quad covers the part, its push constants name the whole layer's rounded rectangle, the
    /// part's `uv` mapping to its offset from the layer's centre.
    pub const fn part(
        (x, y, w, h): (f32, f32, f32, f32),
        (px, py, pw, ph): (f32, f32, f32, f32),
        tint: [f32; 4],
        radius: f32,
    ) -> Self {
        let mut q = Self::layer(px, py, pw, ph, tint, radius);
        q.push[6] = px - (x + w * 0.5);
        q.push[7] = py - (y + h * 0.5);
        q.push[8] = w * 0.5 - radius;
        q.push[9] = h * 0.5 - radius;
        q
    }

    /// The layer showing the rectangle `texture` = (scale x, scale y, offset x, offset y) of its
    /// texture, in its `uv` (an atlas variant reads it; others ignore it).
    pub const fn with_texture(mut self, texture: [f32; 4]) -> Self {
        self.push[12] = texture[0];
        self.push[13] = texture[1];
        self.push[14] = texture[2];
        self.push[15] = texture[3];
        self
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

    /// The part (`px`, `py`, `pw`, `ph`) of this quad (a shadow's, TASK-0335 P2: the pieces
    /// around an opaque layer): the quad covers the part, its push constants keep the whole
    /// shape and name the part's offset from the quad's centre.
    pub const fn part_of(&self, (px, py, pw, ph): (f32, f32, f32, f32)) -> Self {
        let mut q = *self;
        q.x = px;
        q.y = py;
        q.w = pw;
        q.h = ph;
        q.push[4] = pw;
        q.push[5] = ph;
        q.push[6] = px - (self.x + self.w * 0.5);
        q.push[7] = py - (self.y + self.h * 0.5);
        q
    }

    /// A filled rounded rectangle at (`x`, `y`, `w`, `h`) with a vertical gradient
    /// (`resources/gpu/shaders/gradient.wgsl`, its `Gradient` block): the straight colours at the
    /// top and at the bottom, the size and minus half of it, half the size less the corner radius
    /// and the radius.
    pub const fn gradient(
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        top: [f32; 4],
        bottom: [f32; 4],
        radius: f32,
    ) -> Self {
        let (hw, hh) = (w * 0.5, h * 0.5);
        let mut push = [0.0; 16];
        let mut i = 0;
        while i < 4 {
            push[i] = top[i];
            push[4 + i] = bottom[i];
            i += 1;
        }
        push[8] = w;
        push[9] = h;
        push[10] = -hw;
        push[11] = -hh;
        push[12] = hw - radius;
        push[13] = hh - radius;
        push[14] = radius;
        Self { x, y, w, h, push }
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
pub(crate) const DESCRIPTOR_BYTES: u32 = 4 * super::texstate::DESCRIPTOR_DWORDS as u32;

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

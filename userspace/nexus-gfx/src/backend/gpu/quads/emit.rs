// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The emitter (TASK-0215 5b, 5c-3 P1): a pass's blocks made in place and handed to the sink —
//! the vertices, the vertex PDS data naming them, per draw its push constants, its state-load
//! data (naming its push constants, its texture's descriptor, its pipeline's constant block) and
//! its PPP block (the first draw the captured 26-word block, every later one a seven-word block
//! of its pipeline's setup programs — ten words with the ISP words when its pass type differs
//! from the draw before), the control stream's records. A pass may mix pipelines and textures:
//! a frame draws its wallpaper opaque and its layers translucent, each with its own texture.

use super::super::csb::{pds, ppp, vdm};
use super::super::pipelines::Placed;
use super::{Need, Placer, Quad, Region, Sink, Template};
use crate::shader::rogue::setup::{Load, MAX_STATE_DWORDS};

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

/// One draw of a pass: its pipeline, its quad (in plane pixels, with its push constants), the
/// texture descriptor its state load names.
#[derive(Clone, Copy, Debug)]
pub struct Draw<'p> {
    pub pipeline: &'p Placed,
    pub quad: Quad,
    pub texture_va: u64,
}

/// Emits the pass for `quads` drawn with `pipeline` and the template's texture: `place` decides
/// where each block goes, `sink` receives it as it is made.
pub fn emit(
    t: &Template<'_>,
    pipeline: &Placed,
    quads: &[Quad],
    place: &mut dyn Placer,
    sink: &mut dyn Sink,
) -> Result<Emitted, EmitError> {
    let draw = |i: usize| Draw { pipeline, quad: quads[i], texture_va: t.texture_va };
    emit_draws(t, quads.len(), &draw, place, sink)
}

/// Emits the pass of `n` draws, draw `i` being `draw(i)` — each its own pipeline and texture.
/// The first draw's pass type is the 26-word block's; a later draw whose pass type differs from
/// the draw before it carries the ISP words in its block.
pub fn emit_draws<'p>(
    t: &Template<'_>,
    n: usize,
    draw: &dyn Fn(usize) -> Draw<'p>,
    place: &mut dyn Placer,
    sink: &mut dyn Sink,
) -> Result<Emitted, EmitError> {
    if n == 0 {
        return Err(EmitError::NoQuads);
    }
    if t.vs_pds_data.len() != VS_PDS_DATA_BYTES
        || t.vs_patched.iter().any(|&p| p + 8 > VS_PDS_DATA_BYTES)
        || t.ppp26.len() != PPP26_BYTES
    {
        return Err(EmitError::Template);
    }
    let mut at = |i: usize, need| place.place(i, need).ok_or(EmitError::Space);
    // The pass's own: the stream, the vertices, the vertex PDS data naming them.
    let stream_offset = at(0, Need::Stream(stream_bytes(n) as u32))?;
    let mut out = Out { sink, stream_at: stream_offset };
    let vertex_offset = at(0, Need::Vertices((n * QUAD_BYTES) as u32))?;
    for i in 0..n {
        let mut v = Bytes::<QUAD_BYTES>::new();
        for f in quad_vertices(&draw(i).quad).iter().flatten() {
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
    // The captured ISP words (control, front A, front B) a draw re-states with its pass type.
    let word = |k: usize| {
        let b = &t.ppp26[k * 4..k * 4 + 4];
        u32::from_le_bytes([b[0], b[1], b[2], b[3]])
    };
    let isp = [word(PPP26_ISPA - 1), word(PPP26_ISPA), word(PPP26_ISPA + 1)];
    let mut pass_type = None;
    for i in 0..n {
        let d = draw(i);
        let (pipeline, q) = (d.pipeline, &d.quad);
        // The push constants the pipeline's state load reads: their first dwords of the quad's.
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
        // The constant block where the pipeline's load placed it (a block no placement names is
        // a pipeline the table did not place).
        let constants_va = pipeline.constants_va();
        if pipeline.setup.dmas.iter().any(|d| d.load == Load::Constants) && constants_va.is_none() {
            return Err(EmitError::Pipeline);
        }
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
        let data_bytes = (4 * pipeline.setup.state_data_dwords()) as u32;
        let data_off = at(i, Need::StateData(data_bytes))?;
        let data = pipeline
            .setup
            .state_data(|load| match load {
                Load::TextureDescriptor { .. } => d.texture_va,
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
            put_word(&mut block, PPP26_ISPA, ppp::with_pass_type(isp[1], pipeline.pass_type));
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
            // A later draw: its pipeline's PDS state — and the ISP words with its pass type when
            // that differs from the draw before.
            let ppp_off = at(i, Need::Ppp)?;
            let mut block = Bytes::<40>::new();
            let words = if pass_type == Some(pipeline.pass_type) {
                fits(block.words(&state.block()))?;
                7
            } else {
                let ispa = ppp::with_pass_type(isp[1], pipeline.pass_type);
                fits(block.words(&state.block_with_isp([isp[0], ispa, isp[2]])))?;
                10
            };
            out.write(Region::General, ppp_off, block.bytes())?;
            out.record(&vdm::ppp_state(words, t.general_va + u64::from(ppp_off)))?;
            out.record(&[vdm::vdm_state_bare(t.cam_size)])?;
            let first = (i * VERTS_PER_QUAD) as u32;
            let (words, len) = vdm::index_list(VERTS_PER_QUAD as u32, Some(first), t.topology);
            out.record(&words[..len])?;
        }
        pass_type = Some(pipeline.pass_type);
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

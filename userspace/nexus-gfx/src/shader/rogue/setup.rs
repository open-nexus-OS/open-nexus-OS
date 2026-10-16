// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! A fragment pipeline's setup programs (TASK-0215 5c-2 P4b, RFC-0101 §7b): the three programs
//! the setup unit runs before each tile's fragment work — the kick (names the program and its
//! temporaries), the varying iteration (W and each input into the coefficient store) and the
//! state load (a texture's descriptor, the push constants and the pipeline's constant block
//! DMA'd into shared registers) — generated from a compiled program and its interface, carried
//! in the pipeline library as its setup sections and state (the constant block rides in the
//! state). The device decodes them strictly: every instruction must be the form the generator
//! writes, every load inside the interface and the registers it declares.
//!
//! Facts, measured on the render bundle (5c-2 P1b) and in the open driver's constants:
//! - the kick's data segment is fixed at eight dwords — the hardware runs its code at +0x20;
//! - the iteration's data is its DOUTI words, its code at the next 16 bytes; the draw state
//!   counts its size in units of four dwords, the coefficient registers in units of sixteen;
//! - the state load's code is fixed and its data written per draw: a DMA source pair per load,
//!   then the DMA controls, counted in units of four dwords; shared registers in units of
//!   sixteen;
//! - a texture's descriptor block is twenty dwords from its texture state, the sampler state at
//!   its ninth dword. The open driver's state load also carries its descriptor-set plumbing (a
//!   descriptor at sh28, a constant at sh0); our programs read neither, so ours does not load
//!   them.

use alloc::vec::Vec;

use super::codegen::{Layout, Program, MAX_CONSTANTS};
use super::pds::{DmaCtl, DmaSrc, DoutDst, Douti, Doutu, PdsInstr, PdsProgram};
use super::reference::{PASS_OPAQUE, PASS_TRANSLUCENT};
use super::RogueError;
use crate::backend::gpu::csb::ppp;
use crate::shader::ir::{Interface, Interp, Resource, Stage, MAX_INPUTS, MAX_PUSH_BYTES};

/// The kick's data segment (the open driver's fixed pixel-shader data size).
pub const KICK_DATA_DWORDS: usize = 8;
/// A texture's descriptor block: texture state, then sampler state at dword 8.
pub const TEXTURE_DESCRIPTOR_DWORDS: u16 = 20;
const SAMPLER_IN_DESCRIPTOR: u16 = 8;
/// A DMA control's destination: the shared registers.
const DMA_TO_SHARED: u8 = 1;
/// Interpolation across the primitive (the open driver's "Gouraud"): every iterated input.
const SHADE_GOURAUD: u8 = 3;
/// The setup unit's temporaries (units of four), as the open driver gives a fragment pipeline.
const PDS_TEMP_UNITS: u8 = 1;
const MAX_DMAS: usize = 8;
/// The most shared registers a state load may fill (the draw state's 9-bit field, sixteens).
const MAX_SHARED: usize = 511 * 16;

const KIND_KICK: u8 = 1;
const KIND_ITERATE: u8 = 2;
const KIND_STATE: u8 = 3;
/// The state's fixed part; the constant block's dwords follow it.
const STATE_BYTES: usize = 8;

/// Where the vertex stage leaves what the iteration reads (dwords into its output): W, then
/// the first input, the others after it in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Varyings {
    pub w: u8,
    pub first: u8,
}

impl Varyings {
    /// The render bundle's vertex program (captured, until the vertex pipeline is ours).
    pub const BUNDLE: Self = Self { w: 0, first: 2 };
}

/// What a state-load DMA reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Load {
    /// The descriptor block of the interface's resource at this index (a texture).
    TextureDescriptor { resource: u8 },
    /// The push constants from this dword on.
    Push { from: u8 },
    /// The pipeline's constant block, where the device placed it with the setup.
    Constants,
}

/// One state-load DMA: what, into which shared register, how many dwords.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dma {
    pub load: Load,
    pub dest: u16,
    pub dwords: u16,
}

/// What the draw state says beside the setup programs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct State {
    /// The program's temporaries, units of four (the kick names them).
    pub temps: u8,
    pub pass_type: u8,
    /// Coefficient registers, units of sixteen.
    pub usc_varying: u8,
    /// Shared registers, units of sixteen.
    pub usc_shared: u16,
    /// The setup unit's temporaries, units of four.
    pub pds_temps: u8,
}

/// A fragment pipeline's setup: the kick (its program offset zero until placed), the
/// iteration, the state load's code and its loads, the state, the constant block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Setup {
    pub kick: PdsProgram,
    pub iterate: PdsProgram,
    pub state_code: Vec<PdsInstr>,
    pub dmas: Vec<Dma>,
    pub state: State,
    /// The values the state load DMAs from the constant block (empty: no block, no load).
    pub constants: Vec<u32>,
}

fn refuse(what: &'static str) -> RogueError {
    RogueError::Malformed { at: 0, what }
}

fn dout(dst: DoutDst, src0: u8, src1: u8, end: bool) -> PdsInstr {
    PdsInstr::Dout { cc: false, end, src1, src0, dst }
}

fn kick_code() -> PdsInstr {
    dout(DoutDst::U, 0, 0, true)
}

fn iterate_code(issue: usize, issues: usize) -> PdsInstr {
    dout(DoutDst::I, issue as u8, 0, issue + 1 == issues)
}

fn state_code(dma: usize, dmas: usize) -> PdsInstr {
    dout(DoutDst::D, dma as u8, (2 * dmas + dma) as u8, dma + 1 == dmas)
}

fn qword_dwords(q: u64) -> [u32; 2] {
    [q as u32, (q >> 32) as u32]
}

fn units(n: usize, of: usize) -> usize {
    n.div_ceil(of)
}

/// The setup of a fragment `program` compiled from `interface` over `layout`, its inputs where
/// `varyings` puts them.
pub fn fragment(
    program: &Program,
    interface: &Interface,
    layout: Layout,
    varyings: Varyings,
) -> Result<Setup, RogueError> {
    if interface.stage != Stage::Fragment {
        return Err(refuse("a stage other than fragment"));
    }
    let k = Doutu { exe_off: 0, temps: program.temps, sample_rate: 0, dual_phase: false };
    let mut kick_data = qword_dwords(k.encode()?).to_vec();
    kick_data.resize(KICK_DATA_DWORDS, 0);
    let kick = PdsProgram { data: kick_data, code: alloc::vec![kick_code()] };

    // W, then each input: four coefficient registers a component.
    let issues = interface.inputs.len() + 1;
    let mut data = Vec::with_capacity(2 * issues);
    let w = Douti {
        shademodel: SHADE_GOURAUD,
        f32_offset: varyings.w,
        f16_offset: varyings.w,
        dest: (layout.w_coeff / 4) as u8,
        last: issues == 1,
        ..Douti::default()
    };
    data.extend_from_slice(&qword_dwords(w.encode()?));
    let (mut coeff, mut offset) = (usize::from(layout.inputs_coeff), usize::from(varyings.first));
    let mut coeff_end = usize::from(layout.w_coeff) + 4;
    for (i, inp) in interface.inputs.iter().enumerate() {
        let perspective = match inp.interp {
            Interp::Perspective => true,
            Interp::Linear => false,
            Interp::Flat => return Err(refuse("a flat input")),
        };
        let lanes = usize::from(inp.ty.lanes);
        let d = Douti {
            shademodel: SHADE_GOURAUD,
            size: inp.ty.lanes - 1,
            perspective,
            f32_offset: u8::try_from(offset).map_err(|_| refuse("a varying offset"))?,
            f16_offset: u8::try_from(offset).map_err(|_| refuse("a varying offset"))?,
            dest: u8::try_from(coeff / 4).map_err(|_| refuse("a coefficient register"))?,
            last: i + 2 == issues,
            ..Douti::default()
        };
        data.extend_from_slice(&qword_dwords(d.encode()?));
        coeff += 4 * lanes;
        offset += lanes;
        coeff_end = coeff_end.max(coeff);
    }
    data.resize(4 * units(data.len(), 4), 0);
    let iterate = PdsProgram { data, code: (0..issues).map(|i| iterate_code(i, issues)).collect() };

    // A texture's descriptor block brings its sampler state along; then the push constants.
    let mut dmas = Vec::new();
    let textures = interface.resources.iter().filter(|r| matches!(r, Resource::Texture { .. }));
    if textures.count() > 1 {
        return Err(refuse("more than one texture"));
    }
    for (i, r) in interface.resources.iter().enumerate() {
        match r {
            Resource::Texture { .. } => {
                if layout.sampler_state != layout.texture_state + SAMPLER_IN_DESCRIPTOR {
                    return Err(refuse("a sampler apart from its texture's descriptor"));
                }
                let load = Load::TextureDescriptor { resource: i as u8 };
                let dwords = TEXTURE_DESCRIPTOR_DWORDS;
                dmas.push(Dma { load, dest: layout.texture_state, dwords });
            }
            Resource::Sampler { .. } => {}
        }
    }
    if interface.push_bytes > 0 {
        let dwords = (interface.push_bytes / 4) as u16;
        dmas.push(Dma { load: Load::Push { from: 0 }, dest: layout.push, dwords });
    }
    if program.constants.len() > MAX_CONSTANTS {
        return Err(refuse("more constants than a pipeline's block holds"));
    }
    if !program.constants.is_empty() {
        let (dest, dwords) = (layout.constants(interface.push_bytes), program.constants.len());
        dmas.push(Dma { load: Load::Constants, dest, dwords: dwords as u16 });
    }
    if dmas.is_empty() || dmas.len() > MAX_DMAS {
        return Err(refuse("a state load of no or too many loads"));
    }
    let shared_end = dmas.iter().map(|d| usize::from(d.dest + d.dwords)).max().unwrap_or(0);
    let state = State {
        temps: program.temps,
        pass_type: program.pass_type,
        usc_varying: u8::try_from(units(coeff_end, 16)).map_err(|_| refuse("coefficients"))?,
        usc_shared: units(shared_end, 16) as u16,
        pds_temps: PDS_TEMP_UNITS,
    };
    let n = dmas.len();
    Ok(Setup {
        kick,
        iterate,
        state_code: (0..n).map(|i| state_code(i, n)).collect(),
        dmas,
        state,
        constants: program.constants.clone(),
    })
}

impl Setup {
    /// The state load's data dwords for a draw (sources, then controls, to whole units).
    pub fn state_data_dwords(&self) -> usize {
        4 * units(3 * self.dmas.len(), 4)
    }

    /// The draw state's `STATE_PDS_SIZEINFO1` and `…2`.
    pub fn sizeinfo(&self) -> (u32, u32) {
        let s = self.state;
        (
            ppp::sizeinfo1(
                (self.state_data_dwords() / 4) as u32,
                0,
                (self.iterate.data.len() / 4) as u32,
                u32::from(s.usc_varying),
                u32::from(s.pds_temps),
            ),
            ppp::sizeinfo2(u32::from(s.usc_shared), true, 0),
        )
    }

    /// The kick with the program placed at `exe_off` (code heap bytes) — the one address a
    /// pipeline's setup receives when it is placed.
    pub fn kick_at(&self, exe_off: u32) -> Result<PdsProgram, RogueError> {
        let k = Doutu { exe_off, temps: self.state.temps, sample_rate: 0, dual_phase: false };
        let mut kick = self.kick.clone();
        kick.data[..2].copy_from_slice(&qword_dwords(k.encode()?));
        Ok(kick)
    }

    /// The state load's data for one draw: each load's source from `address_of`, then the
    /// controls, the last ending the program.
    pub fn state_data(&self, address_of: impl Fn(Load) -> u64) -> Result<Vec<u32>, RogueError> {
        let n = self.dmas.len();
        let mut d = Vec::with_capacity(self.state_data_dwords());
        for dma in &self.dmas {
            d.extend_from_slice(&qword_dwords(
                DmaSrc { sbase: address_of(dma.load), doffset: 0 }.encode()?,
            ));
        }
        for (i, dma) in self.dmas.iter().enumerate() {
            let c = DmaCtl {
                last: i + 1 == n,
                wordsize: 0,
                dest: DMA_TO_SHARED,
                cmode: 0,
                a0: dma.dest,
                repeat: false,
                bsize: dma.dwords,
            };
            d.push(c.encode()?);
        }
        d.resize(self.state_data_dwords(), 0);
        Ok(d)
    }

    /// The constant block as the device places it.
    pub fn constant_bytes(&self) -> Vec<u8> {
        self.constants.iter().flat_map(|c| c.to_le_bytes()).collect()
    }

    /// The library sections: the kick, the iteration and the state load (in that order), then
    /// the state. A setup section: kind, data dwords, code dwords, load count (u8 each), the
    /// data, the code, four bytes per load (kind, its argument, register, dwords). The state:
    /// temporaries, pass type, coefficient units, setup temporaries (u8 each), shared units
    /// (u16), the constant count (u8), a zero byte, then the constants.
    pub fn sections(&self) -> Result<(Vec<Vec<u8>>, Vec<u8>), RogueError> {
        let section = |kind: u8, p: &PdsProgram, dmas: &[Dma]| -> Result<Vec<u8>, RogueError> {
            let mut b = alloc::vec![kind, p.data.len() as u8, p.code.len() as u8, dmas.len() as u8];
            for d in &p.data {
                b.extend_from_slice(&d.to_le_bytes());
            }
            for i in &p.code {
                b.extend_from_slice(&i.encode()?.to_le_bytes());
            }
            for d in dmas {
                let (kind, arg) = match d.load {
                    Load::TextureDescriptor { resource } => (1, resource),
                    Load::Push { from } => (2, from),
                    Load::Constants => (3, 0),
                };
                b.extend_from_slice(&[kind, arg, d.dest as u8, d.dwords as u8]);
            }
            Ok(b)
        };
        let state_load = PdsProgram { data: Vec::new(), code: self.state_code.clone() };
        let setup = alloc::vec![
            section(KIND_KICK, &self.kick, &[])?,
            section(KIND_ITERATE, &self.iterate, &[])?,
            section(KIND_STATE, &state_load, &self.dmas)?,
        ];
        let s = self.state;
        let count = u8::try_from(self.constants.len()).map_err(|_| refuse("the constants"))?;
        let mut state = alloc::vec![s.temps, s.pass_type, s.usc_varying, s.pds_temps];
        state.extend_from_slice(&s.usc_shared.to_le_bytes());
        state.extend_from_slice(&[count, 0]);
        state.extend_from_slice(&self.constant_bytes());
        Ok((setup, state))
    }

    /// The setup a library entry carries, decoded for `interface` and checked instruction by
    /// instruction against the forms [`fragment`] writes.
    pub fn from_sections(
        setup: &[&[u8]],
        state: &[u8],
        interface: &Interface,
    ) -> Result<Self, RogueError> {
        let [kick, iterate, load] = setup else {
            return Err(refuse("other than a kick, an iteration and a state load"));
        };
        let count = usize::from(*state.get(6).ok_or(refuse("the state"))?);
        if count > MAX_CONSTANTS || state.len() != STATE_BYTES + 4 * count || state[7] != 0 {
            return Err(refuse("the state"));
        }
        let constants: Vec<u32> = state[STATE_BYTES..]
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        let state = State {
            temps: state[0],
            pass_type: state[1],
            usc_varying: state[2],
            pds_temps: state[3],
            usc_shared: u16::from_le_bytes([state[4], state[5]]),
        };
        if state.temps == 0 || state.temps > 63 || state.pds_temps == 0 || state.pds_temps > 15 {
            return Err(refuse("the temporaries"));
        }
        if ![PASS_OPAQUE, PASS_TRANSLUCENT].contains(&state.pass_type) || state.usc_varying > 63 {
            return Err(refuse("the pass type or the coefficients"));
        }
        if usize::from(state.usc_shared) * 16 > MAX_SHARED {
            return Err(refuse("the shared registers"));
        }

        let Section { data: kd, code: kc, .. } =
            split(kick, KIND_KICK, KICK_DATA_DWORDS..=KICK_DATA_DWORDS, 1..=1)?;
        let k = Doutu::decode(u64::from(kd[0]) | (u64::from(kd[1]) << 32));
        let blank = Doutu { exe_off: 0, temps: state.temps, sample_rate: 0, dual_phase: false };
        if k != blank || kd[2..].iter().any(|d| *d != 0) || kc != [kick_code()] {
            return Err(refuse("the kick"));
        }

        let max = 4 * units(2 * (MAX_INPUTS + 1), 4);
        let Section { data: id, code: ic, .. } =
            split(iterate, KIND_ITERATE, 4..=max, 1..=MAX_INPUTS + 1)?;
        let issues = ic.len();
        if id.len() % 4 != 0 || id.len() < 2 * issues || id[2 * issues..].iter().any(|d| *d != 0) {
            return Err(refuse("the iteration's data"));
        }
        for (i, c) in ic.iter().enumerate() {
            let d = Douti::decode(u64::from(id[2 * i]) | (u64::from(id[2 * i + 1]) << 32), 8 * i)?;
            let end = usize::from(d.dest) * 4 + 4 * (usize::from(d.size) + 1);
            if *c != iterate_code(i, issues) || d.last != (i + 1 == issues) {
                return Err(refuse("the iteration's code"));
            }
            if end > usize::from(state.usc_varying) * 16 {
                return Err(refuse("an iteration beyond its coefficients"));
            }
        }

        let Section { data: sd, code: sc, loads: raw } =
            split(load, KIND_STATE, 0..=0, 1..=MAX_DMAS)?;
        let n = sc.len();
        if !sd.is_empty() || raw.len() != 4 * n {
            return Err(refuse("the state load's loads"));
        }
        let mut dmas: Vec<Dma> = Vec::with_capacity(n);
        for (i, c) in sc.iter().enumerate() {
            let [kind, arg, dest, dwords] =
                [raw[4 * i], raw[4 * i + 1], raw[4 * i + 2], raw[4 * i + 3]];
            let load = match kind {
                1 if matches!(
                    interface.resources.get(usize::from(arg)),
                    Some(Resource::Texture { .. })
                ) && u16::from(dwords) == TEXTURE_DESCRIPTOR_DWORDS =>
                {
                    Load::TextureDescriptor { resource: arg }
                }
                2 if (u32::from(arg) + u32::from(dwords)) * 4
                    <= interface.push_bytes.min(MAX_PUSH_BYTES) =>
                {
                    Load::Push { from: arg }
                }
                // The whole block, once.
                3 if arg == 0
                    && usize::from(dwords) == constants.len()
                    && !dmas.iter().any(|d| d.load == Load::Constants) =>
                {
                    Load::Constants
                }
                _ => return Err(refuse("a load outside the interface")),
            };
            let (dest, dwords) = (u16::from(dest), u16::from(dwords));
            if *c != state_code(i, n) || dwords == 0 || dest + dwords > state.usc_shared * 16 {
                return Err(refuse("a load beyond its registers"));
            }
            dmas.push(Dma { load, dest, dwords });
        }
        if !constants.is_empty() && !dmas.iter().any(|d| d.load == Load::Constants) {
            return Err(refuse("a constant block no load reads"));
        }
        Ok(Self {
            kick: PdsProgram { data: kd, code: kc },
            iterate: PdsProgram { data: id, code: ic },
            state_code: sc,
            dmas,
            state,
            constants,
        })
    }
}

/// A setup section split: its data, its code (decoded), its trailing load bytes.
struct Section<'a> {
    data: Vec<u32>,
    code: Vec<PdsInstr>,
    loads: &'a [u8],
}

/// A setup section's parts, each count in its range.
fn split(
    b: &[u8],
    kind: u8,
    data: core::ops::RangeInclusive<usize>,
    code: core::ops::RangeInclusive<usize>,
) -> Result<Section<'_>, RogueError> {
    let head = b.get(..4).ok_or(RogueError::Truncated { at: 0 })?;
    let (nd, nc, nl) = (usize::from(head[1]), usize::from(head[2]), usize::from(head[3]));
    if head[0] != kind || !data.contains(&nd) || !code.contains(&nc) {
        return Err(refuse("a setup section's kind or sizes"));
    }
    let words = |from: usize, n: usize| -> Result<Vec<u32>, RogueError> {
        let bytes = b.get(from..from + 4 * n).ok_or(RogueError::Truncated { at: from })?;
        Ok(bytes.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect())
    };
    let d = words(4, nd)?;
    let c = words(4 + 4 * nd, nc)?
        .iter()
        .enumerate()
        .map(|(i, w)| PdsInstr::decode(*w, 4 * i))
        .collect::<Result<Vec<_>, _>>()?;
    let rest = &b[(4 + 4 * (nd + nc)).min(b.len())..];
    if rest.len() != 4 * nl {
        return Err(refuse("a setup section's length"));
    }
    Ok(Section { data: d, code: c, loads: rest })
}

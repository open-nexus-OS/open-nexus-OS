// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The pipeline library (TASK-0215 5c-2 P4a, RFC-0101 §7b, ADR-0072 §6): one archive per
//! code target holding the pipelines a client draws with — each entry a pipeline's interface, its
//! program, its setup programs and its state, built offline by `tools/nxgpu`. The device parses
//! it before a byte is copied: every size and count is bounded; an archive for another code
//! target or of another format version is refused whole; an entry whose digest does not match
//! its bytes, or that needs a feature the device lacks, is refused alone and the rest accepted.
//! The setup programs and the state are the code target's encoding (`rogue::setup` for the
//! board GPU); this module carries them as bounded bytes.
//! OWNERS: @gpu @runtime
//! STATUS: Experimental (format v1)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `tests/shader_library.rs` — a built library parses back; the rejects of
//!   RFC-0101 §7b; every truncation and every flipped byte of a library parses without a panic.
//!
//! Format v1, little-endian:
//! - the header (32 bytes): `NXGP`, version u16, entry count u16, code target u64, total size
//!   u32, twelve zero bytes;
//! - the entry table: per entry its offset and length (u32 each), in order, four-byte aligned,
//!   each inside the archive, none overlapping the table or another;
//! - an entry: id u32 (never zero, unique), features u32, digest u64 (FNV-1a 64 of the entry with
//!   its digest field as zeros), budget u16 (instruction groups), section count u16, then the
//!   sections — kind u16, two zero bytes, length u32, the bytes, zero padding to four bytes — and
//!   nothing after the last;
//! - the sections: one name, one interface, one program, one state, up to four setup programs.

use alloc::vec::Vec;

use crate::shader::ir::{
    Input, Interface, Interp, Output, Resource, Scalar, Stage, TexDim, Type, MAX_INPUTS,
    MAX_OUTPUTS, MAX_PUSH_BYTES, MAX_RESOURCES,
};

pub const MAGIC: [u8; 4] = *b"NXGP";
pub const VERSION: u16 = 1;
pub const HEADER_BYTES: usize = 32;
const ENTRY_HEADER_BYTES: usize = 20;
const SECTION_HEADER_BYTES: usize = 8;
pub const MAX_ENTRIES: usize = 64;
pub const MAX_ENTRY_BYTES: usize = 64 * 1024;
pub const MAX_LIBRARY_BYTES: usize = 1024 * 1024;
pub const MAX_SECTIONS: usize = 8;
pub const MAX_SETUP: usize = 4;
pub const MAX_NAME: usize = 32;

/// Section kinds.
pub mod section {
    pub const NAME: u16 = 1;
    pub const INTERFACE: u16 = 2;
    pub const PROGRAM: u16 = 3;
    pub const SETUP: u16 = 4;
    pub const STATE: u16 = 5;
}

/// What an entry may need of the device; bits this version does not name are features no
/// device has.
pub mod feature {
    /// Iterate-and-sample in one control word (the board GPU's `itrsmp`, 5c-2 cycle 14).
    pub const FUSED_ITERATE_SAMPLE: u32 = 1 << 0;
    /// Every feature this version names.
    pub const KNOWN: u32 = FUSED_ITERATE_SAMPLE;
}

/// Why a library is refused whole.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LibraryError {
    /// Shorter than its header, table, an entry or a section says, or not its total size.
    Truncated,
    BadMagic,
    UnknownVersion(u16),
    /// Built for another device.
    WrongTarget {
        library: u64,
        device: u64,
    },
    /// More entries, or a larger entry or archive, than the format bounds — named.
    Oversize(&'static str),
    /// An entry or section breaks the format — named.
    Malformed(&'static str),
}

/// Why one entry is refused while the others are accepted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    DigestMismatch,
    /// Feature bits the device lacks.
    UnsupportedFeature {
        missing: u32,
    },
}

/// A pipeline as the library carries it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry<'a> {
    pub id: u32,
    pub features: u32,
    /// Instruction groups the program may take (ADR-0072 §7).
    pub budget: u16,
    pub name: &'a str,
    pub interface: Interface,
    pub program: &'a [u8],
    /// The code target's setup programs, in the order they were built.
    pub setup: Vec<&'a [u8]>,
    /// The code target's state words.
    pub state: &'a [u8],
}

/// A parsed library: what the device accepts and what it refuses, by id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Library<'a> {
    pub target: u64,
    pub accepted: Vec<Entry<'a>>,
    pub refused: Vec<(u32, Refusal)>,
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// An entry's digest: FNV-1a 64 of its bytes with the digest field (bytes 8–15) as zeros.
pub fn digest(entry: &[u8]) -> u64 {
    entry.iter().enumerate().fold(FNV_OFFSET, |h, (i, b)| {
        let b = if (8..16).contains(&i) { 0 } else { *b };
        (h ^ u64::from(b)).wrapping_mul(FNV_PRIME)
    })
}

/// The code target an archive of this format names in its header — what a client picks its
/// library by, against the target the device's capabilities name (RFC-0101 §7b). Only the
/// header is read: the device parses the rest.
pub fn header_target(bytes: &[u8]) -> Option<u64> {
    let h = bytes.get(..HEADER_BYTES)?;
    if h[..4] != MAGIC || u16::from_le_bytes([h[4], h[5]]) != VERSION {
        return None;
    }
    Some(u64::from_le_bytes([h[8], h[9], h[10], h[11], h[12], h[13], h[14], h[15]]))
}

fn u16_at(b: &[u8], at: usize) -> Result<u16, LibraryError> {
    match b.get(at..at + 2) {
        Some(w) => Ok(u16::from_le_bytes([w[0], w[1]])),
        None => Err(LibraryError::Truncated),
    }
}

fn u32_at(b: &[u8], at: usize) -> Result<u32, LibraryError> {
    match b.get(at..at + 4) {
        Some(w) => Ok(u32::from_le_bytes([w[0], w[1], w[2], w[3]])),
        None => Err(LibraryError::Truncated),
    }
}

fn u64_at(b: &[u8], at: usize) -> Result<u64, LibraryError> {
    Ok(u64::from(u32_at(b, at)?) | (u64::from(u32_at(b, at + 4)?) << 32))
}

const fn pad4(n: usize) -> usize {
    (n + 3) & !3
}

/// Parses `bytes` for a device of code target `target` with the feature bits `features`.
pub fn parse(bytes: &[u8], target: u64, features: u32) -> Result<Library<'_>, LibraryError> {
    if bytes.len() < HEADER_BYTES {
        return Err(LibraryError::Truncated);
    }
    if bytes.len() > MAX_LIBRARY_BYTES {
        return Err(LibraryError::Oversize("the archive"));
    }
    if bytes[..4] != MAGIC {
        return Err(LibraryError::BadMagic);
    }
    let version = u16_at(bytes, 4)?;
    if version != VERSION {
        return Err(LibraryError::UnknownVersion(version));
    }
    let count = usize::from(u16_at(bytes, 6)?);
    let library = u64_at(bytes, 8)?;
    if u32_at(bytes, 16)? as usize != bytes.len() {
        return Err(LibraryError::Truncated);
    }
    if bytes[20..HEADER_BYTES].iter().any(|b| *b != 0) {
        return Err(LibraryError::Malformed("reserved header bytes"));
    }
    if library != target {
        return Err(LibraryError::WrongTarget { library, device: target });
    }
    if count == 0 {
        return Err(LibraryError::Malformed("no entries"));
    }
    if count > MAX_ENTRIES {
        return Err(LibraryError::Oversize("the entry count"));
    }
    let table_end = HEADER_BYTES + count * 8;
    if bytes.len() < table_end {
        return Err(LibraryError::Truncated);
    }
    let mut out = Library { target, accepted: Vec::new(), refused: Vec::new() };
    let mut ids: Vec<u32> = Vec::with_capacity(count);
    let mut next_free = table_end;
    for i in 0..count {
        let off = u32_at(bytes, HEADER_BYTES + i * 8)? as usize;
        let len = u32_at(bytes, HEADER_BYTES + i * 8 + 4)? as usize;
        if len > MAX_ENTRY_BYTES {
            return Err(LibraryError::Oversize("an entry"));
        }
        if off % 4 != 0 || off < next_free {
            return Err(LibraryError::Malformed("an entry out of order or overlapping"));
        }
        let entry = bytes.get(off..off + len).ok_or(LibraryError::Truncated)?;
        next_free = off + len;
        let (id, feats) = (u32_at(entry, 0)?, u32_at(entry, 4)?);
        if id == 0 || ids.contains(&id) {
            return Err(LibraryError::Malformed("an id zero or twice"));
        }
        ids.push(id);
        // The digest before the sections: a corrupted entry is refused alone, not parsed.
        let missing = feats & !(features & feature::KNOWN);
        if u64_at(entry, 8)? != digest(entry) {
            out.refused.push((id, Refusal::DigestMismatch));
        } else if missing != 0 {
            out.refused.push((id, Refusal::UnsupportedFeature { missing }));
        } else {
            out.accepted.push(parse_entry(entry)?);
        }
    }
    Ok(out)
}

fn parse_entry(entry: &[u8]) -> Result<Entry<'_>, LibraryError> {
    let malformed = LibraryError::Malformed;
    let (id, features) = (u32_at(entry, 0)?, u32_at(entry, 4)?);
    let budget = u16_at(entry, 16)?;
    let sections = usize::from(u16_at(entry, 18)?);
    if sections > MAX_SECTIONS {
        return Err(LibraryError::Oversize("an entry's sections"));
    }
    let (mut name, mut interface, mut program, mut state) = (None, None, None, None);
    let mut setup = Vec::new();
    let mut at = ENTRY_HEADER_BYTES;
    for _ in 0..sections {
        let kind = u16_at(entry, at)?;
        if u16_at(entry, at + 2)? != 0 {
            return Err(malformed("reserved section bytes"));
        }
        let len = u32_at(entry, at + 4)? as usize;
        if len > entry.len() {
            return Err(LibraryError::Truncated);
        }
        let start = at + SECTION_HEADER_BYTES;
        let body = entry.get(start..start + len).ok_or(LibraryError::Truncated)?;
        let end = start + pad4(len);
        let padding = entry.get(start + len..end).ok_or(LibraryError::Truncated)?;
        if padding.iter().any(|b| *b != 0) {
            return Err(malformed("section padding"));
        }
        match kind {
            section::NAME => {
                once(&name)?;
                name = Some(parse_name(body)?);
            }
            section::INTERFACE => {
                once(&interface)?;
                interface = Some(decode_interface(body)?);
            }
            section::PROGRAM => {
                once(&program)?;
                if body.is_empty() {
                    return Err(malformed("an empty program"));
                }
                program = Some(body);
            }
            section::STATE => {
                once(&state)?;
                state = Some(body);
            }
            section::SETUP => {
                if setup.len() == MAX_SETUP {
                    return Err(LibraryError::Oversize("an entry's setup programs"));
                }
                setup.push(body);
            }
            _ => return Err(malformed("an unknown section")),
        }
        at = end;
    }
    if at != entry.len() {
        return Err(malformed("bytes after the last section"));
    }
    match (name, interface, program, state) {
        (Some(name), Some(interface), Some(program), Some(state)) => {
            Ok(Entry { id, features, budget, name, interface, program, setup, state })
        }
        _ => Err(malformed("a name, interface, program or state missing")),
    }
}

/// A section that may appear once.
fn once<T>(slot: &Option<T>) -> Result<(), LibraryError> {
    match slot {
        Some(_) => Err(LibraryError::Malformed("a section twice")),
        None => Ok(()),
    }
}

fn parse_name(b: &[u8]) -> Result<&str, LibraryError> {
    let ok =
        |c: &u8| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'-' | b'.' | b'_');
    if b.is_empty() || b.len() > MAX_NAME || !b.iter().all(ok) {
        return Err(LibraryError::Malformed("a name"));
    }
    core::str::from_utf8(b).map_err(|_| LibraryError::Malformed("a name"))
}

fn scalar_code(s: Scalar) -> u8 {
    match s {
        Scalar::F32 => 0,
        Scalar::F16 => 1,
        Scalar::I32 => 2,
        Scalar::U32 => 3,
        Scalar::Bool => 4,
    }
}

fn scalar_of(c: u8) -> Option<Scalar> {
    Some(match c {
        0 => Scalar::F32,
        1 => Scalar::F16,
        2 => Scalar::I32,
        3 => Scalar::U32,
        4 => Scalar::Bool,
        _ => return None,
    })
}

fn ty_of(scalar: u8, lanes: u8) -> Option<Type> {
    (1..=4).contains(&lanes).then_some(Type::new(scalar_of(scalar)?, lanes))
}

/// An interface's bytes: stage, input, output and resource counts (u8 each), push bytes (u16),
/// two zero bytes, then four bytes per input (location, scalar, lanes, interpolation), per
/// output (location, scalar, lanes, zero) and per resource (kind, binding, dimensions, zero).
pub fn encode_interface(i: &Interface) -> Vec<u8> {
    let stage = match i.stage {
        Stage::Vertex => 0,
        Stage::Fragment => 1,
        Stage::Compute => 2,
    };
    let mut b = Vec::with_capacity(8 + 4 * (i.inputs.len() + i.outputs.len() + i.resources.len()));
    b.extend_from_slice(&[
        stage,
        i.inputs.len() as u8,
        i.outputs.len() as u8,
        i.resources.len() as u8,
    ]);
    b.extend_from_slice(&(i.push_bytes as u16).to_le_bytes());
    b.extend_from_slice(&[0, 0]);
    for x in &i.inputs {
        let interp = match x.interp {
            Interp::Perspective => 0,
            Interp::Linear => 1,
            Interp::Flat => 2,
        };
        b.extend_from_slice(&[x.location, scalar_code(x.ty.scalar), x.ty.lanes, interp]);
    }
    for x in &i.outputs {
        b.extend_from_slice(&[x.location, scalar_code(x.ty.scalar), x.ty.lanes, 0]);
    }
    for r in &i.resources {
        b.extend_from_slice(&match *r {
            Resource::Texture { binding, dim } => [
                1,
                binding,
                match dim {
                    TexDim::D1 => 1,
                    TexDim::D2 => 2,
                    TexDim::D3 => 3,
                },
                0,
            ],
            Resource::Sampler { binding } => [2, binding, 0, 0],
        });
    }
    b
}

/// The interface of an interface section, every count within the IR's bounds.
pub fn decode_interface(b: &[u8]) -> Result<Interface, LibraryError> {
    let bad = LibraryError::Malformed("an interface");
    let head = b.get(..8).ok_or(LibraryError::Truncated)?;
    let stage = match head[0] {
        0 => Stage::Vertex,
        1 => Stage::Fragment,
        2 => Stage::Compute,
        _ => return Err(bad),
    };
    let (ni, no, nr) = (usize::from(head[1]), usize::from(head[2]), usize::from(head[3]));
    let push_bytes = u32::from(u16::from_le_bytes([head[4], head[5]]));
    if ni > MAX_INPUTS || no > MAX_OUTPUTS || nr > MAX_RESOURCES || head[6..8] != [0, 0] {
        return Err(bad);
    }
    if push_bytes > MAX_PUSH_BYTES || push_bytes % 4 != 0 {
        return Err(bad);
    }
    if b.len() != 8 + 4 * (ni + no + nr) {
        return Err(bad);
    }
    let rec = |k: usize| [b[8 + 4 * k], b[9 + 4 * k], b[10 + 4 * k], b[11 + 4 * k]];
    let mut inputs = Vec::with_capacity(ni);
    for k in 0..ni {
        let [location, scalar, lanes, interp] = rec(k);
        let interp = match interp {
            0 => Interp::Perspective,
            1 => Interp::Linear,
            2 => Interp::Flat,
            _ => return Err(bad),
        };
        inputs.push(Input { location, ty: ty_of(scalar, lanes).ok_or(bad)?, interp });
    }
    let mut outputs = Vec::with_capacity(no);
    for k in ni..ni + no {
        let [location, scalar, lanes, zero] = rec(k);
        if zero != 0 {
            return Err(bad);
        }
        outputs.push(Output { location, ty: ty_of(scalar, lanes).ok_or(bad)? });
    }
    let mut resources = Vec::with_capacity(nr);
    for k in ni + no..ni + no + nr {
        resources.push(match rec(k) {
            [1, binding, dim @ 1..=3, 0] => Resource::Texture {
                binding,
                dim: [TexDim::D1, TexDim::D2, TexDim::D3][usize::from(dim) - 1],
            },
            [2, binding, 0, 0] => Resource::Sampler { binding },
            _ => return Err(bad),
        });
    }
    Ok(Interface { stage, inputs, outputs, resources, push_bytes })
}

/// A pipeline for [`build`].
#[derive(Clone, Copy, Debug)]
pub struct EntrySpec<'a> {
    pub id: u32,
    pub name: &'a str,
    pub features: u32,
    pub budget: u16,
    pub interface: &'a Interface,
    pub program: &'a [u8],
    pub setup: &'a [&'a [u8]],
    pub state: &'a [u8],
}

fn push_section(e: &mut Vec<u8>, kind: u16, body: &[u8]) {
    e.extend_from_slice(&kind.to_le_bytes());
    e.extend_from_slice(&[0, 0]);
    e.extend_from_slice(&(body.len() as u32).to_le_bytes());
    e.extend_from_slice(body);
    e.resize(pad4(e.len()), 0);
}

/// Builds a library for `target`; what it builds always parses for that target (the same
/// bounds refuse it here).
pub fn build(target: u64, entries: &[EntrySpec<'_>]) -> Result<Vec<u8>, LibraryError> {
    if entries.is_empty() {
        return Err(LibraryError::Malformed("no entries"));
    }
    if entries.len() > MAX_ENTRIES {
        return Err(LibraryError::Oversize("the entry count"));
    }
    let mut blobs: Vec<Vec<u8>> = Vec::with_capacity(entries.len());
    for (i, s) in entries.iter().enumerate() {
        if s.id == 0 || entries[..i].iter().any(|o| o.id == s.id) {
            return Err(LibraryError::Malformed("an id zero or twice"));
        }
        parse_name(s.name.as_bytes())?;
        if s.setup.len() > MAX_SETUP {
            return Err(LibraryError::Oversize("an entry's setup programs"));
        }
        if s.program.is_empty() {
            return Err(LibraryError::Malformed("an empty program"));
        }
        let interface = encode_interface(s.interface);
        // Nothing narrowed on the way: the bytes say exactly the interface given.
        if decode_interface(&interface)? != *s.interface {
            return Err(LibraryError::Malformed("an interface"));
        }
        let mut e = Vec::new();
        e.extend_from_slice(&s.id.to_le_bytes());
        e.extend_from_slice(&s.features.to_le_bytes());
        e.extend_from_slice(&[0; 8]);
        e.extend_from_slice(&s.budget.to_le_bytes());
        e.extend_from_slice(&(4 + s.setup.len() as u16).to_le_bytes());
        push_section(&mut e, section::NAME, s.name.as_bytes());
        push_section(&mut e, section::INTERFACE, &interface);
        push_section(&mut e, section::PROGRAM, s.program);
        for setup in s.setup {
            push_section(&mut e, section::SETUP, setup);
        }
        push_section(&mut e, section::STATE, s.state);
        if e.len() > MAX_ENTRY_BYTES {
            return Err(LibraryError::Oversize("an entry"));
        }
        let d = digest(&e);
        e[8..16].copy_from_slice(&d.to_le_bytes());
        blobs.push(e);
    }
    let table_end = HEADER_BYTES + 8 * blobs.len();
    let total = table_end + blobs.iter().map(Vec::len).sum::<usize>();
    if total > MAX_LIBRARY_BYTES {
        return Err(LibraryError::Oversize("the archive"));
    }
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&(blobs.len() as u16).to_le_bytes());
    out.extend_from_slice(&target.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&[0; 12]);
    let mut off = table_end;
    for e in &blobs {
        out.extend_from_slice(&(off as u32).to_le_bytes());
        out.extend_from_slice(&(e.len() as u32).to_le_bytes());
        off += e.len();
    }
    for e in &blobs {
        out.extend_from_slice(e);
    }
    Ok(out)
}

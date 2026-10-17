// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The pipeline library's format (TASK-0215 5c-2 P4a, RFC-0101 §7b): a built library parses back
//! entry for entry; a library that is short, oversized, of another version or for another device
//! is refused whole; an entry whose digest or features do not fit is refused alone; no prefix
//! and no flipped bit of a library makes the parser panic.

use nexus_gfx::shader::ir::{Input, Interface, Interp, Output, Resource, Stage, TexDim, Type};
use nexus_gfx::shader::library::{
    build, digest, feature, parse, EntrySpec, LibraryError, Refusal, HEADER_BYTES, MAX_ENTRIES,
    MAX_ENTRY_BYTES,
};

const TARGET: u64 = 0x0024_001d_0034_00b6;

fn layer_interface() -> Interface {
    Interface {
        stage: Stage::Fragment,
        inputs: vec![Input { location: 0, ty: Type::VEC2, interp: Interp::Perspective }],
        outputs: vec![Output { location: 0, ty: Type::VEC4 }],
        resources: vec![
            Resource::Texture { binding: 0, dim: TexDim::D2 },
            Resource::Sampler { binding: 1 },
        ],
        push_bytes: 16,
    }
}

const PROGRAM_A: [u8; 32] = [0xa5; 32];
const PROGRAM_B: [u8; 120] = [0x5a; 120];
const SETUP: [&[u8]; 2] = [&[1, 2, 3, 4, 5, 6, 7, 8], &[9, 10, 11, 12]];
const STATE: [u8; 12] = [1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0];

fn library(features_b: u32) -> Vec<u8> {
    let i = layer_interface();
    let a = EntrySpec {
        id: 1,
        name: "layer-opaque",
        features: feature::FUSED_ITERATE_SAMPLE,
        budget: 3,
        interface: &i,
        program: &PROGRAM_A,
        setup: &SETUP,
        state: &STATE,
    };
    let b = EntrySpec {
        id: 2,
        name: "layer-premultiplied",
        features: features_b,
        budget: 11,
        program: &PROGRAM_B,
        ..a
    };
    build(TARGET, &[a, b]).expect("builds")
}

/// The entry table's (offset, length) of entry `i`.
fn entry_at(lib: &[u8], i: usize) -> (usize, usize) {
    let at = HEADER_BYTES + 8 * i;
    let w = |o: usize| u32::from_le_bytes([lib[o], lib[o + 1], lib[o + 2], lib[o + 3]]) as usize;
    (w(at), w(at + 4))
}

/// Rewrites an entry's digest after a patch, so the parse reaches the patched structure.
fn reseal(lib: &mut [u8], i: usize) {
    let (off, len) = entry_at(lib, i);
    let d = digest(&lib[off..off + len]);
    lib[off + 8..off + 16].copy_from_slice(&d.to_le_bytes());
}

#[test]
fn a_built_library_parses_back_entry_for_entry() {
    let lib = library(feature::FUSED_ITERATE_SAMPLE);
    let parsed = parse(&lib, TARGET, feature::FUSED_ITERATE_SAMPLE).expect("parses");
    assert!(parsed.refused.is_empty());
    assert_eq!(parsed.accepted.len(), 2);
    let (a, b) = (&parsed.accepted[0], &parsed.accepted[1]);
    assert_eq!((a.id, a.name, a.budget, a.program), (1, "layer-opaque", 3, &PROGRAM_A[..]));
    assert_eq!((b.id, b.name, b.budget, b.program), (2, "layer-premultiplied", 11, &PROGRAM_B[..]));
    assert_eq!(a.interface, layer_interface());
    assert_eq!((a.setup.as_slice(), a.state), (&SETUP[..], &STATE[..]));
}

#[test]
fn test_reject_library_truncated() {
    let lib = library(0);
    for cut in [0, 8, HEADER_BYTES - 1, HEADER_BYTES + 4, lib.len() - 1] {
        assert_eq!(parse(&lib[..cut], TARGET, 0), Err(LibraryError::Truncated), "cut at {cut}");
    }
    // A total size that is not the archive's.
    let mut long = lib.clone();
    long.extend_from_slice(&[0; 4]);
    assert_eq!(parse(&long, TARGET, 0), Err(LibraryError::Truncated));
    // An entry reaching past the end.
    let mut past = lib.clone();
    let at = HEADER_BYTES + 8 + 4;
    past[at..at + 4].copy_from_slice(&(lib.len() as u32).to_le_bytes());
    assert_eq!(parse(&past, TARGET, 0), Err(LibraryError::Truncated));
}

#[test]
fn test_reject_library_oversize_entry() {
    let mut lib = library(0);
    let at = HEADER_BYTES + 4;
    lib[at..at + 4].copy_from_slice(&(MAX_ENTRY_BYTES as u32 + 1).to_le_bytes());
    assert_eq!(parse(&lib, TARGET, 0), Err(LibraryError::Oversize("an entry")));
    let mut many = library(0);
    many[6..8].copy_from_slice(&(MAX_ENTRIES as u16 + 1).to_le_bytes());
    assert_eq!(parse(&many, TARGET, 0), Err(LibraryError::Oversize("the entry count")));
    // The builder refuses what the parser would.
    let i = layer_interface();
    let big = vec![1u8; MAX_ENTRY_BYTES];
    let spec = EntrySpec {
        id: 1,
        name: "big",
        features: 0,
        budget: 1,
        interface: &i,
        program: &big,
        setup: &[],
        state: &[],
    };
    assert_eq!(build(TARGET, &[spec]), Err(LibraryError::Oversize("an entry")));
}

#[test]
fn test_reject_library_unknown_version() {
    let mut lib = library(0);
    lib[4..6].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(parse(&lib, TARGET, 0), Err(LibraryError::UnknownVersion(2)));
    let mut magic = library(0);
    magic[0] = b'X';
    assert_eq!(parse(&magic, TARGET, 0), Err(LibraryError::BadMagic));
}

#[test]
fn test_reject_library_wrong_target() {
    let lib = library(0);
    assert_eq!(
        parse(&lib, TARGET + 1, 0),
        Err(LibraryError::WrongTarget { library: TARGET, device: TARGET + 1 })
    );
}

#[test]
fn test_reject_library_digest_mismatch() {
    let mut lib = library(0);
    // A byte of the first entry's program: that entry alone is refused.
    let (off, len) = entry_at(&lib, 0);
    let at = off + len - 64;
    lib[at] ^= 0x01;
    let parsed = parse(&lib, TARGET, feature::FUSED_ITERATE_SAMPLE).expect("the library parses");
    assert_eq!(parsed.refused, vec![(1, Refusal::DigestMismatch)]);
    assert_eq!(parsed.accepted.iter().map(|e| e.id).collect::<Vec<_>>(), vec![2]);
}

#[test]
fn test_reject_library_unsupported_feature() {
    // A bit this version names on a device without it, and a bit no version names.
    let lib = library(1 << 31);
    let parsed = parse(&lib, TARGET, 0).expect("the library parses");
    assert_eq!(
        parsed.refused,
        vec![
            (1, Refusal::UnsupportedFeature { missing: feature::FUSED_ITERATE_SAMPLE }),
            (2, Refusal::UnsupportedFeature { missing: 1 << 31 })
        ]
    );
    assert!(parsed.accepted.is_empty());
    let parsed = parse(&lib, TARGET, feature::FUSED_ITERATE_SAMPLE | (1 << 31)).expect("parses");
    assert_eq!(parsed.accepted.iter().map(|e| e.id).collect::<Vec<_>>(), vec![1]);
}

#[test]
fn test_reject_library_malformed_entries() {
    let f = feature::FUSED_ITERATE_SAMPLE;
    // The same id twice.
    let mut twice = library(f);
    let (off, _) = entry_at(&twice, 1);
    twice[off..off + 4].copy_from_slice(&1u32.to_le_bytes());
    reseal(&mut twice, 1);
    assert_eq!(parse(&twice, TARGET, f), Err(LibraryError::Malformed("an id zero or twice")));
    // An unknown section kind (the first section's kind word).
    let mut kind = library(f);
    let (off, _) = entry_at(&kind, 0);
    kind[off + 20..off + 22].copy_from_slice(&9u16.to_le_bytes());
    reseal(&mut kind, 0);
    assert_eq!(parse(&kind, TARGET, f), Err(LibraryError::Malformed("an unknown section")));
    // Reserved header bytes.
    let mut reserved = library(f);
    reserved[24] = 1;
    assert_eq!(parse(&reserved, TARGET, f), Err(LibraryError::Malformed("reserved header bytes")));
    // A name the format does not admit, a zero id, an interface the IR bounds refuse.
    let i = layer_interface();
    let spec = EntrySpec {
        id: 1,
        name: "Layer",
        features: 0,
        budget: 1,
        interface: &i,
        program: &[1],
        setup: &[],
        state: &[],
    };
    assert_eq!(build(TARGET, &[spec]), Err(LibraryError::Malformed("a name")));
    assert_eq!(
        build(TARGET, &[EntrySpec { id: 0, name: "a", ..spec }]),
        Err(LibraryError::Malformed("an id zero or twice"))
    );
    let wide = Interface { push_bytes: 1024, ..layer_interface() };
    assert_eq!(
        build(TARGET, &[EntrySpec { name: "a", interface: &wide, ..spec }]),
        Err(LibraryError::Malformed("an interface"))
    );
}

#[test]
fn every_truncation_and_every_flipped_bit_parses_without_a_panic() {
    let lib = library(feature::FUSED_ITERATE_SAMPLE);
    for cut in 0..lib.len() {
        let _ = parse(&lib[..cut], TARGET, u32::MAX);
    }
    for at in 0..lib.len() {
        for bit in 0..8 {
            let mut b = lib.clone();
            b[at] ^= 1 << bit;
            let _ = parse(&b, TARGET, u32::MAX);
        }
    }
}

#[test]
fn a_client_picks_its_library_by_the_header_target() {
    const BOARD: &[u8] = include_bytes!("../../../resources/gpu/pipelines/compositor.nxgp");
    const VIRGL: &[u8] = include_bytes!("../../../resources/gpu/pipelines/compositor-virgl.nxgp");
    use nexus_gfx::shader::library::header_target;
    assert_eq!(header_target(BOARD), Some(TARGET));
    assert_eq!(header_target(VIRGL), Some(nexus_gfx::shader::tgsi::CODE_TARGET));
    assert_eq!(header_target(&BOARD[..31]), None, "a header cut short");
    let mut magic = BOARD[..32].to_vec();
    magic[0] = b'X';
    assert_eq!(header_target(&magic), None, "another magic");
    let mut version = BOARD[..32].to_vec();
    version[4] = 2;
    assert_eq!(header_target(&version), None, "another version");
}

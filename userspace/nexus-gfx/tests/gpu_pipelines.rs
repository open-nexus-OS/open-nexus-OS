// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! A context's pipelines (TASK-0215 5c-2 P4d, RFC-0101 §7b): the compositor's library placed —
//! its programs in the code heap untouched, each kick naming where its program landed, a
//! constant block placed beside its setup — and a draw's pipeline found by id; an unknown id, a
//! second library, an entry this device does not
//! run (a program that does not decode, one above its budget, a setup it does not take) and a
//! heap without room are refused.

use nexus_gfx::backend::gpu::pipelines::{Heaps, PipelineTable, Refused, TableError};
use nexus_gfx::backend::gpu::CoreId;
use nexus_gfx::shader::ir::{Input, Interface, Interp, Output, Resource, Stage, TexDim, Type};
use nexus_gfx::shader::library::{build, feature, EntrySpec, LibraryError};
use nexus_gfx::shader::rogue::codegen::{Layout, Program};
use nexus_gfx::shader::rogue::pds::Doutu;
use nexus_gfx::shader::rogue::reference::{self, PASS_OPAQUE, PASS_TRANSLUCENT};
use nexus_gfx::shader::rogue::setup::{fragment, Varyings};

const LIBRARY: &[u8] = include_bytes!("../../../resources/gpu/pipelines/compositor.nxgp");
const TARGET: u64 = CoreId::BOARD.0;
const FEATURES: u32 = feature::FUSED_ITERATE_SAMPLE;

/// Two heaps in memory, bumped in 64-byte lines from their bases.
struct Mem {
    code: Vec<u8>,
    setup: Vec<u8>,
    room: bool,
}

const CODE_BASE: u32 = 0x9000;
const SETUP_BASE: u32 = 0x3000;

fn put(heap: &mut Vec<u8>, base: u32, bytes: &[u8]) -> u32 {
    let at = heap.len().next_multiple_of(64);
    heap.resize(at, 0);
    heap.extend_from_slice(bytes);
    base + at as u32
}

impl Heaps for Mem {
    fn code(&mut self, bytes: &[u8]) -> Option<u32> {
        self.room.then(|| put(&mut self.code, CODE_BASE, bytes))
    }
    fn setup(&mut self, bytes: &[u8]) -> Option<u32> {
        self.room.then(|| put(&mut self.setup, SETUP_BASE, bytes))
    }
}

fn mem() -> Mem {
    Mem { code: Vec::new(), setup: Vec::new(), room: true }
}

fn layer() -> Interface {
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

#[test]
fn the_compositor_library_is_placed_and_each_kick_names_its_program() {
    let mut table = PipelineTable::default();
    let mut heaps = mem();
    let loaded = table.load(LIBRARY, TARGET, FEATURES, &mut heaps).expect("loads");
    assert_eq!(
        (loaded.accepted.as_slice(), loaded.refused.as_slice()),
        (&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13][..], &[][..])
    );
    // The layers, the shadow, the blur at radius 20, then at 8, 30 and 40 (5c-3 P4b).
    assert_eq!(
        loaded.code_bytes,
        32 + 120 + 160 + 360 + 472 + 936 + 728 + 432 + 416 + 1352 + 1040 + 1776 + 1248
    );
    for (id, temps, pass, program) in [
        (1, 1, PASS_OPAQUE, reference::opaque_itrsmp().unwrap().encode().unwrap()),
        (2, 3, PASS_TRANSLUCENT, reference::premultiplied_itrsmp().unwrap().encode().unwrap()),
    ] {
        let p = table.get(id).expect("accepted");
        assert_eq!((p.pass_type, p.setup.state.temps), (pass, temps));
        // The program lands untouched; the kick (eight data dwords, code at +0x20) names it.
        let at = (p.exe_off - CODE_BASE) as usize;
        assert_eq!(&heaps.code[at..at + program.len()], &program[..]);
        let k = (p.kick - SETUP_BASE) as usize;
        let w = |i: usize| {
            u32::from_le_bytes(heaps.setup[k + 4 * i..k + 4 * i + 4].try_into().unwrap())
        };
        let doutu = Doutu::decode(u64::from(w(0)) | (u64::from(w(1)) << 32));
        assert_eq!((doutu.exe_off, doutu.temps), (p.exe_off, temps));
        assert_eq!(w(8), 0xf400_0002, "dout U, end");
        assert_eq!(p.pds_state(0x4000).uniformdatabase, 0x4000);
    }
    assert_eq!(table.ids().collect::<Vec<_>>(), (1..=13).collect::<Vec<_>>());
}

#[test]
fn test_reject_draw_unknown_pipeline() {
    let mut table = PipelineTable::default();
    assert_eq!(table.get(1), Err(TableError::UnknownPipeline(1)), "before any library");
    table.load(LIBRARY, TARGET, FEATURES, &mut mem()).expect("loads");
    assert_eq!(table.get(14), Err(TableError::UnknownPipeline(14)));
    assert_eq!(table.get(0), Err(TableError::UnknownPipeline(0)));
}

#[test]
fn test_reject_a_second_library_for_the_context() {
    let mut table = PipelineTable::default();
    table.load(LIBRARY, TARGET, FEATURES, &mut mem()).expect("loads");
    assert_eq!(table.load(LIBRARY, TARGET, FEATURES, &mut mem()), Err(TableError::Loaded));
}

#[test]
fn test_reject_entries_this_device_does_not_run() {
    let i = layer();
    let opaque = reference::opaque_itrsmp().unwrap().encode().unwrap();
    let setup = fragment(
        &Program { groups: Vec::new(), temps: 1, pass_type: PASS_OPAQUE, constants: Vec::new() },
        &i,
        Layout::BUNDLE,
        Varyings::BUNDLE,
    )
    .unwrap();
    let (sections, state) = setup.sections().unwrap();
    let refs: Vec<&[u8]> = sections.iter().map(Vec::as_slice).collect();
    let mut bad_state = state.clone();
    bad_state[0] = 0;
    let good = EntrySpec {
        id: 4,
        name: "good",
        features: FEATURES,
        budget: 3,
        interface: &i,
        program: &opaque,
        setup: &refs,
        state: &state,
    };
    let lib = build(
        TARGET,
        &[
            EntrySpec { id: 1, name: "garbage", program: &[0xff; 16], ..good },
            EntrySpec { id: 2, name: "over-budget", budget: 2, ..good },
            EntrySpec { id: 3, name: "bad-setup", state: &bad_state, ..good },
            good,
        ],
    )
    .unwrap();
    let mut table = PipelineTable::default();
    let loaded = table.load(&lib, TARGET, FEATURES, &mut mem()).expect("the library loads");
    assert_eq!(loaded.accepted, vec![4]);
    assert_eq!(
        loaded.refused,
        vec![(1, Refused::Program), (2, Refused::OverBudget), (3, Refused::Setup)]
    );
    assert!(table.get(4).is_ok());
}

#[test]
fn test_reject_a_library_without_room_or_for_another_device() {
    let mut table = PipelineTable::default();
    let mut full = Mem { room: false, ..mem() };
    assert_eq!(table.load(LIBRARY, TARGET, FEATURES, &mut full), Err(TableError::NoRoom));
    let mut table = PipelineTable::default();
    assert_eq!(
        table.load(LIBRARY, TARGET + 1, FEATURES, &mut mem()),
        Err(TableError::Library(LibraryError::WrongTarget { library: TARGET, device: TARGET + 1 }))
    );
}

#[test]
fn a_pipelines_constant_block_is_placed_once_beside_its_setup() {
    let i = layer();
    let opaque = reference::opaque_itrsmp().unwrap().encode().unwrap();
    let block = vec![0.7f32.to_bits(), 0.3f32.to_bits()];
    let program =
        Program { groups: Vec::new(), temps: 1, pass_type: PASS_OPAQUE, constants: block };
    let setup = fragment(&program, &i, Layout::BUNDLE, Varyings::BUNDLE).unwrap();
    let (sections, state) = setup.sections().unwrap();
    let refs: Vec<&[u8]> = sections.iter().map(Vec::as_slice).collect();
    let spec = EntrySpec {
        id: 9,
        name: "weighted",
        features: FEATURES,
        budget: 3,
        interface: &i,
        program: &opaque,
        setup: &refs,
        state: &state,
    };
    let lib = build(TARGET, &[spec]).unwrap();
    let mut table = PipelineTable::default();
    let mut heaps = mem();
    let loaded = table.load(&lib, TARGET, FEATURES, &mut heaps).expect("loads");
    let p = table.get(9).expect("accepted");
    // The values land where the placement says, after the three setup programs; the state load
    // reads them at the PDS heap's address of that place.
    let at = p.constants.expect("a block");
    assert!(at > p.state_code);
    let off = (at - SETUP_BASE) as usize;
    assert_eq!(&heaps.setup[off..off + 8], &setup.constant_bytes()[..]);
    assert_eq!(p.constants_va(), Some(0xda_0000_0000 + u64::from(at)));
    let programs = 4 * (8 + 1) + setup.iterate.encode().unwrap().len() + 4 * 3;
    assert_eq!(loaded.setup_bytes, programs + 8);
    // Of the compositor's pipelines only the blur passes read a block (their weights).
    let mut table = PipelineTable::default();
    table.load(LIBRARY, TARGET, FEATURES, &mut mem()).expect("loads");
    for id in table.ids() {
        assert_eq!(table.get(id).is_ok_and(|p| p.constants.is_some()), id >= 6, "{id}");
    }
}

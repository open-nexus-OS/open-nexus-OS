// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! What the device runs allocates nothing it does not keep (TASK-0215, before 5c-3): gpud's heap
//! never frees, so a context's library is checked without decoding its programs into memory — the
//! groups are counted as they are decoded — and the table keeps only each pipeline's placement
//! and setup; a pass's command streams and its state-load data are made in place. (The emitter's
//! own proof is `gpu_quads_model::a_pass_is_emitted_without_allocating`.)

use std::alloc::{GlobalAlloc, Layout as AllocLayout, System};
use std::cell::Cell;

use nexus_gfx::backend::gpu::fwif::{FragCmd, GeomCmd, RenderFeatures};
use nexus_gfx::backend::gpu::pipelines::{Heaps, PipelineTable};
use nexus_gfx::backend::gpu::scene::Scene;
use nexus_gfx::backend::gpu::CoreId;
use nexus_gfx::shader::library::{feature, parse};
use nexus_gfx::shader::rogue::{count_groups, decode_program};

const LIBRARY: &[u8] = include_bytes!("../../../resources/gpu/pipelines/compositor.nxgp");
const RENDER: &[u8] = include_bytes!("../../../resources/gpu/scenes/render-quad.scene");

thread_local! {
    static ALLOCS: Cell<usize> = const { Cell::new(0) };
    static BYTES: Cell<usize> = const { Cell::new(0) };
}

/// Counts this thread's allocations and their bytes (tests run on their own threads).
struct Counting;

fn count(bytes: usize) {
    let _ = ALLOCS.try_with(|c| c.set(c.get() + 1));
    let _ = BYTES.try_with(|c| c.set(c.get() + bytes));
}

// SAFETY: every method forwards to `System`, which upholds the GlobalAlloc contract; the only
// addition is a non-allocating counter bump.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: AllocLayout) -> *mut u8 {
        count(layout.size());
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: AllocLayout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: AllocLayout, new_size: usize) -> *mut u8 {
        count(new_size);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

/// This thread's allocations and their bytes so far.
fn so_far() -> (usize, usize) {
    (ALLOCS.try_with(Cell::get).unwrap_or(0), BYTES.try_with(Cell::get).unwrap_or(0))
}

/// Two heaps bumped in 64-byte lines, owning nothing.
struct Lines(u32);

impl Heaps for Lines {
    fn code(&mut self, bytes: &[u8]) -> Option<u32> {
        let at = self.0.next_multiple_of(64);
        self.0 = at + bytes.len() as u32;
        Some(at)
    }
    fn setup(&mut self, bytes: &[u8]) -> Option<u32> {
        self.code(bytes)
    }
}

#[test]
fn a_programs_groups_are_counted_without_allocating() {
    let lib = parse(LIBRARY, CoreId::BOARD.0, feature::FUSED_ITERATE_SAMPLE).expect("parses");
    for e in &lib.accepted {
        let before = so_far();
        let n = count_groups(e.program).expect("decodes");
        assert_eq!(so_far(), before, "{}: allocations while counting", e.name);
        assert_eq!(n, decode_program(e.program).expect("decodes").len(), "{}", e.name);
    }
}

#[test]
fn the_table_keeps_what_it_allocates() {
    // Each entry's placement and setup (its programs' words, its loads, its constant block): about
    // a kilobyte an entry, never its program decoded (that was fourteen times the program).
    let lib = parse(LIBRARY, CoreId::BOARD.0, feature::FUSED_ITERATE_SAMPLE).expect("parses");
    let entries = lib.accepted.len();
    let program_bytes: usize = lib.accepted.iter().map(|e| e.program.len()).sum();
    let before = so_far();
    let mut table = PipelineTable::default();
    table
        .load(LIBRARY, CoreId::BOARD.0, feature::FUSED_ITERATE_SAMPLE, &mut Lines(0))
        .expect("loads");
    let (_, bytes) = so_far();
    let used = bytes - before.1;
    assert!(used < 2048 * entries, "{used} bytes for {entries} entries");
    assert!(used < 4 * program_bytes, "{used} bytes against {program_bytes} of programs");
}

#[test]
fn a_passs_command_streams_and_state_data_are_made_in_place() {
    let scene = Scene::parse(RENDER, CoreId::BOARD).expect("the render bundle parses");
    let jobs: Vec<_> = scene.jobs().collect();
    let feats = RenderFeatures::BOARD;
    let g = GeomCmd::from_stream(jobs[0].stream, feats, jobs[0].flags, 0, Default::default())
        .expect("geometry");
    let f = FragCmd::from_stream(jobs[2].stream, feats, jobs[2].flags, 1).expect("fragment");
    let mut table = PipelineTable::default();
    table
        .load(LIBRARY, CoreId::BOARD.0, feature::FUSED_ITERATE_SAMPLE, &mut Lines(0))
        .expect("loads");
    let blur = table.get(6).expect("the horizontal blur");
    let before = so_far();
    let (gs, fs) = (g.to_stream(feats), f.to_stream(feats));
    let data = blur.setup.state_data(|_| 0x80_0000_0000).expect("state data");
    assert_eq!(so_far(), before, "allocations while making a pass's streams and state data");
    assert_eq!((&*gs, &*fs), (jobs[0].stream, jobs[2].stream));
    assert_eq!(data.len(), blur.setup.state_data_dwords());
}

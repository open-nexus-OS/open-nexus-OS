// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The reference programs (TASK-0215 5c-2 P3b) as bytes the board can run: each encodes, decodes
//! back to its own groups, ends on the instruction cache's alignment and has the group count its
//! question names; the opaque ones never touch the tile, the blended one reads it once.

use nexus_gfx::shader::rogue::backend::BackendInstr;
use nexus_gfx::shader::rogue::reference::{all, PASS_OPAQUE, PASS_TRANSLUCENT};
use nexus_gfx::shader::rogue::{decode_program, Instr, ICACHE_ALIGN};

#[test]
fn every_reference_program_round_trips_and_ends_aligned() {
    for r in all().expect("the references assemble") {
        let bytes = r.encode().expect("encodes");
        let back = decode_program(&bytes).expect("decodes");
        assert_eq!(back, r.groups, "{}: our decoder reads our encoder", r.name);
        let last = back.last().expect("a group");
        let offset: usize = back[..back.len() - 1].iter().map(|g| g.size()).sum();
        assert_eq!((offset % ICACHE_ALIGN, last.size() % ICACHE_ALIGN), (0, 0), "{}", r.name);
        assert!(
            last.header.end && last.header.olchk,
            "{}: the pack into the pixel output ends it",
            r.name
        );
        assert!(!back
            .iter()
            .flat_map(|g| &g.instrs)
            .any(|(_, i)| matches!(i, Instr::Backend(BackendInstr::VisTest { .. }))));
    }
}

#[test]
fn the_group_counts_are_the_questions() {
    let counts: Vec<(&str, usize, u8)> =
        all().unwrap().iter().map(|r| (r.name, r.groups.len(), r.pass_type)).collect();
    assert_eq!(
        counts,
        [
            ("opaque-ditr", 5, PASS_OPAQUE),
            ("tinted", 6, PASS_OPAQUE),
            ("premultiplied", 13, PASS_TRANSLUCENT),
            ("opaque-itrsmp", 3, PASS_OPAQUE),
            ("opaque-itrsmp-count", 3, PASS_OPAQUE),
        ]
    );
}

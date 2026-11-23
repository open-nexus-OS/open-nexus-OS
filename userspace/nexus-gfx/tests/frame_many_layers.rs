// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! A scene past the old table of 64 layers (TASK-0351 P6): the set, the plan, the glass cache's
//! slots and the board follow the scene — twenty windows lost the launcher on the board because
//! a frame held 64 layers (`board--2026-11-18T11-44-54`).

#[path = "frame_support.rs"]
mod support;

use nexus_gfx::command::buffer::Command;
use nexus_gfx::frame::{Board, CacheStep, PassKind, Target, MERGE_MAX};
use support::*;

const PLANE_ROWS: u32 = 1024;

/// `windows` windows cascaded over one another, each a glass back and two panes over it —
/// keyed as windowd keys them.
fn cascade(windows: u32) -> Vec<Command> {
    (0..windows)
        .flat_map(|w| {
            let (x, y, key) = (40 + 8 * w, 40 + 6 * w, (w + 1) << 16);
            let part = move |cmd: Command, n: u32| {
                with(cmd, |l| {
                    l.layer_id = w + 1;
                    l.key = key | n;
                })
            };
            [
                part(glass(layer(x, y, 900, 600), 40), 0x100),
                part(layer(x + 200, y + 40, 700, 560), 0x200),
                part(layer(x, y + 40, 200, 560), 0x300),
            ]
        })
        .collect()
}

#[test]
fn a_scene_of_two_hundred_layers_is_planned_whole_and_every_glass_has_rows() {
    let mut f = ready_frame(PLANE_ROWS);
    let scene = cascade(70);
    let mut cmds = scene.clone();
    cmds.push(damage(0, 0, 1, 1));
    assert_eq!(f.layers.take_frame(&cmds).dropped, 0);
    let p = f.plan(DISPLAY, &every_blur);
    assert_eq!(p.layers, 210);
    // The cache of one plane is short; planes are added until no glass is off.
    let mut planes = 1;
    while f.glass.short() != 0 {
        f.place_short();
        planes += 1;
        assert!(planes < 64, "the cache never fills");
    }
    let glasses = (0..p.layers).filter_map(|i| cache_of(&f, i));
    assert!(glasses.clone().all(|c| matches!(c, CacheStep::Write(_))));
    assert_eq!(glasses.count(), 70);
    // The same scene again, a pointer-sized change above it: every glass reads its rows.
    let mut again = scene.clone();
    again.push(Command::Damage {
        rect: nexus_gfx::core::types::TileRect { x: 5, y: 5, width: 1, height: 1 },
        origin: u32::MAX,
    });
    f.layers.take_frame(&again);
    let p = f.plan(DISPLAY, &every_blur);
    assert_eq!((p.layers, p.writes.total(), f.glass.short()), (210, 0, 0));
    assert!((0..p.layers).filter_map(|i| cache_of(&f, i)).all(|c| matches!(c, CacheStep::Read(_))));
    // A window closed in the middle: the others keep their rows.
    let rows_of = cache_of;
    let last = rows_of(&f, 207);
    let mut closed: Vec<Command> = scene[..30].to_vec();
    closed.extend_from_slice(&scene[33..]);
    closed.push(damage(0, 0, 1, 1));
    f.layers.take_frame(&closed);
    let p = f.plan(DISPLAY, &every_blur);
    assert_eq!(p.layers, 207);
    let row = |c: Option<CacheStep>| match c {
        Some(CacheStep::Read(y) | CacheStep::Write(y)) => Some(y),
        _ => None,
    };
    assert_eq!(row(rows_of(&f, 204)), row(last), "the last window's rows did not move");
}

#[test]
fn windows_over_one_another_write_a_wave_each() {
    // A cascade, more windows than a wave holds: each one's glass shows the one below it, so
    // each runs in a wave of its own — three passes a write, as one by one.
    let mut f = ready_frame(PLANE_ROWS);
    let windows = MERGE_MAX as u32 + 4;
    f.layers.take_frame(&cascade(windows));
    let p = f.plan(DISPLAY, &every_blur);
    while f.glass.short() != 0 {
        f.place_short();
    }
    let b = Board::new(&f, p, DISPLAY, 14_040, 600);
    assert_eq!(b.glass.writes as u32, windows);
    assert_eq!(b.wave_count(), windows as usize);
    assert_eq!(b.pass_count(), 3 * windows as usize + 1);
    // Every write's chain ends in its own rows of its plane; the slot's pass is the last.
    for w in 0..windows as usize {
        let rows = b.pass(3 * w + 2).expect("a vertical pass");
        assert!(matches!(rows.target, Target::Cache(plane) if plane < f.glass.planes()));
        assert!(matches!(rows.kind, PassKind::One(_)));
    }
    let slot = b.pass(b.pass_count() - 1).expect("the slot's pass");
    assert_eq!(slot.target, Target::Slot);
    assert!(matches!(slot.kind, PassKind::Scene { upto } if upto == 3 * windows as usize));
}

// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! TASK-0333 6e-2 host proofs: the one memory authority's whole contract through the same
//! `answer` the service loop runs — the band table and the victim rule (never the foreground,
//! at `critical` nothing on screen, the lowest band first and the longest there first), the
//! sender gates (`test_reject_*`), the status reply, the drill and the kill proof that arrives
//! only with the victim's `gone` band.

use memd::answer::{Memd, Outcome, Peers};
use memd::authority::{BandEvent, Decision, Pending, Source};
use memd::bands::{kill_floor, Applied, Table};
use memd::order::{encode_mem_kill, mem_kill_carried_out, Recording};
use nexus_abi::syscall::memory::{
    MEM_PRESSURE_CRITICAL, MEM_PRESSURE_EMERGENCY, MEM_PRESSURE_NORMAL, MEM_PRESSURE_WARN,
};
use nexus_wire::memd as wire;

const ABILITYMGR: u64 = 0xAB11;
const HARNESS: u64 = 0x5E1F;
const GPUD: u64 = 0x6B0D;
const STRANGER: u64 = 0x0BAD;
const PEERS: Peers = Peers { abilitymgr: ABILITYMGR, harness: HARNESS, gpud: GPUD };

fn push(m: &mut Memd, pid: u32, band: u8) -> Outcome {
    let mut out = [0u8; wire::REPLY_MAX_BYTES];
    let mut orders = Recording::accepting();
    let (n, outcome) = m.answer(&wire::encode_band(pid, band), ABILITYMGR, &mut out, &mut orders);
    assert_eq!(n, 0, "a band push is never answered");
    outcome
}

fn status(m: &mut Memd) -> (u8, u8, u8, u8, u8, u16, u16, u64, u64) {
    let mut out = [0u8; wire::REPLY_MAX_BYTES];
    let mut orders = Recording::accepting();
    let (n, outcome) = m.answer(&wire::encode_status(), HARNESS, &mut out, &mut orders);
    assert_eq!(outcome, Outcome::Status);
    wire::decode_status_reply(wire::OP_STATUS, &out[..n]).expect("status reply")
}

fn drill(m: &mut Memd, level: u8, orders: &mut Recording) -> (u8, u32, u8) {
    let mut out = [0u8; wire::REPLY_MAX_BYTES];
    let (n, outcome) = m.answer(&wire::encode_drill(level), HARNESS, &mut out, orders);
    assert!(matches!(outcome, Outcome::Drill { .. }), "{outcome:?}");
    wire::decode_drill_reply(wire::OP_DRILL, &out[..n]).expect("drill reply")
}

/// The scene of the visible lane: the shell's app focused, one visible beside it, one hidden
/// in its grace, one declared background, two frozen (the older one first).
fn scene() -> Memd {
    let mut m = Memd::new(PEERS, true);
    push(&mut m, 10, wire::BAND_FOREGROUND);
    push(&mut m, 11, wire::BAND_VISIBLE);
    push(&mut m, 12, wire::BAND_HIDDEN);
    push(&mut m, 13, wire::BAND_DECLARED);
    push(&mut m, 14, wire::BAND_SUSPENDED);
    push(&mut m, 15, wire::BAND_SUSPENDED);
    m
}

#[test]
fn the_floor_follows_the_level() {
    assert_eq!(kill_floor(MEM_PRESSURE_NORMAL), None);
    assert_eq!(kill_floor(MEM_PRESSURE_WARN), None);
    assert_eq!(kill_floor(MEM_PRESSURE_CRITICAL), Some(wire::BAND_HIDDEN));
    assert_eq!(kill_floor(MEM_PRESSURE_EMERGENCY), Some(wire::BAND_VISIBLE));
}

#[test]
fn test_reject_kill_of_foreground_or_visible() {
    // Only the foreground and a visible window: critical finds nobody; emergency takes the
    // visible one and NEVER the foreground, however hard the pool is pressed.
    let mut t = Table::new();
    t.apply(1, wire::BAND_FOREGROUND);
    t.apply(2, wire::BAND_VISIBLE);
    assert_eq!(t.victim(MEM_PRESSURE_CRITICAL), None, "critical never takes what is on screen");
    assert_eq!(t.victim(MEM_PRESSURE_EMERGENCY).map(|e| e.pid), Some(2));
    t.apply(2, wire::BAND_GONE);
    assert_eq!(t.victim(MEM_PRESSURE_EMERGENCY), None, "the foreground is never a victim");
    assert_eq!(t.victim(MEM_PRESSURE_WARN), None, "warn kills nobody");
}

#[test]
fn the_lowest_band_goes_first_and_the_longest_in_it() {
    let mut m = scene();
    let t = m.authority().table();
    // Suspended before hidden; of the two suspended, 14 (frozen first) before 15.
    assert_eq!(t.victim(MEM_PRESSURE_CRITICAL).map(|e| (e.pid, e.band)), Some((14, 4)));
    let mut orders = Recording::accepting();
    assert_eq!(drill(&mut m, MEM_PRESSURE_CRITICAL, &mut orders), (wire::STATUS_OK, 14, 4));
    assert_eq!(orders.orders(), &[14]);
    // The kill is in flight: a second level orders nothing until the victim is gone.
    assert_eq!(drill(&mut m, MEM_PRESSURE_CRITICAL, &mut orders), (wire::STATUS_NO_VICTIM, 0, 0));
    assert_eq!(orders.orders(), &[14], "one kill at a time");
    // The reap came back as the victim's gone band: the kill is proven, a drill asks no more.
    assert_eq!(
        push(&mut m, 14, wire::BAND_GONE),
        Outcome::Band(BandEvent::KillProven {
            pending: Pending { pid: 14, band: 4, source: Source::Drill },
            again: false,
        })
    );
    // Next: the other frozen one, then the hidden one, then nothing at critical.
    assert_eq!(drill(&mut m, MEM_PRESSURE_CRITICAL, &mut orders), (wire::STATUS_OK, 15, 4));
    push(&mut m, 15, wire::BAND_GONE);
    assert_eq!(drill(&mut m, MEM_PRESSURE_CRITICAL, &mut orders), (wire::STATUS_OK, 12, 3));
    push(&mut m, 12, wire::BAND_GONE);
    assert_eq!(drill(&mut m, MEM_PRESSURE_CRITICAL, &mut orders), (wire::STATUS_NO_VICTIM, 0, 0));
    // Emergency takes the declared background, then the visible one — never pid 10.
    assert_eq!(drill(&mut m, MEM_PRESSURE_EMERGENCY, &mut orders), (wire::STATUS_OK, 13, 2));
    push(&mut m, 13, wire::BAND_GONE);
    assert_eq!(drill(&mut m, MEM_PRESSURE_EMERGENCY, &mut orders), (wire::STATUS_OK, 11, 1));
    push(&mut m, 11, wire::BAND_GONE);
    assert_eq!(drill(&mut m, MEM_PRESSURE_EMERGENCY, &mut orders), (wire::STATUS_NO_VICTIM, 0, 0));
    assert_eq!(m.authority().table().len(), 1, "the foreground alone remains");
}

#[test]
fn a_kernel_level_judges_at_once_and_asks_again_when_proven() {
    let mut m = scene();
    let mut orders = Recording::accepting();
    let a = m.authority();
    assert_eq!(a.on_pressure(MEM_PRESSURE_WARN, 9_000, 77_261, &mut orders), Decision::None);
    assert_eq!(a.level(), (MEM_PRESSURE_WARN, 9_000, 77_261));
    assert_eq!(
        a.on_pressure(MEM_PRESSURE_CRITICAL, 4_338, 77_261, &mut orders),
        Decision::Kill { pid: 14, band: 4, ordered: true }
    );
    assert_eq!(a.kills(), 1);
    // A kernel-sourced kill, proven, asks the loop to read the level again (one frame per
    // level CHANGE: a pool still at critical needs the next victim without a new frame).
    assert_eq!(
        a.on_band(14, wire::BAND_GONE),
        BandEvent::KillProven {
            pending: Pending { pid: 14, band: 4, source: Source::Kernel },
            again: true
        }
    );
}

#[test]
fn a_refused_order_is_said_and_leaves_nothing_in_flight() {
    let mut m = scene();
    let mut refusing = Recording::refusing();
    assert_eq!(drill(&mut m, MEM_PRESSURE_CRITICAL, &mut refusing), (wire::STATUS_FAILED, 14, 4));
    assert_eq!(m.authority().pending(), None);
    assert_eq!(m.authority().kills(), 0, "a refused order is not a kill");
}

#[test]
fn status_counts_the_bands_and_the_resumes() {
    let mut m = scene();
    assert_eq!(
        status(&mut m),
        (wire::STATUS_OK, MEM_PRESSURE_NORMAL, 1, 6, 2, 0, 0, 0, 0),
        "(status, level, sink, bands, suspended, resumed, kills, free, total)"
    );
    // A frozen app seen again: suspended → foreground is one resume.
    assert_eq!(
        push(&mut m, 14, wire::BAND_FOREGROUND),
        Outcome::Band(BandEvent::Applied {
            pid: 14,
            band: 0,
            applied: Applied::Moved { from: wire::BAND_SUSPENDED }
        })
    );
    assert_eq!(
        push(&mut m, 14, wire::BAND_FOREGROUND),
        Outcome::Band(BandEvent::Applied { pid: 14, band: 0, applied: Applied::Unchanged })
    );
    let s = status(&mut m);
    assert_eq!((s.3, s.4, s.5), (6, 1, 1));
    let mut unbound = Memd::new(PEERS, false);
    assert_eq!(status(&mut unbound).2, 0, "the sink byte says the bind failed");
}

#[test]
fn test_reject_band_from_anyone_but_the_lifecycle_authority() {
    let mut m = Memd::new(PEERS, true);
    let mut out = [0u8; wire::REPLY_MAX_BYTES];
    let mut orders = Recording::accepting();
    for sender in [STRANGER, HARNESS, 0] {
        let (n, outcome) =
            m.answer(&wire::encode_band(7, wire::BAND_SUSPENDED), sender, &mut out, &mut orders);
        assert_eq!((n, outcome), (0, Outcome::BandDenied), "sender {sender:#x}");
    }
    assert!(m.authority().table().is_empty(), "a denied band changes nothing");
}

/// TASK-0333 6e-4: a GPU charge is taken from the GPU driver alone; it is absolute (replaces),
/// zero frees the row, and the push is never answered.
#[test]
fn test_reject_gpu_charge_from_anyone_but_gpud() {
    let mut m = Memd::new(PEERS, true);
    let mut out = [0u8; wire::REPLY_MAX_BYTES];
    let mut orders = Recording::accepting();
    const WINDOWD: u64 = 0x77;
    for sender in [STRANGER, ABILITYMGR, HARNESS, 0] {
        let (n, outcome) =
            m.answer(&wire::encode_gpu_charge(WINDOWD, 1 << 20), sender, &mut out, &mut orders);
        assert_eq!((n, outcome), (0, Outcome::GpuChargeDenied), "sender {sender:#x}");
    }
    assert_eq!(m.gpu().total(), 0, "a denied charge counts nothing");
    let (n, outcome) =
        m.answer(&wire::encode_gpu_charge(WINDOWD, 40 << 20), GPUD, &mut out, &mut orders);
    assert_eq!(n, 0, "a charge push is never answered");
    assert_eq!(
        outcome,
        Outcome::GpuCharge { client: WINDOWD, bytes: 40 << 20, total: Some(40 << 20) }
    );
    let (_, outcome) =
        m.answer(&wire::encode_gpu_charge(WINDOWD, 46 << 20), GPUD, &mut out, &mut orders);
    assert_eq!(
        outcome,
        Outcome::GpuCharge { client: WINDOWD, bytes: 46 << 20, total: Some(46 << 20) },
        "absolute, not a delta"
    );
    assert_eq!(m.gpu().of(WINDOWD), 46 << 20);
    let (_, outcome) = m.answer(&wire::encode_gpu_charge(WINDOWD, 0), GPUD, &mut out, &mut orders);
    assert_eq!(outcome, Outcome::GpuCharge { client: WINDOWD, bytes: 0, total: Some(0) });
    assert!(m.gpu().is_empty(), "zero frees the row");
    assert!(orders.orders().is_empty(), "a charge orders nothing");
}

#[test]
fn test_reject_drill_from_anyone_but_the_harness() {
    let mut m = scene();
    let mut out = [0u8; wire::REPLY_MAX_BYTES];
    let mut orders = Recording::accepting();
    for sender in [STRANGER, ABILITYMGR, 0] {
        let (n, outcome) =
            m.answer(&wire::encode_drill(MEM_PRESSURE_CRITICAL), sender, &mut out, &mut orders);
        assert_eq!(outcome, Outcome::DrillDenied, "sender {sender:#x}");
        assert_eq!(
            wire::decode_drill_reply(wire::OP_DRILL, &out[..n]),
            Some((wire::STATUS_DENIED, 0, 0))
        );
    }
    assert!(orders.orders().is_empty(), "a denied drill orders nothing");
    assert_eq!(m.authority().table().len(), 6);
}

#[test]
fn test_reject_malformed_frames_and_unknown_levels() {
    let mut m = scene();
    let mut out = [0u8; wire::REPLY_MAX_BYTES];
    let mut orders = Recording::accepting();
    let short = &wire::encode_band(1, 1)[..6];
    assert_eq!(m.answer(short, ABILITYMGR, &mut out, &mut orders), (0, Outcome::Malformed));
    assert_eq!(m.answer(&[], ABILITYMGR, &mut out, &mut orders), (0, Outcome::Malformed));
    let mut foreign = wire::encode_status();
    foreign[0] = b'X';
    assert_eq!(m.answer(&foreign, HARNESS, &mut out, &mut orders), (0, Outcome::Malformed));
    let mut unknown_op = wire::encode_status();
    unknown_op[3] = 9;
    assert_eq!(m.answer(&unknown_op, HARNESS, &mut out, &mut orders), (0, Outcome::Unsupported));
    assert_eq!(
        m.answer(&wire::encode_drill(7), HARNESS, &mut out, &mut orders),
        (0, Outcome::Malformed)
    );
    // An unknown band is dropped, named; the table is unchanged.
    assert_eq!(
        push(&mut m, 99, 6),
        Outcome::Band(BandEvent::Applied { pid: 99, band: 6, applied: Applied::UnknownBand })
    );
    assert_eq!(m.authority().table().len(), 6);
    assert!(orders.orders().is_empty());
}

#[test]
fn the_table_is_bounded_and_a_gone_stranger_is_nothing() {
    let mut t = Table::new();
    for pid in 1..=memd::bands::MAX_PROCESSES as u32 {
        assert_eq!(t.apply(pid, wire::BAND_HIDDEN), Applied::Entered);
    }
    assert_eq!(t.apply(1000, wire::BAND_HIDDEN), Applied::Full);
    assert_eq!(t.apply(1000, wire::BAND_GONE), Applied::UnknownGone);
    assert_eq!(t.apply(1, wire::BAND_GONE), Applied::Gone { was: wire::BAND_HIDDEN });
    assert_eq!(t.apply(1000, wire::BAND_HIDDEN), Applied::Entered, "a freed slot is reused");
    assert_eq!(t.count_in(wire::BAND_HIDDEN), memd::bands::MAX_PROCESSES);
}

#[test]
fn the_kill_order_is_the_lifecycle_authoritys_frame() {
    assert_eq!(encode_mem_kill(0x0102_0304), [b'A', b'M', 1, 7, 0x04, 0x03, 0x02, 0x01]);
    let ok = [b'A', b'M', 1, 0x87, 0, 0x04, 0x03, 0x02, 0x01];
    assert!(mem_kill_carried_out(&ok, 0x0102_0304));
    assert!(!mem_kill_carried_out(&ok, 5), "another pid's answer");
    let mut refused = ok;
    refused[4] = 2;
    assert!(!mem_kill_carried_out(&refused, 0x0102_0304));
    assert!(!mem_kill_carried_out(&ok[..8], 0x0102_0304), "short");
}

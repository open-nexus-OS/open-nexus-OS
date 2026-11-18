// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: an app's time (RFC-0120, TASK-0365 P1). execd holds the apps' right — init derived
//! it from the machine's root in grains and pinned it at `TIME_RIGHT_SLOT` — and hands every
//! app a part of it between spawn and resume: derived here, pinned into the app's own
//! `TIME_RIGHT_SLOT`, and kept in this table so the reap revokes it (its share is the apps'
//! right's again). The app holds exactly that: the kernel refuses it a bind or a derive beyond
//! the right or finer than the grain init set
//! (`KSELFTEST: test_reject_an_app_claims_more_than_app_host_holds`), so an app cannot fill the
//! kernel's tables. An app the right has nothing left for — or a machine too small for an apps'
//! right — runs in the fair class: said once, never a failure.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: boot markers (`execd: app time right ok (`, the app's own
//!   `apphost: time right (held_ppm=`); the kernel's refusals in its selftest

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use nexus_abi::time_right;

/// execd's slot of the apps' right, and the slot an app's right lands in: the fleet's one.
const SLOT: u32 = nexus_service_topology::TIME_RIGHT_SLOT;

/// The rights of the live apps: `pid << 32 | (execd's slot + 1)`, 0 free. As many rows as the
/// kernel holds rights — the grain bounds the live ones far below it.
static HELD: [AtomicU64; 64] = [const { AtomicU64::new(0) }; 64];
static GRANTED: AtomicU32 = AtomicU32::new(0);
static NONE: AtomicU32 = AtomicU32::new(0);

fn keep(pid: u32, right: u32) -> bool {
    let row = (u64::from(pid) << 32) | u64::from(right + 1);
    HELD.iter()
        .any(|cell| cell.compare_exchange(0, row, Ordering::Relaxed, Ordering::Relaxed).is_ok())
}

/// Takes a right back: revoked — whatever the app derived or bound from it ends in that step —
/// and execd's capability closed.
fn take_back(right: u32) {
    let _ = time_right::revoke(right);
    let _ = nexus_abi::cap_close(right);
}

/// Hands app `pid` its time before its first run: `share_ppm` of the apps' right, or what the
/// right still holds when that is less.
pub(crate) fn grant(pid: u32, share_ppm: u32) {
    let own = time_right::held(SLOT).map_or(0, |(_, own)| own);
    let share = share_ppm.min(own);
    let right = if share == 0 { None } else { time_right::derive(SLOT, share, 0).ok() };
    let granted = right.is_some_and(|right| {
        let kept = time_right::hand_down(right, pid, SLOT).is_ok() && keep(pid, right);
        if !kept {
            take_back(right);
        }
        kept
    });
    // RFC-0068: one line per outcome per boot (repeat spawns are silent).
    if granted {
        if GRANTED.fetch_add(1, Ordering::Relaxed) == 0 {
            let _ = nexus_abi::debug_println(&alloc::format!(
                "execd: app time right ok (pid={pid} share_ppm={share} apps_own_ppm={})",
                own - share
            ));
        }
    } else if NONE.fetch_add(1, Ordering::Relaxed) == 0 {
        let _ = nexus_abi::debug_println(&alloc::format!(
            "execd: app time right none (pid={pid} apps_own_ppm={own}): it runs fair"
        ));
    }
}

/// App `pid` was reaped: its right is revoked and its share is the apps' right's again.
pub(crate) fn end(pid: u32) {
    for cell in &HELD {
        let row = cell.load(Ordering::Relaxed);
        if row != 0 && (row >> 32) as u32 == pid {
            cell.store(0, Ordering::Relaxed);
            take_back(row as u32 - 1);
            return;
        }
    }
}

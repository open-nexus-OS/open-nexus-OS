// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Grants-before-resume family — everything execd installs into a
//! child's capability table BETWEEN spawn and `task_resume` (windowd route,
//! payload VMO, per-app event channel, demo.minidump statefs route) plus the
//! shared clone→COPY-transfer→close helper. Split out of `os_lite.rs`
//! (module-size ratchet), same move as `atlas_vmo.rs`. Discipline (#102):
//! every grant lands BEFORE resume; a post-resume transfer races the child's
//! exit (a transfer into a reaped task collapses to EPERM).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU markers (`execd: apphost windowd route granted`,
//!   `execd: app event channel granted (minted)`, `execd: minidump statefs
//!   route granted`, `APPHOST: *`) + exec-phase crash chain.

use crate::os_lite::route_ctrl;

/// execd's own capability slots holding the windowd client route for spawned
/// app processes (granted by nexus-init in the execd wiring arm, slot-order
/// convention like `LOGD_SEND_SLOT`; init logs `init: execd windowd slots`).
const APP_WINDOWD_SEND_SLOT: u32 = 8;
const APP_WINDOWD_RECV_SLOT: u32 = 9;
/// The child slots the app-host expects them in (its fixed constants).
const CHILD_WINDOWD_SEND_SLOT: u32 = 5;
const CHILD_WINDOWD_RECV_SLOT: u32 = 6;
/// The child slot receiving the payload VMO (app-host's fixed constant).
const CHILD_PAYLOAD_SLOT: u32 = 7;
/// ADR-0042 per-app event channel (init-minted pair; slot-order contract,
/// proven by `init: execd app-event slots send=0xb recv=0xc`): windowd gets
/// a SEND clone (`OP_SURFACE_EVENTS`, cap-move) and delivers input events +
/// surface acks on it; the child gets a RECV clone. Replaces the shared
/// `window_rsp` delivery, which raced with inputd's ack drain (taps were
/// consumed by inputd before the app ever saw them).
// Per-app event channels are minted DYNAMICALLY per launch via init's ctrl
// plane (`@mint-pair`): init — the EndpointFactory holder — mints a fresh
// PRIVATE pair on demand; execd does mint→grant→close (zero cap-table
// accumulation). No static pair, no pool sizing, no slot-order contract —
// the whole "adjust the pool after every feature" class is retired.
/// The child slot receiving the event-channel RECV (app-host's constant).
const CHILD_EVENTS_SLOT: u32 = 8;
/// TASK-0049 reanimation: statefs route slots for the `demo.minidump`
/// payload (SSOT: `userspace/apps/demo-exit0/build.rs` STATEFS_SEND_SLOT /
/// STATEFS_RECV_SLOT). Numerically these overlap CHILD_PAYLOAD_SLOT /
/// CHILD_EVENTS_SLOT, which is safe: those are only ever granted to
/// IMG_APPHOST children, this pair only to IMG_EXIT42 children.
const CHILD_MINIDUMP_STATEFS_SEND_SLOT: u32 = 7;
/// RECV half of the minidump payload's statefs route (see above).
const CHILD_MINIDUMP_STATEFS_RECV_SLOT: u32 = 8;
/// SEND clone of the child's OWN event channel (it attaches this to windowd
/// itself, nonce-tagged). After the service SEND slots 11..13 (nexus-sdk-routes).
const CHILD_EVENTS_SEND_SLOT: u32 = 14;
/// Hands the child its RECV half of the dedicated event channel
/// (`CHILD_EVENTS_SLOT`).
pub(crate) fn grant_event_channel(child_pid: u32) {
    // Mint a fresh PRIVATE pair via init's ctrl plane (`@mint-pair`); the
    // child gets BOTH halves: RECV→slot 8 (its event inbox) and a SEND clone→
    // slot 14 — the child attaches that to windowd ITSELF, tagged with its
    // nonce (deterministic channel↔surface binding). execd closes its own
    // halves after the grants: mint→grant→close, zero accumulation.
    let Some((send_slot, recv_slot)) = route_ctrl(b"@mint-pair") else {
        let _ = nexus_abi::debug_println("execd: FAIL app event channel mint");
        return;
    };
    // NOTE: `cap_transfer_to_slot` COPIES, so `grant_clone` closes each clone
    // after the transfer — else execd retains a live SEND cap to every child's
    // event channel, which both LEAKS a cap per launch AND keeps the RFC-0079
    // last-sender scan non-zero (the window-close EOF never fires).
    let recv_ok = grant_clone(child_pid, recv_slot, nexus_abi::Rights::RECV, CHILD_EVENTS_SLOT);
    let send_ok =
        grant_clone(child_pid, send_slot, nexus_abi::Rights::SEND, CHILD_EVENTS_SEND_SLOT);
    // Close execd's own minted halves too (the child holds the live ones).
    let _ = nexus_abi::cap_close(send_slot);
    let _ = nexus_abi::cap_close(recv_slot);
    if recv_ok && send_ok {
        let _ = nexus_abi::debug_println("execd: app event channel granted (minted)");
    } else {
        let _ = nexus_abi::debug_println("execd: FAIL app event channel grant (minted)");
    }
}
/// Moves the payload VMO into the child's fixed payload slot
/// (`CHILD_PAYLOAD_SLOT`, Rights::MAP — `vmo_read` is all the child needs).
pub(crate) fn grant_payload_vmo(child_pid: u32, vmo: u32) {
    match nexus_abi::cap_transfer_to_slot(
        child_pid as nexus_abi::Pid,
        vmo,
        nexus_abi::Rights::MAP,
        CHILD_PAYLOAD_SLOT,
    ) {
        Ok(_) => {
            let _ = nexus_abi::debug_println("execd: app payload granted");
        }
        Err(_) => {
            let _ = nexus_abi::debug_println("execd: FAIL app payload grant");
            let _ = nexus_abi::cap_close(vmo);
        }
    }
}
/// ADR-0042: hands the spawned app process its windowd client route —
/// clones of execd's own granted caps, placed into the child's FIXED slots
/// (the app-host constants). The child retries its first sends bounded, so
/// the transfer landing moments after resume is safe (#123 lesson).
pub(crate) fn grant_windowd_route(child_pid: u32) {
    let send = nexus_abi::cap_clone(APP_WINDOWD_SEND_SLOT)
        .and_then(|clone| {
            nexus_abi::cap_transfer_to_slot(
                child_pid as nexus_abi::Pid,
                clone,
                nexus_abi::Rights::SEND,
                CHILD_WINDOWD_SEND_SLOT,
            )
            .map_err(|_| nexus_abi::AbiError::Unsupported)
        })
        .is_ok();
    let recv = nexus_abi::cap_clone(APP_WINDOWD_RECV_SLOT)
        .and_then(|clone| {
            nexus_abi::cap_transfer_to_slot(
                child_pid as nexus_abi::Pid,
                clone,
                nexus_abi::Rights::RECV,
                CHILD_WINDOWD_RECV_SLOT,
            )
            .map_err(|_| nexus_abi::AbiError::Unsupported)
        })
        .is_ok();
    if send && recv {
        let _ = nexus_abi::debug_println("execd: apphost windowd route granted");
    } else {
        // Values, not guesses: the probe's send retries will exhaust next.
        let _ = nexus_abi::debug_println("execd: FAIL apphost windowd route grant");
    }
}
/// TASK-0049 reanimation: grants the statefsd route into a `demo.minidump`
/// child (slots 7/8, payload SSOT `userspace/apps/demo-exit0/build.rs`) so
/// the payload can PUT its own dump before `exit(42)`. Bring-up note: the
/// child shares execd's statefsd reply queue for its single PUT — the same
/// sharing the retired selftest-side grant had; execd issues no statefs
/// traffic in that window.
pub(crate) fn grant_minidump_statefs_route(child_pid: u32) {
    let Some((send_slot, recv_slot)) = route_ctrl(b"statefsd") else {
        let _ = nexus_abi::debug_println("execd: FAIL minidump statefs route");
        return;
    };
    let send_ok = grant_clone(
        child_pid,
        send_slot,
        nexus_abi::Rights::SEND,
        CHILD_MINIDUMP_STATEFS_SEND_SLOT,
    );
    let recv_ok = grant_clone(
        child_pid,
        recv_slot,
        nexus_abi::Rights::RECV,
        CHILD_MINIDUMP_STATEFS_RECV_SLOT,
    );
    if send_ok && recv_ok {
        let _ = nexus_abi::debug_println("execd: minidump statefs route granted");
    } else {
        let _ = nexus_abi::debug_println("execd: FAIL minidump statefs grant");
    }
}
/// Clones `src_slot` and transfers the clone into the child's `child_slot`
/// with `rights`; returns whether the grant landed.
pub(crate) fn grant_clone(
    child_pid: u32,
    src_slot: u32,
    rights: nexus_abi::Rights,
    child_slot: u32,
) -> bool {
    // `cap_transfer_to_slot` COPIES, so close execd's clone after the transfer
    // — otherwise execd accumulates a live cap per grant (RFC-0079: a retained
    // SEND cap to a child's event channel keeps the last-sender scan non-zero
    // and the window-close EOF never fires).
    let Ok(clone) = nexus_abi::cap_clone(src_slot) else {
        return false;
    };
    let ok =
        nexus_abi::cap_transfer_to_slot(child_pid as nexus_abi::Pid, clone, rights, child_slot)
            .is_ok();
    let _ = nexus_abi::cap_close(clone);
    ok
}

// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Phase 3 of 12 — ota (TASK-0007 A/B normalize → stage → switch →
//!   health → rollback cycle → bootctl persist).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os) — OTA state-machine slice.
//!
//! Extracted in Cut P2-06 of TASK-0023B. Marker order and marker strings are
//! byte-identical to the pre-cut body. Reply-pump correlation (RFC-0019
//! nonce-correlated `updated_pending`) is preserved by routing every
//! `updated::*` call through the same `ctx.updated_pending` queue.
//!
//! `bundlemgrd` and `updated` handles are local to this phase; the policy
//! slice (later P2-07) re-resolves them via the silent `route_with_retry`.
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

use nexus_abi::yield_;

use crate::markers::emit_line;
use crate::os_lite::context::PhaseCtx;
use crate::os_lite::ipc::routing::route_with_retry;
use crate::os_lite::probes::reset::bootctl_call_raw;
use crate::os_lite::{services, updated};

/// bootctld wire op for a quorum health report (RFC-0089 §13).
const BOOTCTL_OP_HEALTH_OK: u8 = 3;
const BOOTCTL_STATUS_OK: u8 = 0;

/// Health-commit v2: the selftest is a DECLARED quorum reporter — its
/// direct confirmation plus updated's pass-through (init_health_ok) form
/// the full mask; commit fires only when both landed.
fn selftest_quorum_report() -> core::result::Result<(), ()> {
    match bootctl_call_raw(&[b'B', b'T', 1, BOOTCTL_OP_HEALTH_OK], BOOTCTL_OP_HEALTH_OK) {
        Some((BOOTCTL_STATUS_OK, _)) => Ok(()),
        _ => Err(()),
    }
}

/// TASK-0140: the page's read surface — status/feed/check answer, the
/// seeded feed is non-empty, and updated's forwarded active slot matches
/// a DIRECT authority read (both wires encode a=1/b=2). State-neutral.
fn updates_surface_verdict(ctx: &mut PhaseCtx, updated_client: &nexus_ipc::KernelClient) -> bool {
    let Ok((active, _pending, _tries, _healthy)) = updated::updated_get_status(
        updated_client,
        ctx.reply_send_slot,
        ctx.reply_recv_slot,
        &mut ctx.updated_pending,
    ) else {
        return false;
    };
    let Ok(feed) = updated::updated_feed_count(
        updated_client,
        ctx.reply_send_slot,
        ctx.reply_recv_slot,
        &mut ctx.updated_pending,
    ) else {
        return false;
    };
    let Ok(check) = updated::updated_check_count(
        updated_client,
        ctx.reply_send_slot,
        ctx.reply_recv_slot,
        &mut ctx.updated_pending,
    ) else {
        return false;
    };
    if feed == 0 || check == 0 {
        return false;
    }
    let frame = [b'B', b'T', 1, 4u8]; // bootctld OP_GET_STATUS
    let mut status = [0u8; 4];
    match crate::os_lite::probes::reset::bootctl_call_payload(&frame, 4, &mut status) {
        Some((0, n)) if n >= 1 => {
            let forwarded = match active {
                updated::SlotId::A => 1,
                updated::SlotId::B => 2,
            };
            status[0] == forwarded
        }
        _ => false,
    }
}

pub(crate) fn run(ctx: &mut PhaseCtx) -> core::result::Result<(), ()> {
    // TASK-0289 B1: the measured-boot surface — probed FIRST, before this
    // phase mutates the authority: the cross-check (measured slot ==
    // active slot) is a statement about THIS BOOT, and the stage/switch
    // cycle below legitimately moves the active slot without a reboot
    // (probing after it compared boot evidence against post-OTA state
    // and failed honestly). The record travelled
    // nxboot -> handoff page -> kernel capture -> bootctld; the probe
    // CROSS-CHECKS it against the authority's own status (active slot)
    // instead of merely proving the plumbing echoes bytes. Absent is an
    // honest verdict of its own (direct-kernel dev boots have no loader)
    // and is never accepted silently on the nxboot path — the harness
    // requires the ok marker in proof lanes.
    {
        let frame = [b'B', b'T', 1, 12u8]; // OP_GET_MEASURED
        let mut measured = [0u8; 61];
        let mut status = [0u8; 4];
        let first = crate::os_lite::probes::reset::bootctl_call_payload(&frame, 12, &mut measured);
        let verdict = match first {
            Some((0, len)) if len >= 61 && measured[0] == 1 => {
                // Raw handoff layout (ADR-0059, +1 for the present byte):
                // slot at [11], validated by the kernel's CRC check.
                let status_frame = [b'B', b'T', 1, 4u8]; // OP_GET_STATUS
                match crate::os_lite::probes::reset::bootctl_call_payload(
                    &status_frame,
                    4,
                    &mut status,
                ) {
                    // Status encodes a=1/b=2; the handoff encodes a=0/b=1.
                    Some((0, slen)) if slen >= 1 => Some(status[0] == measured[11] + 1),
                    _ => Some(false),
                }
            }
            Some((0, len)) if len >= 1 && measured[0] == 0 => None,
            _ => Some(false),
        };
        match verdict {
            Some(true) => emit_line(crate::markers::M_SELFTEST_MEASURED_BOOT_LOG_OK),
            Some(false) => {
                emit_measured_dbg(first, &measured, &status);
                emit_line(crate::markers::M_SELFTEST_MEASURED_BOOT_LOG_FAIL)
            }
            None => emit_line(crate::markers::M_SELFTEST_MEASURED_BOOT_LOG_ABSENT_DIRECT_KERNEL),
        }
    }

    // TASK-0315: cross-partition deny (state write without a grant) —
    // late in the ladder so virtioblkd is long serving.
    crate::os_lite::probes::blkgate::blk_cross_partition_deny_proof();
    // Fail-closed + LOUD: a silent `map_err(|_| ())` here swallowed a routing
    // failure and made the whole OTA phase vanish with no marker (the ladder saw
    // a missing `bundlemgrd: slot a active` with no cause). Name the failing route
    // so the next boot says exactly which resolution failed.
    let bundlemgrd = route_with_retry("bundlemgrd").map_err(|_| {
        emit_line(crate::markers::M_SELFTEST_OTA_ROUTE_FAIL_SVC_BUNDLEMGRD);
    })?;
    let updated = route_with_retry("updated").map_err(|_| {
        emit_line(crate::markers::M_SELFTEST_OTA_ROUTE_FAIL_SVC_UPDATED);
    })?;

    // TASK-0140: the Settings Updates page's READ surface, live and
    // state-neutral (before this phase mutates anything): status, feed and
    // check answer, the seeded feed is non-empty, and updated's active
    // slot equals the boot authority's own — the page renders exactly
    // this surface, so its coherence is the honest "page truth" proof the
    // headless ladder can give without input injection.
    {
        let verdict = updates_surface_verdict(ctx, &updated);
        if verdict {
            emit_line(crate::markers::M_SELFTEST_UPDATES_SURFACE_OK);
        } else {
            emit_line(crate::markers::M_SELFTEST_UPDATES_SURFACE_FAIL);
        }
    }

    // TASK-0007: updated stage/switch/rollback (non-persistent A/B skeleton).
    // Fail-closed + LOUD: this activation emits `bundlemgrd: slot a active` (the
    // ladder's required marker) IFF the request reaches bundlemgrd. A silent
    // `let _ =` here hid a delivery failure (route resolved but the send never
    // reached bundlemgrd's serving endpoint) — name it so the cause is visible.
    if services::bundlemgrd::bundlemgrd_v1_set_active_slot(&bundlemgrd, 1).is_err() {
        emit_line(crate::markers::M_SELFTEST_OTA_SLOT_ACTIVATE_FAIL_SVC_BUNDLEMGRD);
    }
    // Determinism: updated bootctrl state is persisted via statefs and may survive across runs.
    // Normalize to active-slot A before the OTA flow so rollback assertions are stable.
    if let Ok((_active, pending_slot, _tries_left, _health_ok)) = updated::updated_get_status(
        &updated,
        ctx.reply_send_slot,
        ctx.reply_recv_slot,
        &mut ctx.updated_pending,
    ) {
        if pending_slot.is_some() {
            // Clear a pending state from a prior run (bounded).
            for _ in 0..4 {
                let _ = updated::updated_boot_attempt(
                    &updated,
                    ctx.reply_send_slot,
                    ctx.reply_recv_slot,
                    &mut ctx.updated_pending,
                );
                if let Ok((_a, p, _t, _h)) = updated::updated_get_status(
                    &updated,
                    ctx.reply_send_slot,
                    ctx.reply_recv_slot,
                    &mut ctx.updated_pending,
                ) {
                    if p.is_none() {
                        break;
                    }
                }
                let _ = yield_();
            }
        }
    }
    normalize_active_to_a(ctx, &updated);
    // TASK-0198 Phase 1 deny lane FIRST (state-neutral: a rejected stage
    // mutates nothing): a validly self-signed archive whose publisher is not
    // in the device anchor must come back FAILED with the untrusted-publisher
    // reject — proving the anchor is enforced before the happy path runs.
    if updated::updated_stage_untrusted_deny(
        &updated,
        ctx.reply_send_slot,
        ctx.reply_recv_slot,
        &mut ctx.updated_pending,
    )
    .is_ok()
    {
        emit_line(crate::markers::M_SELFTEST_UPDATES_TRUST_REJECT_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_UPDATES_TRUST_REJECT_FAIL);
    }
    // TASK-0179 deny lanes (state-neutral: a rejected stage mutates
    // nothing) — each proves ONE stable reject reason of the apply engine.
    if updated::updated_stage_deny(
        &updated,
        ctx.reply_send_slot,
        ctx.reply_recv_slot,
        &mut ctx.updated_pending,
        updated::TAMPERED_PATH,
        updated::REJECT_DIGEST,
    )
    .is_ok()
    {
        emit_line(crate::markers::M_SELFTEST_OTA_TAMPER_DENY_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_OTA_TAMPER_DENY_FAIL);
    }
    if updated::updated_stage_deny(
        &updated,
        ctx.reply_send_slot,
        ctx.reply_recv_slot,
        &mut ctx.updated_pending,
        updated::DOWNGRADE_PATH,
        updated::REJECT_DOWNGRADE,
    )
    .is_ok()
    {
        emit_line(crate::markers::M_SELFTEST_OTA_DOWNGRADE_DENY_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_OTA_DOWNGRADE_DENY_FAIL);
    }
    if updated::updated_stage(
        &updated,
        ctx.reply_send_slot,
        ctx.reply_recv_slot,
        &mut ctx.updated_pending,
    )
    .is_ok()
    {
        emit_line(crate::markers::M_SELFTEST_OTA_STAGE_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_OTA_STAGE_FAIL);
    }
    if updated::updated_switch(
        &updated,
        ctx.reply_send_slot,
        ctx.reply_recv_slot,
        2,
        &mut ctx.updated_pending,
    )
    .is_ok()
    {
        emit_line(crate::markers::M_SELFTEST_OTA_SWITCH_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_OTA_SWITCH_FAIL);
    }
    if services::bundlemgrd::bundlemgrd_v1_fetch_image_slot(&bundlemgrd, Some(b'b')).is_ok() {
        emit_line(crate::markers::M_SELFTEST_OTA_PUBLISH_B_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_OTA_PUBLISH_B_FAIL);
    }
    // Health-commit v2 (RFC-0089 §13): the selftest confirms its declared
    // quorum bit directly at bootctld, then init's signal arrives through
    // updated — commit fires only when BOTH landed (mask complete).
    let quorum_direct = selftest_quorum_report();
    if updated::init_health_ok().is_ok() {
        emit_line(crate::markers::M_SELFTEST_OTA_HEALTH_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_OTA_HEALTH_FAIL);
    }
    // Verify the commit actually landed (health_ok in the authority's
    // status) — a quorum that never completes must be LOUD here.
    let committed = matches!(
        updated::updated_get_status(
            &updated,
            ctx.reply_send_slot,
            ctx.reply_recv_slot,
            &mut ctx.updated_pending,
        ),
        Ok((_a, None, _t, true))
    );
    if quorum_direct.is_ok() && committed {
        emit_line(crate::markers::M_SELFTEST_BOOTCTL_QUORUM_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_BOOTCTL_QUORUM_FAIL);
    }
    // Second cycle to force rollback (tries_left=1).
    if updated::updated_stage(
        &updated,
        ctx.reply_send_slot,
        ctx.reply_recv_slot,
        &mut ctx.updated_pending,
    )
    .is_ok()
    {
        // Determinism: rollback target is the slot that was active *before* the switch.
        let expected_rollback = updated::updated_get_status(
            &updated,
            ctx.reply_send_slot,
            ctx.reply_recv_slot,
            &mut ctx.updated_pending,
        )
        .ok()
        .map(|(active, _pending, _tries_left, _health_ok)| active);
        if updated::updated_switch(
            &updated,
            ctx.reply_send_slot,
            ctx.reply_recv_slot,
            1,
            &mut ctx.updated_pending,
        )
        .is_ok()
        {
            let got = updated::updated_boot_attempt(
                &updated,
                ctx.reply_send_slot,
                ctx.reply_recv_slot,
                &mut ctx.updated_pending,
            );
            match (expected_rollback, got) {
                (Some(expected), Ok(Some(slot))) if slot == expected => {
                    emit_line(crate::markers::M_SELFTEST_OTA_ROLLBACK_OK)
                }
                (None, Ok(Some(_slot))) => emit_line(crate::markers::M_SELFTEST_OTA_ROLLBACK_OK),
                _ => emit_line(crate::markers::M_SELFTEST_OTA_ROLLBACK_FAIL),
            }
        } else {
            emit_line(crate::markers::M_SELFTEST_OTA_ROLLBACK_FAIL);
        }
    } else {
        emit_line(crate::markers::M_SELFTEST_OTA_ROLLBACK_FAIL);
    }

    if services::bootctl::bootctl_persist_check().is_ok() {
        emit_line(crate::markers::M_SELFTEST_BOOTCTL_PERSIST_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_BOOTCTL_PERSIST_FAIL);
    }

    // TASK-0036-B: the BSB projection must be live — GET_STATUS carries the
    // synced flag + last projected seq as an additive tail. seq >= 2 proves
    // a RUNTIME projection happened this session (factory seeds seq=1), not
    // just a read of the factory block.
    {
        let frame = [b'B', b'T', 1, 4u8]; // OP_GET_STATUS
        let mut payload = [0u8; 16];
        let verdict =
            match crate::os_lite::probes::reset::bootctl_call_payload(&frame, 4, &mut payload) {
                Some((0, len)) if len >= 13 && payload[4] == 1 => {
                    let mut seq_bytes = [0u8; 8];
                    seq_bytes.copy_from_slice(&payload[5..13]);
                    u64::from_le_bytes(seq_bytes) >= 2
                }
                _ => false,
            };
        if verdict {
            emit_line(crate::markers::M_SELFTEST_BOOTCTL_BSB_OK);
        } else {
            emit_line(crate::markers::M_SELFTEST_BOOTCTL_BSB_FAIL);
        }
    }

    // TASK-0036-B: leave the persisted state (and thus the projected BSB)
    // on the real bootable slot — see normalize_active_to_a.
    normalize_active_to_a(ctx, &updated);

    let _ = (bundlemgrd, updated);
    Ok(())
}

/// Flips the persisted slot state back to active-A when a prior cycle (or
/// this one) left it on B. Called at phase START (stable assertions) and
/// at phase END (TASK-0036-B: the BSB now PROJECTS this state and the
/// loader OBEYS it on the next boot — leaving the record on the imageless
/// slot B would send every subsequent boot through the fallback path).
fn normalize_active_to_a(ctx: &mut PhaseCtx, updated: &nexus_ipc::KernelClient) {
    if let Ok((active, _pending, _tries_left, _health_ok)) = updated::updated_get_status(
        updated,
        ctx.reply_send_slot,
        ctx.reply_recv_slot,
        &mut ctx.updated_pending,
    ) {
        if active == updated::SlotId::B {
            // Flip B -> A (bounded), same tries as the real flow.
            for _ in 0..2 {
                if updated::updated_stage(
                    updated,
                    ctx.reply_send_slot,
                    ctx.reply_recv_slot,
                    &mut ctx.updated_pending,
                )
                .is_err()
                {
                    break;
                }
                let _ = updated::updated_switch(
                    updated,
                    ctx.reply_send_slot,
                    ctx.reply_recv_slot,
                    2,
                    &mut ctx.updated_pending,
                );
                // Quorum v2: both declared reporters must confirm.
                let _ = selftest_quorum_report();
                let _ = updated::init_health_ok();
                if let Ok((a, _p, _t, _h)) = updated::updated_get_status(
                    updated,
                    ctx.reply_send_slot,
                    ctx.reply_recv_slot,
                    &mut ctx.updated_pending,
                ) {
                    if a == updated::SlotId::A {
                        break;
                    }
                }
                let _ = yield_();
            }
        }
    }
}

/// Diagnostic companion of the measured-boot FAIL verdict (TASK-0289 B1):
/// one line naming which hop broke — transport (`rs=9`), reply status,
/// payload length, present flag, and the slot pair the cross-check saw.
/// Fixed-buffer render, single digits (lengths clamp at 9).
fn emit_measured_dbg(first: Option<(u8, usize)>, measured: &[u8; 61], status: &[u8; 4]) {
    let mut line = *b"measured-dbg: rs=9 len=9 pr=9 sl=9 ac=9";
    let digit = |v: usize| b'0' + (v.min(9) as u8);
    if let Some((st, len)) = first {
        line[17] = digit(st as usize);
        line[23] = digit(len / 10);
        line[28] = digit(measured[0] as usize);
        line[33] = digit(measured[11] as usize);
        line[38] = digit(status[0] as usize);
    }
    if let Ok(msg) = core::str::from_utf8(&line[..]) {
        emit_line(msg);
    }
}

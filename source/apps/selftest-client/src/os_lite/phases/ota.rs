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

pub(crate) fn run(ctx: &mut PhaseCtx) -> core::result::Result<(), ()> {
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
    if let Ok((active, _pending, _tries_left, _health_ok)) = updated::updated_get_status(
        &updated,
        ctx.reply_send_slot,
        ctx.reply_recv_slot,
        &mut ctx.updated_pending,
    ) {
        if active == updated::SlotId::B {
            // Flip B -> A (bounded) so the following tests always stage/switch to B.
            // Use the same tries_left as the real flow to avoid corner-cases in BootCtrl.
            for _ in 0..2 {
                if updated::updated_stage(
                    &updated,
                    ctx.reply_send_slot,
                    ctx.reply_recv_slot,
                    &mut ctx.updated_pending,
                )
                .is_err()
                {
                    break;
                }
                let _ = updated::updated_switch(
                    &updated,
                    ctx.reply_send_slot,
                    ctx.reply_recv_slot,
                    2,
                    &mut ctx.updated_pending,
                );
                // Quorum v2: both declared reporters must confirm.
                let _ = selftest_quorum_report();
                let _ = updated::init_health_ok();
                if let Ok((a, _p, _t, _h)) = updated::updated_get_status(
                    &updated,
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

    let _ = (bundlemgrd, updated);
    Ok(())
}

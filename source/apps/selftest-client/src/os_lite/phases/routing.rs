// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Phase 2 of 12 — routing (policyd routing, bundlemgrd routing,
//!   updated routing, updated log probe, bundlemgrd v1 list/image/malformed).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os) — routing slice.
//!
//! Extracted in Cut P2-05 of TASK-0023B. Marker order and marker strings are
//! byte-identical to the pre-cut body. `policyd`, `bundlemgrd`, and `updated`
//! handles are local to this phase and dropped at end-of-phase. Downstream
//! phases (ota, policy) re-resolve via `route_with_retry`; that call is silent
//! (no markers), so the marker ladder is preserved.
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

use nexus_ipc::{Client, Wait as IpcWait};

use crate::markers::{emit_byte, emit_bytes, emit_hex_u64, emit_line};
use crate::os_lite::context::PhaseCtx;
use crate::os_lite::ipc::routing::route_with_retry;
use crate::os_lite::{services, updated};

pub(crate) fn run(ctx: &mut PhaseCtx) -> core::result::Result<(), ()> {
    // Policy E2E via policyd (minimal IPC protocol).
    let policyd = match route_with_retry("policyd") {
        Ok(client) => client,
        Err(_) => return Err(()),
    };
    emit_line(crate::markers::M_SELFTEST_IPC_ROUTING_POLICYD_OK);
    let bundlemgrd = match route_with_retry("bundlemgrd") {
        Ok(client) => client,
        Err(_) => return Err(()),
    };
    let (bnd_send, bnd_recv) = bundlemgrd.slots();
    emit_bytes(crate::markers::M_SELFTEST_BUNDLEMGRD_SLOTS.as_bytes());
    emit_hex_u64(bnd_send as u64);
    emit_byte(b' ');
    emit_hex_u64(bnd_recv as u64);
    emit_byte(b'\n');
    emit_line(crate::markers::M_SELFTEST_IPC_ROUTING_BUNDLEMGRD_OK);
    let updated = match route_with_retry("updated") {
        Ok(client) => client,
        Err(_) => return Err(()),
    };
    let (upd_send, upd_recv) = updated.slots();
    emit_bytes(crate::markers::M_SELFTEST_UPDATED_SLOTS.as_bytes());
    emit_hex_u64(upd_send as u64);
    emit_byte(b' ');
    emit_hex_u64(upd_recv as u64);
    emit_byte(b'\n');
    emit_line(crate::markers::M_SELFTEST_IPC_ROUTING_UPDATED_OK);
    if updated::updated_log_probe(
        &updated,
        ctx.reply_send_slot,
        ctx.reply_recv_slot,
        &mut ctx.updated_pending,
    )
    .is_ok()
    {
        emit_line(crate::markers::M_SELFTEST_UPDATED_PROBE_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_UPDATED_PROBE_FAIL);
    }
    let (st, count) = services::bundlemgrd::bundlemgrd_v1_list(&bundlemgrd)?;
    if st == 0 && count == 1 {
        emit_line(crate::markers::M_SELFTEST_BUNDLEMGRD_V1_LIST_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_BUNDLEMGRD_V1_LIST_FAIL);
    }
    if services::bundlemgrd::bundlemgrd_v1_fetch_image(&bundlemgrd).is_ok() {
        emit_line(crate::markers::M_SELFTEST_BUNDLEMGRD_V1_IMAGE_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_BUNDLEMGRD_V1_IMAGE_FAIL);
    }
    bundlemgrd
        .send(b"bad", IpcWait::Timeout(core::time::Duration::from_millis(100)))
        .map_err(|_| ())?;
    let rsp = bundlemgrd
        .recv(IpcWait::Timeout(core::time::Duration::from_millis(100)))
        .map_err(|_| ())?;
    if rsp.len() == 8 && rsp[0] == b'B' && rsp[1] == b'N' && rsp[2] == 1 && rsp[4] != 0 {
        emit_line(crate::markers::M_SELFTEST_BUNDLEMGRD_V1_MALFORMED_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_BUNDLEMGRD_V1_MALFORMED_FAIL);
    }

    // `policyd`/`bundlemgrd`/`updated` are dropped at end-of-phase. Downstream
    // phases re-resolve via the silent `route_with_retry` (no marker change).
    let _ = (policyd, bundlemgrd, updated);
    Ok(())
}

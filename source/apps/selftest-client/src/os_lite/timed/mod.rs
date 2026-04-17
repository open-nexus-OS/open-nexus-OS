// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: timed (kernel timer) coalescing/cancel/sleep selftest probes.
//! Extracted verbatim from the previous monolithic `os_lite/mod.rs` block
//! (TASK-0023B / RFC-0038 phase 1, cut 8). No behavior, marker, or reject-path
//! change.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal (binary crate)
//! TEST_COVERAGE: QEMU marker ladder via `just test-os`.
//! ADR: docs/adr/0017-service-architecture.md, docs/rfcs/RFC-0038-*.md

extern crate alloc;

use alloc::vec::Vec;

use nexus_abi::QosClass;
use nexus_ipc::{Client, KernelClient, Wait as IpcWait};

use super::ipc::routing::route_with_retry;

fn timed_align_up(deadline_ns: u64, window_ns: u64) -> Option<u64> {
    if window_ns == 0 {
        return Some(deadline_ns);
    }
    let rem = deadline_ns % window_ns;
    if rem == 0 {
        Some(deadline_ns)
    } else {
        deadline_ns.checked_add(window_ns - rem)
    }
}

fn timed_register(
    client: &KernelClient,
    nonce: u32,
    qos_raw: u8,
    deadline_ns: u64,
) -> core::result::Result<(u8, u32, u64), ()> {
    let mut req = [0u8; 18];
    req[0] = b'T';
    req[1] = b'M';
    req[2] = 1;
    req[3] = 1; // OP_REGISTER
    req[4..8].copy_from_slice(&nonce.to_le_bytes());
    req[8] = qos_raw;
    req[9] = 0;
    req[10..18].copy_from_slice(&deadline_ns.to_le_bytes());
    if client.send(&req, IpcWait::Timeout(core::time::Duration::from_millis(200))).is_err() {
        return Err(());
    }
    let rsp = match client.recv(IpcWait::Timeout(core::time::Duration::from_millis(200))) {
        Ok(v) => v,
        Err(_) => return Err(()),
    };
    if rsp.len() != 21 || rsp[0] != b'T' || rsp[1] != b'M' || rsp[2] != 1 || rsp[3] != (1 | 0x80) {
        return Err(());
    }
    let got_nonce = u32::from_le_bytes([rsp[5], rsp[6], rsp[7], rsp[8]]);
    if got_nonce != nonce {
        return Err(());
    }
    let status = rsp[4];
    let timer_id = u32::from_le_bytes([rsp[9], rsp[10], rsp[11], rsp[12]]);
    let coalesced = u64::from_le_bytes([
        rsp[13], rsp[14], rsp[15], rsp[16], rsp[17], rsp[18], rsp[19], rsp[20],
    ]);
    Ok((status, timer_id, coalesced))
}

fn timed_cancel(client: &KernelClient, nonce: u32, timer_id: u32) -> core::result::Result<u8, ()> {
    let mut req = [0u8; 12];
    req[0] = b'T';
    req[1] = b'M';
    req[2] = 1;
    req[3] = 2; // OP_CANCEL
    req[4..8].copy_from_slice(&nonce.to_le_bytes());
    req[8..12].copy_from_slice(&timer_id.to_le_bytes());
    if client.send(&req, IpcWait::Timeout(core::time::Duration::from_millis(200))).is_err() {
        return Err(());
    }
    let rsp = match client.recv(IpcWait::Timeout(core::time::Duration::from_millis(200))) {
        Ok(v) => v,
        Err(_) => return Err(()),
    };
    if rsp.len() != 9 || rsp[0] != b'T' || rsp[1] != b'M' || rsp[2] != 1 || rsp[3] != (2 | 0x80) {
        return Err(());
    }
    let got_nonce = u32::from_le_bytes([rsp[5], rsp[6], rsp[7], rsp[8]]);
    if got_nonce != nonce {
        return Err(());
    }
    Ok(rsp[4])
}

fn timed_sleep_until(
    client: &KernelClient,
    nonce: u32,
    qos_raw: u8,
    deadline_ns: u64,
) -> core::result::Result<(u8, u64), ()> {
    let mut req = [0u8; 18];
    req[0] = b'T';
    req[1] = b'M';
    req[2] = 1;
    req[3] = 3; // OP_SLEEP_UNTIL
    req[4..8].copy_from_slice(&nonce.to_le_bytes());
    req[8] = qos_raw;
    req[9] = 0;
    req[10..18].copy_from_slice(&deadline_ns.to_le_bytes());
    if client.send(&req, IpcWait::Timeout(core::time::Duration::from_millis(250))).is_err() {
        return Err(());
    }
    let rsp = match client.recv(IpcWait::Timeout(core::time::Duration::from_millis(250))) {
        Ok(v) => v,
        Err(_) => return Err(()),
    };
    if rsp.len() != 17 || rsp[0] != b'T' || rsp[1] != b'M' || rsp[2] != 1 || rsp[3] != (3 | 0x80) {
        return Err(());
    }
    let got_nonce = u32::from_le_bytes([rsp[5], rsp[6], rsp[7], rsp[8]]);
    if got_nonce != nonce {
        return Err(());
    }
    let status = rsp[4];
    let wake_ns =
        u64::from_le_bytes([rsp[9], rsp[10], rsp[11], rsp[12], rsp[13], rsp[14], rsp[15], rsp[16]]);
    Ok((status, wake_ns))
}

fn timed_fail(code: u64) -> core::result::Result<(), ()> {
    let _ = code;
    Err(())
}

pub(crate) fn timed_coalesce_probe() -> core::result::Result<(), ()> {
    const STATUS_OK: u8 = 0;
    const STATUS_INVALID_ARGS: u8 = 1;
    const STATUS_OVER_LIMIT: u8 = 2;
    let timed = match route_with_retry("timed") {
        Ok(v) => v,
        Err(_) => return timed_fail(0x01),
    };
    let now = match nexus_abi::nsec() {
        Ok(v) => v,
        Err(_) => return timed_fail(0x02),
    };

    let d1 = now.saturating_add(20_500_000);
    let (st1, id1, c1) = match timed_register(&timed, 0x5449_0001, QosClass::Normal as u8, d1) {
        Ok(v) => v,
        Err(_) => return timed_fail(0x11),
    };
    let expect1 = match timed_align_up(d1, 4_000_000) {
        Some(v) => v,
        None => return timed_fail(0x12),
    };
    if st1 != STATUS_OK || c1 != expect1 || id1 == 0 {
        return timed_fail(0x13);
    }

    let d2 = now.saturating_add(21_100_000);
    let (st2, id2, c2) = match timed_register(&timed, 0x5449_0002, QosClass::Interactive as u8, d2)
    {
        Ok(v) => v,
        Err(_) => return timed_fail(0x21),
    };
    let expect2 = match timed_align_up(d2, 1_000_000) {
        Some(v) => v,
        None => return timed_fail(0x22),
    };
    if st2 != STATUS_OK || c2 != expect2 || id2 == 0 {
        return timed_fail(0x23);
    }

    let sleep_deadline = now.saturating_add(2_100_000);
    let (sleep_st, woke_ns) =
        match timed_sleep_until(&timed, 0x5449_0003, QosClass::Interactive as u8, sleep_deadline) {
            Ok(v) => v,
            Err(_) => return timed_fail(0x31),
        };
    let sleep_expect = match timed_align_up(sleep_deadline, 1_000_000) {
        Some(v) => v,
        None => return timed_fail(0x32),
    };
    if sleep_st != STATUS_OK || woke_ns < sleep_expect {
        return timed_fail(0x33);
    }

    let cancel_1 = match timed_cancel(&timed, 0x5449_0004, id1) {
        Ok(v) => v,
        Err(_) => return timed_fail(0x41),
    };
    let cancel_2 = match timed_cancel(&timed, 0x5449_0005, id2) {
        Ok(v) => v,
        Err(_) => return timed_fail(0x42),
    };
    if cancel_1 != STATUS_OK || cancel_2 != STATUS_OK {
        return timed_fail(0x43);
    }

    let base = now.saturating_add(40_000_000);
    let mut ids = Vec::new();
    for i in 0..64u32 {
        let deadline = base.saturating_add(i as u64);
        let (st, id, _coalesced) = match timed_register(
            &timed,
            0x5449_1000u32.wrapping_add(i),
            QosClass::Idle as u8,
            deadline,
        ) {
            Ok(v) => v,
            Err(_) => return timed_fail(0x50 + i as u64),
        };
        if st != STATUS_OK || id == 0 {
            return timed_fail(0x90 + i as u64);
        }
        ids.push(id);
    }
    let (st_over, _id_over, _coal_over) =
        match timed_register(&timed, 0x5449_10FF, QosClass::Idle as u8, base.saturating_add(65)) {
            Ok(v) => v,
            Err(_) => return timed_fail(0xA1),
        };
    if st_over != STATUS_OVER_LIMIT {
        return timed_fail(0xA2);
    }

    let (st_bad_qos, id_bad_qos, coal_bad_qos) =
        match timed_register(&timed, 0x5449_2000, 0xFF, base.saturating_add(66)) {
            Ok(v) => v,
            Err(_) => return timed_fail(0xA3),
        };
    if st_bad_qos != STATUS_INVALID_ARGS || id_bad_qos != 0 || coal_bad_qos != 0 {
        return timed_fail(0xA4);
    }

    for (idx, id) in ids.into_iter().enumerate() {
        let nonce = 0x5449_3000u32.wrapping_add(idx as u32);
        let st_cancel = match timed_cancel(&timed, nonce, id) {
            Ok(v) => v,
            Err(_) => return timed_fail(0xB0 + idx as u64),
        };
        if st_cancel != STATUS_OK {
            return timed_fail(0xC0 + idx as u64);
        }
    }

    Ok(())
}

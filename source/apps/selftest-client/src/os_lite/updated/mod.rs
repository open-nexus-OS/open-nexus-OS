extern crate alloc;

use alloc::collections::VecDeque;
use alloc::vec::Vec;

use nexus_abi::{yield_, MsgHeader};
use nexus_ipc::KernelClient;

use crate::markers::{emit_byte, emit_bytes, emit_hex_u64, emit_line};

// SECURITY: bring-up test system-set signed with a test key (NOT production custody).
pub(crate) const SYSTEM_TEST_NXS: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/system-test.nxs"));

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SlotId {
    A,
    B,
}

pub(crate) fn updated_stage(
    client: &KernelClient,
    reply_send_slot: u32,
    reply_recv_slot: u32,
    pending: &mut VecDeque<Vec<u8>>,
) -> core::result::Result<(), ()> {
    let mut frame = Vec::with_capacity(8 + SYSTEM_TEST_NXS.len());
    frame.resize(8 + SYSTEM_TEST_NXS.len(), 0u8);
    let n = nexus_abi::updated::encode_stage_req(SYSTEM_TEST_NXS, &mut frame).ok_or(())?;
    emit_line("SELFTEST: updated stage send");
    let rsp = updated_send_with_reply(
        client,
        reply_send_slot,
        reply_recv_slot,
        nexus_abi::updated::OP_STAGE,
        &frame[..n],
        pending,
    )?;
    updated_expect_status(&rsp, nexus_abi::updated::OP_STAGE)?;
    Ok(())
}

pub(crate) fn updated_log_probe(
    client: &KernelClient,
    reply_send_slot: u32,
    reply_recv_slot: u32,
    pending: &mut VecDeque<Vec<u8>>,
) -> core::result::Result<(), ()> {
    let mut frame = [0u8; 4];
    frame[0] = nexus_abi::updated::MAGIC0;
    frame[1] = nexus_abi::updated::MAGIC1;
    frame[2] = nexus_abi::updated::VERSION;
    frame[3] = 0x7f;
    let rsp =
        updated_send_with_reply(client, reply_send_slot, reply_recv_slot, 0x7f, &frame, pending)?;
    updated_expect_status(&rsp, 0x7f)?;
    Ok(())
}

pub(crate) fn updated_switch(
    client: &KernelClient,
    reply_send_slot: u32,
    reply_recv_slot: u32,
    tries_left: u8,
    pending: &mut VecDeque<Vec<u8>>,
) -> core::result::Result<(), ()> {
    let mut frame = [0u8; 5];
    let n = nexus_abi::updated::encode_switch_req(tries_left, &mut frame).ok_or(())?;
    let rsp = updated_send_with_reply(
        client,
        reply_send_slot,
        reply_recv_slot,
        nexus_abi::updated::OP_SWITCH,
        &frame[..n],
        pending,
    )?;
    updated_expect_status(&rsp, nexus_abi::updated::OP_SWITCH)?;
    Ok(())
}

pub(crate) fn updated_get_status(
    client: &KernelClient,
    reply_send_slot: u32,
    reply_recv_slot: u32,
    pending: &mut VecDeque<Vec<u8>>,
) -> core::result::Result<(SlotId, Option<SlotId>, u8, bool), ()> {
    let mut frame = [0u8; 4];
    let n = nexus_abi::updated::encode_get_status_req(&mut frame).ok_or(())?;
    let rsp = updated_send_with_reply(
        client,
        reply_send_slot,
        reply_recv_slot,
        nexus_abi::updated::OP_GET_STATUS,
        &frame[..n],
        pending,
    )?;
    let payload = updated_expect_status(&rsp, nexus_abi::updated::OP_GET_STATUS)?;
    if payload.len() != 4 {
        return Err(());
    }
    let active = match payload[0] {
        1 => SlotId::A,
        2 => SlotId::B,
        _ => return Err(()),
    };
    let pending_slot = match payload[1] {
        0 => None,
        1 => Some(SlotId::A),
        2 => Some(SlotId::B),
        _ => None,
    };
    Ok((active, pending_slot, payload[2], payload[3] != 0))
}

pub(crate) fn updated_boot_attempt(
    client: &KernelClient,
    reply_send_slot: u32,
    reply_recv_slot: u32,
    pending: &mut VecDeque<Vec<u8>>,
) -> core::result::Result<Option<SlotId>, ()> {
    let mut frame = [0u8; 4];
    let n = nexus_abi::updated::encode_boot_attempt_req(&mut frame).ok_or(())?;
    let rsp = updated_send_with_reply(
        client,
        reply_send_slot,
        reply_recv_slot,
        nexus_abi::updated::OP_BOOT_ATTEMPT,
        &frame[..n],
        pending,
    )?;
    let payload = updated_expect_status(&rsp, nexus_abi::updated::OP_BOOT_ATTEMPT)?;
    if payload.len() != 1 {
        return Ok(None);
    }
    Ok(match payload[0] {
        1 => Some(SlotId::A),
        2 => Some(SlotId::B),
        _ => None,
    })
}

pub(crate) fn init_health_ok() -> core::result::Result<(), ()> {
    const CTRL_SEND_SLOT: u32 = 1;
    const CTRL_RECV_SLOT: u32 = 2;
    static NONCE: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(1);
    let nonce = NONCE.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    let mut req = [0u8; 8];
    req[..4].copy_from_slice(&[b'I', b'H', 1, 1]);
    req[4..8].copy_from_slice(&nonce.to_le_bytes());
    let hdr = MsgHeader::new(0, 0, 0, 0, req.len() as u32);

    // Use explicit time-bounded NONBLOCK loops (avoid flaky kernel deadline semantics).
    let start = nexus_abi::nsec().map_err(|_| ())?;
    let deadline = start.saturating_add(30_000_000_000); // 30s (init may contend with stage work)
    let mut i: usize = 0;
    loop {
        match nexus_abi::ipc_send_v1(CTRL_SEND_SLOT, &hdr, &req, nexus_abi::IPC_SYS_NONBLOCK, 0) {
            Ok(_) => break,
            Err(nexus_abi::IpcError::QueueFull) => {
                if (i & 0x7f) == 0 {
                    let now = nexus_abi::nsec().map_err(|_| ())?;
                    if now >= deadline {
                        return Err(());
                    }
                }
                let _ = yield_();
            }
            Err(_) => return Err(()),
        }
        i = i.wrapping_add(1);
    }

    let mut rh = MsgHeader::new(0, 0, 0, 0, 0);
    let mut buf = [0u8; 16];
    let mut j: usize = 0;
    loop {
        if (j & 0x7f) == 0 {
            let now = nexus_abi::nsec().map_err(|_| ())?;
            if now >= deadline {
                return Err(());
            }
        }
        match nexus_abi::ipc_recv_v1(
            CTRL_RECV_SLOT,
            &mut rh,
            &mut buf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(n) => {
                let n = core::cmp::min(n as usize, buf.len());
                if n == 9 && buf[0] == b'I' && buf[1] == b'H' && buf[2] == 1 {
                    let got_nonce = u32::from_le_bytes([buf[5], buf[6], buf[7], buf[8]]);
                    if got_nonce != nonce {
                        continue;
                    }
                    if buf[3] == (1 | 0x80) && buf[4] == 0 {
                        return Ok(());
                    }
                    return Err(());
                }
                // Ignore unrelated control responses.
            }
            Err(nexus_abi::IpcError::QueueEmpty) => {
                let _ = yield_();
            }
            Err(_) => return Err(()),
        }
        j = j.wrapping_add(1);
    }
}

pub(crate) fn updated_expect_status<'a>(
    rsp: &'a [u8],
    op: u8,
) -> core::result::Result<&'a [u8], ()> {
    if rsp.len() < 7 {
        emit_line("SELFTEST: updated rsp short");
        return Err(());
    }
    if rsp[0] != nexus_abi::updated::MAGIC0
        || rsp[1] != nexus_abi::updated::MAGIC1
        || rsp[2] != nexus_abi::updated::VERSION
    {
        emit_bytes(b"SELFTEST: updated rsp magic ");
        emit_hex_u64(rsp[0] as u64);
        emit_byte(b' ');
        emit_hex_u64(rsp[1] as u64);
        emit_byte(b' ');
        emit_hex_u64(rsp[2] as u64);
        emit_byte(b'\n');
        return Err(());
    }
    if rsp[3] != (op | 0x80) || rsp[4] != nexus_abi::updated::STATUS_OK {
        emit_bytes(b"SELFTEST: updated rsp status ");
        emit_hex_u64(rsp[3] as u64);
        emit_byte(b' ');
        emit_hex_u64(rsp[4] as u64);
        emit_byte(b'\n');
        return Err(());
    }
    let len = u16::from_le_bytes([rsp[5], rsp[6]]) as usize;
    if rsp.len() != 7 + len {
        emit_line("SELFTEST: updated rsp len mismatch");
        return Err(());
    }
    Ok(&rsp[7..])
}

pub(crate) fn updated_send_with_reply(
    client: &KernelClient,
    reply_send_slot: u32,
    reply_recv_slot: u32,
    op: u8,
    frame: &[u8],
    pending: &mut VecDeque<Vec<u8>>,
) -> core::result::Result<alloc::vec::Vec<u8>, ()> {
    if reply_send_slot == 0 || reply_recv_slot == 0 {
        return Err(());
    }

    // Drain any stale messages on the shared reply inbox before starting a new exchange.
    // IMPORTANT: do NOT discard them; buffer them so late/out-of-order replies remain consumable.
    for _ in 0..256 {
        let mut hdr = MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 512];
        match nexus_abi::ipc_recv_v1(
            reply_recv_slot,
            &mut hdr,
            &mut buf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(n) => {
                let n = n as usize;
                // Only buffer frames that look like an `updated` reply; other noise is ignored.
                if n >= 4
                    && buf[0] == nexus_abi::updated::MAGIC0
                    && buf[1] == nexus_abi::updated::MAGIC1
                    && buf[2] == nexus_abi::updated::VERSION
                    && (buf[3] & 0x80) != 0
                {
                    if pending.len() >= 16 {
                        let _ = pending.pop_front();
                    }
                    pending.push_back(buf[..n].to_vec());
                }
                continue;
            }
            Err(nexus_abi::IpcError::QueueEmpty) => break,
            Err(_) => break,
        }
    }

    // Also drain the normal updated reply channel (client recv slot). This is a compatibility
    // fallback for bring-up where CAP_MOVE/@reply delivery can be flaky or unavailable.
    let (_updated_send_slot, updated_recv_slot) = client.slots();
    for _ in 0..256 {
        let mut hdr = MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 512];
        match nexus_abi::ipc_recv_v1(
            updated_recv_slot,
            &mut hdr,
            &mut buf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(n) => {
                let n = n as usize;
                if n >= 4
                    && buf[0] == nexus_abi::updated::MAGIC0
                    && buf[1] == nexus_abi::updated::MAGIC1
                    && buf[2] == nexus_abi::updated::VERSION
                    && (buf[3] & 0x80) != 0
                {
                    if pending.len() >= 16 {
                        let _ = pending.pop_front();
                    }
                    pending.push_back(buf[..n].to_vec());
                }
                continue;
            }
            Err(nexus_abi::IpcError::QueueEmpty) => break,
            Err(_) => break,
        }
    }

    // Shared reply inbox: replies can arrive out-of-order across ops.
    if let Some(pos) = pending.iter().position(|rsp| {
        rsp.len() >= 4
            && rsp[0] == nexus_abi::updated::MAGIC0
            && rsp[1] == nexus_abi::updated::MAGIC1
            && rsp[2] == nexus_abi::updated::VERSION
            && rsp[3] == (op | 0x80)
    }) {
        if let Some(rsp) = pending.remove(pos) {
            return Ok(rsp);
        }
    }

    // Prefer plain request/response for bring-up stability; CAP_MOVE remains available but is
    // not required to validate the OTA stage/switch/health markers.
    //
    // IMPORTANT: Avoid kernel deadline-based blocking IPC in bring-up; we've observed
    // deadline semantics that can stall indefinitely. Use NONBLOCK + bounded retry.
    let (updated_send_slot, _updated_recv_slot2) = client.slots();
    {
        let hdr = MsgHeader::new(0, 0, 0, 0, frame.len() as u32);
        let start_ns = nexus_abi::nsec().map_err(|_| ())?;
        let budget_ns: u64 = if op == nexus_abi::updated::OP_STAGE {
            2_000_000_000 // 2s to enqueue a stage request under QEMU
        } else {
            500_000_000 // 0.5s for small ops
        };
        let deadline_ns = start_ns.saturating_add(budget_ns);
        let mut i: usize = 0;
        loop {
            match nexus_abi::ipc_send_v1(
                updated_send_slot,
                &hdr,
                frame,
                nexus_abi::IPC_SYS_NONBLOCK,
                0,
            ) {
                Ok(_) => break,
                Err(nexus_abi::IpcError::QueueFull) => {
                    if (i & 0x7f) == 0 {
                        let now = nexus_abi::nsec().map_err(|_| ())?;
                        if now >= deadline_ns {
                            emit_line("SELFTEST: updated send timeout");
                            return Err(());
                        }
                    }
                    let _ = yield_();
                }
                Err(_) => {
                    emit_line("SELFTEST: updated send fail");
                    return Err(());
                }
            }
            i = i.wrapping_add(1);
        }
    }
    // Give the receiver a chance to run immediately after enqueueing (cooperative scheduler).
    let _ = yield_();
    let mut hdr = MsgHeader::new(0, 0, 0, 0, 0);
    let mut buf = [0u8; 512];
    let mut logged_noise = false;
    // Time-bounded nonblocking receive loop (explicitly yields).
    //
    // NOTE: Kernel deadline semantics for ipc_recv_v1 have been flaky in bring-up; using an
    // explicit nsec()-bounded loop keeps the QEMU smoke run deterministic and bounded (RFC-0013).
    let start_ns = nexus_abi::nsec().map_err(|_| ())?;
    let budget_ns: u64 = if op == nexus_abi::updated::OP_STAGE {
        30_000_000_000 // 30s (stage includes digest + signature verify; allow for QEMU jitter)
    } else {
        5_000_000_000 // 5s (switch/health can involve cross-service publication)
    };
    let deadline_ns = start_ns.saturating_add(budget_ns);
    let mut i: usize = 0;
    loop {
        if (i & 0x7f) == 0 {
            let now = nexus_abi::nsec().map_err(|_| ())?;
            if now >= deadline_ns {
                break;
            }
        }
        match nexus_abi::ipc_recv_v1(
            updated_recv_slot,
            &mut hdr,
            &mut buf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(n) => {
                let n = n as usize;
                if n >= 4
                    && buf[0] == nexus_abi::updated::MAGIC0
                    && buf[1] == nexus_abi::updated::MAGIC1
                    && buf[2] == nexus_abi::updated::VERSION
                    && (buf[3] & 0x80) != 0
                {
                    if buf[3] == (op | 0x80) {
                        return Ok(buf[..n].to_vec());
                    }
                    if !logged_noise {
                        logged_noise = true;
                        emit_bytes(b"SELFTEST: updated rsp other op=0x");
                        emit_hex_u64(buf[3] as u64);
                        if n >= 5 {
                            emit_bytes(b" st=0x");
                            emit_hex_u64(buf[4] as u64);
                        }
                        emit_byte(b'\n');
                    }
                    if pending.len() >= 16 {
                        let _ = pending.pop_front();
                    }
                    pending.push_back(buf[..n].to_vec());
                }
            }
            Err(nexus_abi::IpcError::QueueEmpty) => {
                let _ = yield_();
            }
            Err(_) => return Err(()),
        }
        i = i.wrapping_add(1);
    }
    emit_line("SELFTEST: updated recv timeout");
    Err(())
}

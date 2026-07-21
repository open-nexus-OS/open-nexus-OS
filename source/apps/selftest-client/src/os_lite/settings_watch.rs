// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0078 settings-watch probe: subscribe with a freshly minted
//! push channel (`@mint-pair`), flip `input.keymap`, and require the pushed
//! `OP_EVENT` to arrive — proving the OP_WATCH spine end-to-end. Restores
//! the default afterwards (the second event is verified too).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal (binary crate)
//! TEST_COVERAGE: QEMU marker ladder via `just test-os`.
//! RFC: docs/rfcs/RFC-0078-settings-region-keys-watch.md

use core::time::Duration;

use nexus_abi::settingsd as wire;
use nexus_ipc::budget::{self, NonceMismatchBudget, RouteRetryOutcome};
use nexus_ipc::{Client, Wait as IpcWait};

use super::ipc::routing::route_with_retry;

fn mint_pair() -> Option<(u32, u32)> {
    match budget::route_with_nonce_budgeted(
        b"@mint-pair",
        1,
        2,
        Duration::from_secs(2),
        NonceMismatchBudget::new(64),
    ) {
        RouteRetryOutcome::Success { send_slot, recv_slot } => Some((send_slot, recv_slot)),
        _ => None,
    }
}

fn set_key(client: &nexus_ipc::KernelClient, key: &str, value: &str) -> Result<(), ()> {
    let mut req = [0u8; 300];
    let n = wire::encode_set_req(key, value, &mut req).ok_or(())?;
    client.send(&req[..n], IpcWait::Timeout(Duration::from_millis(300))).map_err(|_| ())?;
    let rsp = client.recv(IpcWait::Timeout(Duration::from_millis(300))).map_err(|_| ())?;
    let (status, _v) = wire::decode_response(wire::OP_SET, &rsp).ok_or(())?;
    if status == wire::STATUS_OK {
        Ok(())
    } else {
        Err(())
    }
}

fn recv_event(recv_slot: u32, want_key: &str, want_value: &str) -> Result<(), ()> {
    // Blocking recv with a hard deadline (never a yield spin — that disturbs
    // the kernel tick-budget proof running in the same window).
    let deadline = nexus_abi::nsec().map_err(|_| ())?.saturating_add(800_000_000);
    let mut buf = [0u8; 600];
    loop {
        if nexus_abi::nsec().unwrap_or(u64::MAX) >= deadline {
            return Err(());
        }
        let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut sid: u64 = 0;
        match nexus_abi::ipc_recv_v2(
            recv_slot,
            &mut hdr,
            &mut buf,
            &mut sid,
            nexus_abi::IPC_SYS_TRUNCATE,
            deadline,
        ) {
            Ok(len) => {
                let len = (len as usize).min(buf.len());
                let Some((_flags, key, value)) = wire::decode_event(&buf[..len]) else {
                    return Err(());
                };
                if key == want_key && value == want_value {
                    return Ok(());
                }
                // A different key under the prefix — keep draining.
            }
            Err(_) => return Err(()),
        }
    }
}

fn fail(code: u32) -> Result<(), ()> {
    let mut msg = [0u8; 40];
    let text = b"SELFTEST: settings watch step=0x";
    msg[..text.len()].copy_from_slice(text);
    let hex = b"0123456789abcdef";
    msg[text.len()] = hex[((code >> 4) & 0xF) as usize];
    msg[text.len() + 1] = hex[(code & 0xF) as usize];
    if let Ok(m) = core::str::from_utf8(&msg[..text.len() + 2]) {
        let _ = nexus_abi::debug_println(m);
    }
    Err(())
}

pub(crate) fn settings_watch_probe() -> Result<(), ()> {
    let Ok(client) = route_with_retry("settingsd") else {
        return fail(0x01);
    };
    let Some((ev_send, ev_recv)) = mint_pair() else {
        return fail(0x02);
    };
    // Subscribe: OP_WATCH with the cap-moved push SEND half.
    let mut req = [0u8; 72];
    let Some(n) = wire::encode_watch_req("input.", &mut req) else {
        return fail(0x03);
    };
    let hdr = nexus_abi::MsgHeader::new(ev_send, 0, 0, nexus_abi::ipc_hdr::CAP_MOVE, n as u32);
    let (send_slot, _) = client.slots();
    if nexus_abi::ipc_send_v1(send_slot, &hdr, &req[..n], nexus_abi::IPC_SYS_NONBLOCK, 0).is_err() {
        return fail(0x04);
    }
    // Flip the keymap and require the pushed event; then restore the default
    // (and require that push too — end state = shipped default).
    if set_key(&client, "input.keymap", "us").is_err() {
        return fail(0x05);
    }
    if recv_event(ev_recv, "input.keymap", "us").is_err() {
        return fail(0x06);
    }
    if set_key(&client, "input.keymap", "de").is_err() {
        return fail(0x07);
    }
    if recv_event(ev_recv, "input.keymap", "de").is_err() {
        return fail(0x08);
    }
    // Settle: inputd receives the same OP_EVENTs and prints its keymap
    // markers concurrently — give those UART lines time to complete before
    // the verdict marker (interleaved lines hard-fail evidence assembly).
    // A deadline-blocked recv on the now-idle event channel sleeps without
    // yield-spinning (which would disturb the kernel tick-budget proof).
    let settle_deadline = nexus_abi::nsec().unwrap_or(0).saturating_add(150_000_000);
    let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
    let mut sid: u64 = 0;
    let mut buf = [0u8; 64];
    let _ = nexus_abi::ipc_recv_v2(
        ev_recv,
        &mut hdr,
        &mut buf,
        &mut sid,
        nexus_abi::IPC_SYS_TRUNCATE,
        settle_deadline,
    );
    Ok(())
}

/// RFC-0076: tz-lite conversion proof in the OS build — a fixed epoch must
/// convert correctly for two zones (Berlin CEST +2, Tokyo +9).
pub(crate) fn clock_tz_probe() -> Result<(), ()> {
    const T_2026_07_21_1200Z: u64 = 1_784_635_200 * 1_000_000_000;
    let berlin = tz_lite::zone("Europe/Berlin").ok_or(())?;
    let tokyo = tz_lite::zone("Asia/Tokyo").ok_or(())?;
    let b = tz_lite::to_civil(T_2026_07_21_1200Z, berlin);
    let t = tz_lite::to_civil(T_2026_07_21_1200Z, tokyo);
    if (b.hour, b.minute, b.day) == (14, 0, 21) && (t.hour, t.day) == (21, 21) {
        Ok(())
    } else {
        Err(())
    }
}

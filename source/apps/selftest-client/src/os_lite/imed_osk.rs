// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: imed OSK-endpoint probe (RFC-0075 Phase 2): the selftest holds
//! an init-provisioned SEND on imed's DEDICATED osk endpoint (possession =
//! authorization) and proves BOTH directions: a well-formed `source=osk`
//! key is ACCEPTED (STATUS_OK on the probe's own reply channel), a
//! mis-tagged `source=hw` frame on the same endpoint is DENIED. The
//! commit-at-focused-field chain is the interactive OSK proof (`just
//! start`); this lane proves authorization + codec + serve loop. Fixture
//! character only — no real text.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal (binary crate)
//! TEST_COVERAGE: QEMU marker ladder via `just test-os`.
//! RFC: docs/rfcs/RFC-0075-ime-v2-text-focus-composition-delivery.md

use core::time::Duration;

use nexus_ipc::budget::{self, NonceMismatchBudget, RouteRetryOutcome};

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

/// One osk-endpoint request with a CAP_MOVE'd reply SEND; returns the reply
/// status + the commit ECHO (deadline-blocked recv, never a spin).
fn osk_call(osk_send: u32, req: &[u8]) -> Result<(u8, [u8; 64], usize), ()> {
    use nexus_abi::imed as wire;
    let (ev_send, ev_recv) = mint_pair().ok_or(())?;
    let hdr =
        nexus_abi::MsgHeader::new(ev_send, 0, 0, nexus_abi::ipc_hdr::CAP_MOVE, req.len() as u32);
    if nexus_abi::ipc_send_v1(osk_send, &hdr, req, nexus_abi::IPC_SYS_NONBLOCK, 0).is_err() {
        return Err(());
    }
    let deadline = nexus_abi::nsec().map_err(|_| ())?.saturating_add(800_000_000);
    let mut rhdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
    let mut sid: u64 = 0;
    let mut buf = [0u8; 96];
    let len = nexus_abi::ipc_recv_v2(
        ev_recv,
        &mut rhdr,
        &mut buf,
        &mut sid,
        nexus_abi::IPC_SYS_TRUNCATE,
        deadline,
    )
    .map_err(|_| ())? as usize;
    let op = *req.get(3).ok_or(())?;
    let (status, text) = wire::decode_osk_reply(op, &buf[..len]).ok_or(())?;
    let mut echo = [0u8; 64];
    let n = text.len().min(64);
    echo[..n].copy_from_slice(&text.as_bytes()[..n]);
    Ok((status, echo, n))
}

fn key_frame(source: u8, kind: u8, ch: char, action: u8) -> [u8; 12] {
    let mut req = [0u8; 12];
    req[0] = b'I';
    req[1] = b'E';
    req[2] = 1; // VERSION
    req[3] = 2; // OP_KEY
    req[4] = source;
    req[5] = kind;
    req[6..10].copy_from_slice(&u32::from(ch).to_le_bytes());
    req[10] = action;
    req[11] = 0; // modifiers
    req
}

fn osk_key_status(osk_send: u32, source: u8, ch: char) -> Result<u8, ()> {
    osk_call(osk_send, &key_frame(source, 0, ch, 0)).map(|(status, _, _)| status)
}

fn set_layout(osk_send: u32, layout: &str) -> Result<(), ()> {
    use nexus_abi::imed as wire;
    let mut req = [0u8; 16];
    let n = wire::encode_set_layout(layout, &mut req).ok_or(())?;
    let (status, _, _) = osk_call(osk_send, &req[..n])?;
    if status == 0 {
        Ok(())
    } else {
        Err(())
    }
}

/// Types `text` as osk keys and returns the LAST step's commit echo.
fn type_and_echo(
    osk_send: u32,
    text: &str,
    then_action: Option<u8>,
) -> Result<([u8; 64], usize), ()> {
    let mut last = ([0u8; 64], 0usize);
    for ch in text.chars() {
        let (status, echo, n) = osk_call(osk_send, &key_frame(1, 0, ch, 0))?;
        if status != 0 {
            return Err(());
        }
        last = (echo, n);
    }
    if let Some(action) = then_action {
        let (status, echo, n) = osk_call(osk_send, &key_frame(1, 2, '\0', action))?;
        if status != 0 {
            return Err(());
        }
        last = (echo, n);
    }
    Ok(last)
}

pub(crate) fn imed_osk_probe() -> Result<(), ()> {
    // The osk route is init-provisioned for the harness; resolving it also
    // proves the imed-osk named route exists.
    let client = route_with_retry("imed-osk").map_err(|_| ())?;
    let (osk_send, _) = client.slots();
    // Positive: a well-formed osk-sourced key is ACCEPTED (STATUS_OK = 0).
    if osk_key_status(osk_send, 1, 'x')? != 0 {
        return Err(());
    }
    // Negative: a hw-tagged frame on the OSK endpoint is DENIED (2).
    if osk_key_status(osk_send, 0, 'x')? != 2 {
        return Err(());
    }
    Ok(())
}

/// RFC-0075 Phase 3: the REAL jp engine behind the service path — romaji
/// `nn` + Enter must echo ん (fixture text, no user data). Composition is
/// focus-independent; delivery stays focus-gated (nothing reaches apps).
pub(crate) fn imed_cjk_jp_probe() -> Result<(), ()> {
    let client = route_with_retry("imed-osk").map_err(|_| ())?;
    let (osk_send, _) = client.slots();
    set_layout(osk_send, "jp")?;
    let (echo, n) = type_and_echo(osk_send, "nn", Some(0 /* ACTION_ENTER */))?;
    let ok = &echo[..n] == "ん".as_bytes();
    // Restore the shipped default engine regardless of the verdict.
    let _ = set_layout(osk_send, "de");
    if ok {
        Ok(())
    } else {
        Err(())
    }
}

/// RFC-0075 Phase 3: pinyin `nihao` + space (opens candidates) + select(0)
/// must echo 你好 — the full candidate machinery through the service path.
pub(crate) fn imed_candidates_probe() -> Result<(), ()> {
    use nexus_abi::imed as wire;
    let client = route_with_retry("imed-osk").map_err(|_| ())?;
    let (osk_send, _) = client.slots();
    set_layout(osk_send, "zh")?;
    let _ = type_and_echo(osk_send, "nihao ", None)?;
    let sel = wire::encode_candidate_select(0);
    let (status, echo, n) = osk_call(osk_send, &sel)?;
    let ok = status == 0 && &echo[..n] == "你好".as_bytes();
    let _ = set_layout(osk_send, "de");
    if ok {
        Ok(())
    } else {
        Err(())
    }
}

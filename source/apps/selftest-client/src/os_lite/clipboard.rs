// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: clipboardd probe (RFC-0094, TASK-0067). The harness holds a declared route to
//! the clipboard authority and proves the gate from the side it can see:
//!   - WRITE is the route's capability: six texts are stored, each under a strictly larger
//!     `seq` (they stay in the history — the operator asked for a clipboard that is not empty
//!     on first open, so the shell's search and the keyboard show cards on every boot);
//!   - READ and LIST are refused: the harness is no focused window, no shell, no keyboard;
//!   - a lying length is MALFORMED, never a stored item.
//!
//! The allow side (the shell lists and copies back, the keyboard pastes) is proven where the
//! focus truth is real: the visible lane's injector and `SELFTEST: ui v7 clipboard ok`.
//! The texts are fixtures written for this purpose — no user data.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal (binary crate)
//! TEST_COVERAGE: QEMU marker ladder (`SELFTEST: clipboard gate ok`)
//! RFC: docs/rfcs/RFC-0094-content-transfer-v1-clipboardd.md

use nexus_wire::clipboardd as wire;

/// The prefill, oldest first (the history shows the last one on top).
const PREFILL: [&str; 6] = [
    "Treffen morgen um 10:00 im Besprechungsraum 2",
    "https://example.org/open-nexus/docs",
    "Einkaufsliste: Milch, Brot, Kaffee, Äpfel",
    "Musterstraße 12, 10115 Berlin",
    "Bestellnummer 4711-2026",
    "Danke für die schnelle Antwort!",
];

/// One request on the declared route; the reply rides the harness's CAP_MOVE inbox
/// (a waited exchange: the answer or clipboardd's death — no clock).
fn call(req: &[u8], rsp: &mut [u8]) -> Result<usize, ()> {
    let route = nexus_service_topology::slots::selftest_client::CLIPBOARDD;
    let reply = nexus_service_topology::slots::selftest_client::REPLY;
    nexus_ipc::exchange::call_into(route.send, reply, req, rsp).map_err(|_| ())
}

pub(crate) fn clipboard_gate_probe() -> Result<(), ()> {
    let mut req = [0u8; wire::REQUEST_MAX_BYTES];
    let mut rsp = [0u8; wire::REPLY_MAX_BYTES];
    // WRITE: the route is the capability; every item gets a larger seq.
    let mut last = 0u64;
    for text in PREFILL {
        let n = wire::encode_write(text, &mut req).ok_or(())?;
        let len = call(&req[..n], &mut rsp)?;
        match wire::decode_seq_reply(wire::OP_WRITE, &rsp[..len]) {
            Some((wire::STATUS_OK, seq)) if seq > last => last = seq,
            _ => return Err(()),
        }
    }
    // READ: the harness is not a focused window, the shell or the keyboard.
    let len = call(&wire::encode_read(0), &mut rsp)?;
    if !matches!(
        wire::decode_read_reply(wire::OP_READ, &rsp[..len]),
        Some((wire::STATUS_DENIED, _, ""))
    ) {
        return Err(());
    }
    // LIST (the history): the shell and the keyboard only.
    let n = wire::encode_list("", &mut req).ok_or(())?;
    let len = call(&req[..n], &mut rsp)?;
    match wire::decode_list_reply(wire::OP_LIST, &rsp[..len]) {
        Some((wire::STATUS_DENIED, 0, packed)) if packed.is_empty() => {}
        _ => return Err(()),
    }
    // A lying length byte is malformed — never a stored item.
    let mut lie = [0u8; 8];
    lie[..4].copy_from_slice(&[wire::MAGIC0, wire::MAGIC1, wire::VERSION, wire::OP_WRITE]);
    lie[4] = 200;
    let len = call(&lie, &mut rsp)?;
    if wire::decode_status(wire::OP_WRITE, &rsp[..len]) != Some(wire::STATUS_MALFORMED) {
        return Err(());
    }
    Ok(())
}

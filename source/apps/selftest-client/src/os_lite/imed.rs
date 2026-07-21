// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: imed identity-gate negative probe (RFC-0075): the selftest sends
//! an `OP_KEY` frame — a FOREIGN key source — and the IME authority MUST
//! answer DENIED (keys are accepted only from inputd's kernel identity).
//! Proves imed serves AND fails closed; the fixture carries no real text.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal (binary crate)
//! TEST_COVERAGE: QEMU marker ladder via `just test-os`.
//! RFC: docs/rfcs/RFC-0075-ime-v2-text-focus-composition-delivery.md

use nexus_ipc::{Client, Wait as IpcWait};

use super::ipc::routing::route_with_retry;

pub(crate) fn imed_reject_foreign_probe() -> core::result::Result<(), ()> {
    let client = route_with_retry("imed").map_err(|_| ())?;
    // OP_KEY (source=hw, kind=text, ch='a') from the WRONG sender identity.
    let mut req = [0u8; 12];
    req[0] = b'I';
    req[1] = b'E';
    req[2] = 1; // VERSION
    req[3] = 2; // OP_KEY
    req[4] = 0; // KEY_SOURCE_HW
    req[5] = 0; // KEY_KIND_TEXT
    req[6..10].copy_from_slice(&u32::from('a').to_le_bytes());
    req[10] = 0; // action
    req[11] = 0; // modifiers
    if client.send(&req, IpcWait::Timeout(core::time::Duration::from_millis(300))).is_err() {
        return Err(());
    }
    let rsp =
        client.recv(IpcWait::Timeout(core::time::Duration::from_millis(300))).map_err(|_| ())?;
    // Expected: [I, E, 1, OP_KEY|0x80, STATUS_DENIED].
    if rsp.len() == 5
        && rsp[0] == b'I'
        && rsp[1] == b'E'
        && rsp[2] == 1
        && rsp[3] == (2 | 0x80)
        && rsp[4] == 2
    {
        Ok(())
    } else {
        Err(())
    }
}

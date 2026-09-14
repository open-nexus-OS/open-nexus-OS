// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Init-health helper for the `updated` submodule — `init_health_ok`
//!   sends the bring-up health probe on the init control channel
//!   (`nexus_service_topology::CTRL_SLOTS`).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os) — ota phase (mark-good wait).
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

use nexus_abi::MsgHeader;

pub(crate) fn init_health_ok() -> core::result::Result<(), ()> {
    use nexus_service_topology::CTRL_SLOTS;
    static NONCE: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(1);
    let nonce = NONCE.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    let mut req = [0u8; 8];
    req[..4].copy_from_slice(&[b'I', b'H', 1, 1]);
    req[4..8].copy_from_slice(&nonce.to_le_bytes());
    let hdr = MsgHeader::new(0, 0, 0, 0, req.len() as u32);

    // A waited send on the control channel, then waited receives (init's answer, nonce-matched,
    // or init's death) — no clock (TASK-0324 P7-d).
    if nexus_abi::ipc_send_v1(CTRL_SLOTS.send, &hdr, &req, 0, 0).is_err() {
        return Err(());
    }

    let mut rh = MsgHeader::new(0, 0, 0, 0, 0);
    let mut buf = [0u8; 16];
    loop {
        match nexus_abi::ipc_recv_v1(
            CTRL_SLOTS.recv,
            &mut rh,
            &mut buf,
            nexus_abi::IPC_SYS_TRUNCATE,
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
            Err(_) => return Err(()),
        }
    }
}

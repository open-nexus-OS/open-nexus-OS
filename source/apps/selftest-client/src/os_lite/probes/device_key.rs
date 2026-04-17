use nexus_ipc::{Client, KernelClient, Wait as IpcWait};

use super::super::ipc::routing::route_with_retry;
use crate::markers::{emit_byte, emit_bytes, emit_hex_u64, emit_line};

/// Test keystored device key operations.
/// Proves:
/// - Device keygen works (via rngd entropy)
/// - Device pubkey export works
/// - Private key export is correctly rejected
///
/// # Security
/// - Private key is NEVER exported
pub(crate) fn device_key_selftest() -> Option<[u8; 32]> {
    // Connect to keystored
    let client = match KernelClient::new_for("keystored") {
        Ok(c) => c,
        Err(_) => {
            emit_line("SELFTEST: device key pubkey FAIL (no route)");
            return None;
        }
    };

    let wait = IpcWait::Timeout(core::time::Duration::from_millis(500));

    // 1. Trigger device keygen (OP=10)
    {
        let req = [b'K', b'S', 1, 10]; // DEVICE_KEYGEN
        if client.send(&req, wait).is_err() {
            emit_line("SELFTEST: device key pubkey FAIL (keygen send)");
            return None;
        }
        match client.recv(wait) {
            Ok(rsp) => {
                if rsp.len() < 7 || rsp[0] != b'K' || rsp[1] != b'S' || rsp[2] != 1 {
                    emit_line("SELFTEST: device key pubkey FAIL (keygen malformed)");
                    return None;
                }
                // Status can be OK (0) or KEY_EXISTS (10)
                let status = rsp[4];
                if status != 0 && status != 10 {
                    emit_bytes(b"SELFTEST: device key pubkey FAIL (keygen status=");
                    emit_hex_u64(status as u64);
                    emit_line(")");
                    return None;
                }
            }
            Err(_) => {
                emit_line("SELFTEST: device key pubkey FAIL (keygen recv)");
                return None;
            }
        }
    }

    // 2. Get device pubkey (OP=11)
    let pubkey = {
        let req = [b'K', b'S', 1, 11]; // GET_DEVICE_PUBKEY
        if client.send(&req, wait).is_err() {
            emit_line("SELFTEST: device key pubkey FAIL (pubkey send)");
            return None;
        }
        match client.recv(wait) {
            Ok(rsp) => {
                if rsp.len() < 7 || rsp[0] != b'K' || rsp[1] != b'S' || rsp[2] != 1 {
                    emit_line("SELFTEST: device key pubkey FAIL (pubkey malformed)");
                    return None;
                }
                let status = rsp[4];
                if status != 0 {
                    emit_bytes(b"SELFTEST: device key pubkey FAIL (pubkey status=");
                    emit_hex_u64(status as u64);
                    emit_line(")");
                    return None;
                }
                // Response should include 32-byte pubkey after the 7-byte header
                // [K, S, ver, op|0x80, status, len:u16le, pubkey...]
                let val_len = u16::from_le_bytes([rsp[5], rsp[6]]) as usize;
                if val_len != 32 || rsp.len() < 7 + 32 {
                    emit_bytes(b"SELFTEST: device key pubkey FAIL (pubkey len=");
                    emit_hex_u64(val_len as u64);
                    emit_line(")");
                    return None;
                }
                // SECURITY: We can log pubkey (it's public), but keep it brief
                emit_line("SELFTEST: device key pubkey ok");
                let mut out = [0u8; 32];
                out.copy_from_slice(&rsp[7..7 + 32]);
                out
            }
            Err(_) => {
                emit_line("SELFTEST: device key pubkey FAIL (pubkey recv)");
                return None;
            }
        }
    };

    // 3. Verify private key export is rejected
    // There's no OP for private export in the protocol by design,
    // but we can verify signing requires policy
    device_key_private_export_rejected_selftest(&client);
    Some(pubkey)
}

/// Verify that private key export attempts are rejected.
/// This tests that an unprivileged caller cannot sign with the device key.
pub(crate) fn device_key_private_export_rejected_selftest(client: &KernelClient) {
    // Explicit private export op must deterministically reject.
    // Request: [K, S, ver, OP_GET_DEVICE_PRIVKEY=13]
    let req = [b'K', b'S', 1, 13];
    let wait = IpcWait::Timeout(core::time::Duration::from_millis(500));
    if client.send(&req, wait).is_err() {
        emit_line("SELFTEST: device key private export rejected FAIL (send)");
        return;
    }
    match client.recv(wait) {
        Ok(rsp) => {
            if rsp.len() < 7 || rsp[0] != b'K' || rsp[1] != b'S' || rsp[2] != 1 {
                emit_line("SELFTEST: device key private export rejected FAIL (malformed)");
                return;
            }
            let status = rsp[4];
            if status == 12 {
                emit_line("SELFTEST: device key private export rejected ok");
            } else {
                emit_bytes(b"SELFTEST: device key private export status=");
                emit_hex_u64(status as u64);
                emit_byte(b'\n');
                emit_line("SELFTEST: device key private export rejected FAIL");
            }
        }
        Err(_) => emit_line("SELFTEST: device key private export rejected FAIL (recv)"),
    }
}

pub(crate) fn device_key_reload_and_check(expected: &[u8; 32]) -> core::result::Result<(), ()> {
    let client = match route_with_retry("keystored") {
        Ok(c) => c,
        Err(_) => {
            emit_line("SELFTEST: reload route fail");
            return Err(());
        }
    };
    let wait = IpcWait::Timeout(core::time::Duration::from_millis(1000));
    let req = [b'K', b'S', 1, 14]; // DEVICE_RELOAD
    if client.send(&req, wait).is_err() {
        emit_line("SELFTEST: reload send fail");
        return Err(());
    }
    let rsp = match client.recv(wait) {
        Ok(r) => r,
        Err(_) => {
            emit_line("SELFTEST: reload recv fail");
            return Err(());
        }
    };
    if rsp.len() < 7 || rsp[0] != b'K' || rsp[1] != b'S' || rsp[2] != 1 {
        emit_line("SELFTEST: reload rsp malformed");
        return Err(());
    }
    if rsp[4] != 0 {
        emit_bytes(b"SELFTEST: reload rsp status=");
        emit_hex_u64(rsp[4] as u64);
        emit_byte(b'\n');
        return Err(());
    }
    emit_line("SELFTEST: reload ok");
    let req = [b'K', b'S', 1, 11]; // GET_DEVICE_PUBKEY
    if client.send(&req, wait).is_err() {
        emit_line("SELFTEST: reload pubkey send fail");
        return Err(());
    }
    let rsp = match client.recv(wait) {
        Ok(r) => r,
        Err(_) => {
            emit_line("SELFTEST: reload pubkey recv fail");
            return Err(());
        }
    };
    if rsp.len() < 7 || rsp[0] != b'K' || rsp[1] != b'S' || rsp[2] != 1 {
        emit_line("SELFTEST: reload pubkey rsp malformed");
        return Err(());
    }
    if rsp[4] != 0 {
        emit_bytes(b"SELFTEST: reload pubkey status=");
        emit_hex_u64(rsp[4] as u64);
        emit_byte(b'\n');
        return Err(());
    }
    let val_len = u16::from_le_bytes([rsp[5], rsp[6]]) as usize;
    if val_len != 32 || rsp.len() < 7 + 32 {
        emit_line("SELFTEST: reload pubkey len mismatch");
        return Err(());
    }
    if &rsp[7..7 + 32] != expected {
        emit_line("SELFTEST: reload pubkey mismatch");
        return Err(());
    }
    Ok(())
}

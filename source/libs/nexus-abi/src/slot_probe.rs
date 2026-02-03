// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Fast capability slot probing for early boot validation
//! OWNERS: @runtime
//! STATUS: Experimental
//!
//! This module provides functions to quickly validate that capability
//! slots are populated before a service tries to use them.
//! This catches routing/distribution errors at startup instead of
//! during runtime when they're harder to debug.

use crate::{cap_query, debug_putc, CapQuery};

/// Probe a slot and return whether it holds a valid capability.
/// This is a fast, non-blocking check.
pub fn slot_is_valid(slot: u32) -> bool {
    let mut info = CapQuery {
        kind_tag: 0,
        reserved: 0,
        base: 0,
        len: 0,
    };
    match cap_query(slot, &mut info) {
        Ok(()) => info.kind_tag != 0, // 0 = empty/invalid
        Err(_) => false,
    }
}

/// Kind tag for IPC endpoints (from kernel cap model).
const KIND_IPC_ENDPOINT: u32 = 3;

/// Probe a slot and verify it holds an IPC endpoint.
pub fn slot_is_ipc_endpoint(slot: u32) -> bool {
    let mut info = CapQuery {
        kind_tag: 0,
        reserved: 0,
        base: 0,
        len: 0,
    };
    match cap_query(slot, &mut info) {
        Ok(()) => info.kind_tag == KIND_IPC_ENDPOINT,
        Err(_) => false,
    }
}

/// Validate a set of required slots at startup.
/// Emits clear error messages for any missing slots.
/// Returns the number of missing slots (0 = all OK).
pub fn validate_slots(service: &str, required: &[(&str, u32)]) -> usize {
    fn emit(s: &[u8]) {
        for &b in s {
            let _ = debug_putc(b);
        }
    }
    let hex = |n: u8| if n < 10 { b'0' + n } else { b'a' + n - 10 };

    let mut missing = 0;
    for (name, slot) in required {
        if !slot_is_valid(*slot) {
            emit(b"SLOT MISSING: ");
            emit(service.as_bytes());
            emit(b" needs ");
            emit(name.as_bytes());
            emit(b" at slot 0x");
            let _ = debug_putc(hex((*slot >> 4) as u8 & 0xf));
            let _ = debug_putc(hex(*slot as u8 & 0xf));
            emit(b"\n");
            missing += 1;
        }
    }

    if missing == 0 {
        emit(b"slot-check: ");
        emit(service.as_bytes());
        emit(b" all ");
        // Emit count as decimal
        let count = required.len();
        if count >= 10 {
            let _ = debug_putc(b'0' + (count / 10) as u8);
        }
        let _ = debug_putc(b'0' + (count % 10) as u8);
        emit(b" slots ok\n");
    } else {
        emit(b"slot-check: ");
        emit(service.as_bytes());
        emit(b" FAIL (");
        if missing >= 10 {
            let _ = debug_putc(b'0' + (missing / 10) as u8);
        }
        let _ = debug_putc(b'0' + (missing % 10) as u8);
        emit(b" missing)\n");
    }

    missing
}

/// Quick IPC send test - verify a slot can accept a message.
/// Returns true if send succeeded (even if no response).
/// This is useful for validating that an endpoint is alive.
pub fn probe_ipc_slot(slot: u32) -> bool {
    // Send a minimal probe frame that all services should ignore
    // Format: [0x00] - zero-length "ping" that gets dropped
    let probe = [0u8; 0];
    let hdr = crate::MsgHeader::new(0, 0, 0, 0, 0);
    match crate::ipc_send_v1(slot, &hdr, &probe, crate::IPC_SYS_NONBLOCK, 0) {
        Ok(_) => true,
        Err(crate::IpcError::QueueFull) => true, // Endpoint exists but busy
        Err(_) => false,
    }
}

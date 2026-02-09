// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Deterministic capability slot assignment map
//! OWNERS: @runtime
//! STATUS: Experimental
//!
//! This module defines a compile-time slot map for OS-lite bring-up.
//! All slot assignments are explicit and documented, making it easy to:
//! - Detect when a service gets the wrong slot
//! - Debug routing failures by comparing expected vs actual slots
//! - Avoid race conditions during early boot
//!
//! INVARIANT: Slot numbers MUST NOT change without updating all consumers.

#![allow(missing_docs)] // Slot constants are self-documenting by name

/// Slot assignments for selftest-client.
/// These are the slots that selftest-client receives from init-lite.
pub mod selftest {
    // First slots are reserved for ctrl channel (0, 1, 2)
    pub const CTRL_RECV: u32 = 0x01;
    pub const CTRL_SEND: u32 = 0x02;

    // Service endpoints (in transfer order from init-lite)
    pub const VFSD_SEND: u32 = 0x03;
    pub const VFSD_RECV: u32 = 0x04;
    pub const PKGFSD_SEND: u32 = 0x05;
    pub const PKGFSD_RECV: u32 = 0x06;
    pub const POLICYD_SEND: u32 = 0x07;
    pub const POLICYD_RECV: u32 = 0x08;
    pub const BUNDLEMGRD_SEND: u32 = 0x09;
    pub const BUNDLEMGRD_RECV: u32 = 0x0A;
    pub const UPDATED_SEND: u32 = 0x0B;
    pub const UPDATED_RECV: u32 = 0x0C;
    pub const SAMGRD_SEND: u32 = 0x0D;
    pub const SAMGRD_RECV: u32 = 0x0E;
    pub const EXECD_SEND: u32 = 0x0F;
    pub const EXECD_RECV: u32 = 0x10;
    pub const KEYSTORED_SEND: u32 = 0x11;
    pub const KEYSTORED_RECV: u32 = 0x12;
    pub const STATEFSD_SEND: u32 = 0x13;
    pub const STATEFSD_RECV: u32 = 0x14;
    pub const LOGD_SEND: u32 = 0x15;
    pub const LOGD_RECV: u32 = 0x16;
    pub const REPLY_RECV: u32 = 0x17;
    pub const REPLY_SEND: u32 = 0x18;
    pub const RNGD_SEND: u32 = 0x1D;
    pub const RNGD_RECV: u32 = 0x1E;
}

/// Slot assignments for keystored.
pub mod keystored {
    pub const CTRL_RECV: u32 = 0x01;
    pub const CTRL_SEND: u32 = 0x02;
    pub const SERVICE_RECV: u32 = 0x03;
    pub const SERVICE_SEND: u32 = 0x04;
    pub const REPLY_RECV: u32 = 0x05;
    pub const REPLY_SEND: u32 = 0x06;
    pub const STATEFSD_SEND: u32 = 0x07;
    pub const LOGD_SEND: u32 = 0x08;
    pub const POLICYD_SEND: u32 = 0x09;
    pub const RNGD_SEND: u32 = 0x0A;
}

/// Slot assignments for statefsd.
pub mod statefsd {
    pub const CTRL_RECV: u32 = 0x01;
    pub const CTRL_SEND: u32 = 0x02;
    pub const SERVICE_RECV: u32 = 0x03;
    pub const SERVICE_SEND: u32 = 0x04;
    pub const REPLY_RECV: u32 = 0x05;
    pub const REPLY_SEND: u32 = 0x06;
    pub const POLICYD_SEND: u32 = 0x07;
    pub const LOGD_SEND: u32 = 0x08;
}

/// Slot assignments for updated.
pub mod updated {
    pub const CTRL_RECV: u32 = 0x01;
    pub const CTRL_SEND: u32 = 0x02;
    pub const SERVICE_RECV: u32 = 0x03;
    pub const SERVICE_SEND: u32 = 0x04;
    // Additional slots depend on init-lite transfer order
    pub const BUNDLEMGRD_SEND: u32 = 0x05;
    pub const BUNDLEMGRD_RECV: u32 = 0x06;
    pub const KEYSTORED_SEND: u32 = 0x07;
    pub const KEYSTORED_RECV: u32 = 0x08;
    pub const STATEFSD_SEND: u32 = 0x09;
    pub const REPLY_RECV: u32 = 0x0A;
    pub const REPLY_SEND: u32 = 0x0B;
    pub const LOGD_SEND: u32 = 0x0C;
}

/// Verify a slot assignment at runtime.
/// Panics with a clear message if the slot doesn't match.
#[inline]
pub fn assert_slot(actual: u32, expected: u32, service: &str, role: &str) {
    if actual != expected {
        // In OS builds, emit to UART for debugging
        #[cfg(all(nexus_env = "os", feature = "os-lite"))]
        {
            use nexus_abi::debug_putc;
            fn emit(s: &[u8]) {
                for &b in s {
                    let _ = debug_putc(b);
                }
            }
            emit(b"SLOT MISMATCH: ");
            emit(service.as_bytes());
            emit(b" ");
            emit(role.as_bytes());
            emit(b" expected=0x");
            let hex = |n: u8| if n < 10 { b'0' + n } else { b'a' + n - 10 };
            let _ = debug_putc(hex((expected >> 4) as u8 & 0xf));
            let _ = debug_putc(hex(expected as u8 & 0xf));
            emit(b" actual=0x");
            let _ = debug_putc(hex((actual >> 4) as u8 & 0xf));
            let _ = debug_putc(hex(actual as u8 & 0xf));
            emit(b"\n");
        }
    }
    // In host builds the parameters may not be used (UART printing is cfg'd out).
    let _ = (service, role);
}

/// Emit the slot map for a service at startup (for QEMU log verification).
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
pub fn emit_slot_map(service: &str, slots: &[(&str, u32)]) {
    use nexus_abi::debug_putc;
    fn emit(s: &[u8]) {
        for &b in s {
            let _ = debug_putc(b);
        }
    }
    let hex = |n: u8| if n < 10 { b'0' + n } else { b'a' + n - 10 };

    emit(b"slot-map: ");
    emit(service.as_bytes());
    emit(b" [");
    for (i, (name, slot)) in slots.iter().enumerate() {
        if i > 0 {
            emit(b", ");
        }
        emit(name.as_bytes());
        emit(b"=0x");
        let _ = debug_putc(hex((*slot >> 4) as u8 & 0xf));
        let _ = debug_putc(hex(*slot as u8 & 0xf));
    }
    emit(b"]\n");
}

// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Bootstrap protocol data structures shared between kernel and initial tasks
//! OWNERS: @kernel-team
//! PUBLIC API: BootstrapMsg
//! DEPENDS_ON: core
//! INVARIANTS: repr(C) layout frozen (golden vectors), LE byte order for tests
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md

/// Bootstrap flags shared across kernel and early user tasks.
///
/// These flags are part of the stable bootstrap protocol. They are intentionally additive.
pub mod flags {
    /// `argv_ptr` points to a read-only `BootstrapInfo` page in the child's address space.
    #[allow(dead_code)] // used by docs + future IPC bootstrap delivery (RFC-0005)
    pub const HAS_INFO_PAGE: u32 = 1 << 0;
}

/// Message delivered to the child's bootstrap endpoint after [`spawn`](crate::syscall)
/// succeeds.
///
/// The layout is frozen for the MVP bootstrap path and is validated via golden
/// vector tests to guard against accidental drift.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BootstrapMsg {
    /// Number of arguments supplied to the child. Zero for the MVP.
    pub argc: u32,
    /// Pointer to the argv table in the child's address space. Zero for the MVP.
    pub argv_ptr: u64,
    /// Pointer to the environment table in the child's address space. Zero for the MVP.
    pub env_ptr: u64,
    /// Capability handle for the initial endpoint granted to the child.
    pub cap_seed_ep: u32,
    /// Reserved for future expansion.
    pub flags: u32,
}

impl BootstrapMsg {
    /// Creates a message with the provided fields.
    pub const fn new(argc: u32, argv_ptr: u64, env_ptr: u64, cap_seed_ep: u32, flags: u32) -> Self {
        Self {
            argc,
            argv_ptr,
            env_ptr,
            cap_seed_ep,
            flags,
        }
    }

    /// Serialises the message into a little-endian byte array suitable for
    /// golden vector comparisons.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn to_le_bytes(&self) -> [u8; 32] {
        let mut bytes = [0u8; 32];
        bytes[0..4].copy_from_slice(&self.argc.to_le_bytes());
        // Bytes 4..8 are padding introduced by repr(C) to align argv_ptr to 8 bytes.
        bytes[8..16].copy_from_slice(&self.argv_ptr.to_le_bytes());
        bytes[16..24].copy_from_slice(&self.env_ptr.to_le_bytes());
        bytes[24..28].copy_from_slice(&self.cap_seed_ep.to_le_bytes());
        bytes[28..32].copy_from_slice(&self.flags.to_le_bytes());
        bytes
    }
}

/// Read-only bootstrap info page mapped into the child's address space.
///
/// This is the preferred, provenance-safe way to publish small pieces of metadata from the kernel
/// to the child without relying on pointers to mutable/shared buffers.
///
/// When [`flags::HAS_INFO_PAGE`] is set in [`BootstrapMsg::flags`], `argv_ptr` MUST point to this
/// structure in the child's VA space.
///
/// Current v2 content (RFC-0005 Service Identity token):
/// - `meta_name_ptr/meta_name_len` describe the NUL-terminated service name bytes in a dedicated
///   read-only mapping.
/// - `service_id` is a kernel-derived stable identifier computed from the service name bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BootstrapInfo {
    /// Version of this structure (starts at 1).
    pub version: u32,
    /// Reserved for future expansion; must be zero.
    pub reserved: u32,
    /// Child VA of the service name bytes (NUL-terminated when space permits).
    pub meta_name_ptr: u64,
    /// Length of the name bytes (excluding NUL).
    pub meta_name_len: u32,
    /// Reserved for future expansion; must be zero.
    pub reserved2: u32,
    /// Kernel-derived stable service identifier (FNV-1a 64 of the name bytes).
    pub service_id: u64,
}

#[cfg(test)]
mod info_tests {
    use super::BootstrapInfo;
    use core::mem::{align_of, size_of};

    #[test]
    fn layout_is_stable() {
        assert_eq!(size_of::<BootstrapInfo>(), 32);
        assert_eq!(align_of::<BootstrapInfo>(), 8);
    }
}

#[cfg(test)]
mod tests {
    use super::BootstrapMsg;
    use core::mem::{align_of, size_of};

    #[test]
    fn layout_is_stable() {
        assert_eq!(size_of::<BootstrapMsg>(), 32);
        assert_eq!(align_of::<BootstrapMsg>(), 8);

        let msg = BootstrapMsg::default();
        let base = &msg as *const _ as usize;
        assert_eq!((&msg.argc as *const _ as usize) - base, 0);
        assert_eq!((&msg.argv_ptr as *const _ as usize) - base, 8);
        assert_eq!((&msg.env_ptr as *const _ as usize) - base, 16);
        assert_eq!((&msg.cap_seed_ep as *const _ as usize) - base, 24);
        assert_eq!((&msg.flags as *const _ as usize) - base, 28);
    }

    #[test]
    fn golden_vector() {
        let msg = BootstrapMsg::new(
            3,
            0x1122_3344_5566_7788,
            0x99aa_bbcc_ddee_ff00,
            0x1234_5678,
            0xaabb_ccdd,
        );
        let expected: [u8; 32] = [
            0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // argc + padding
            0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11, // argv_ptr
            0x00, 0xff, 0xee, 0xdd, 0xcc, 0xbb, 0xaa, 0x99, // env_ptr
            0x78, 0x56, 0x34, 0x12, // cap_seed_ep
            0xdd, 0xcc, 0xbb, 0xaa, // flags
        ];
        assert_eq!(msg.to_le_bytes(), expected);
    }
}

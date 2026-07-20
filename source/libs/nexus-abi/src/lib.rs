// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]
#![cfg_attr(
    not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")),
    forbid(unsafe_code)
)]
#![deny(clippy::all, missing_docs)]

//! CONTEXT: Shared ABI definitions exposed to userland crates
//! OWNERS: @runtime
//! PUBLIC API: MsgHeader, IpcError; OS-only syscalls: yield_, spawn, exit, wait, cap_transfer, as_*, vmo_*, debug_*
//! DEPENDS_ON: no_std (OS), riscv ecall asm (OS), bitflags
//! INVARIANTS: Header is 16 bytes LE; userspace wrappers map to stable kernel syscall IDs
//! ADR: docs/adr/0016-kernel-libs-architecture.md

/// Result type returned by ABI helpers.
pub type Result<T> = core::result::Result<T, IpcError>;

/// Errors surfaced by IPC syscalls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IpcError {
    /// Referenced endpoint is not present in the router.
    NoSuchEndpoint,
    /// Target queue ran out of space.
    QueueFull,
    /// Queue did not contain a message when operating in non-blocking mode.
    QueueEmpty,
    /// Caller lacks permission to perform the requested operation.
    PermissionDenied,
    /// Blocking IPC operation hit its deadline.
    TimedOut,
    /// Not enough resources to complete the IPC operation (e.g. receiver cap table full).
    NoSpace,
    /// IPC is not supported for this configuration.
    Unsupported,
}

/// IPC message header shared between kernel and userland.
#[repr(C, align(4))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MsgHeader {
    /// Source capability slot.
    pub src: u32,
    /// Destination endpoint identifier.
    pub dst: u32,
    /// Message opcode.
    pub ty: u16,
    /// Transport flags.
    pub flags: u16,
    /// Inline payload length.
    pub len: u32,
}

/// IPC message header flags.
///
/// These are interpreted by the kernel IPC transport (not by service-level protocols).
pub mod ipc_hdr {
    /// Move one capability with the message (Phase‑2 scalability/hardening).
    ///
    /// When sending with this flag:
    /// - `MsgHeader.src` is treated as a **capability slot** in the sender and is **consumed**.
    /// - On receive, `MsgHeader.src` is overwritten with the **newly allocated capability slot**
    ///   in the receiver.
    pub const CAP_MOVE: u16 = 1 << 0;
}

// ADR-0051: service wire protocols live in the declarative SSOT crate
// `nexus-wire`; the re-exports below keep the historical `nexus_abi::<svc>`
// paths compiling unchanged (transitional shim — consumers migrate to
// `nexus_wire::<svc>` in a follow-up task).
pub use nexus_wire::{
    bundleimg, bundlemgrd, execd, policy, policyd, routing, sessiond, settingsd, updated,
};

/// Computes a stable service identifier from the UTF-8 service name bytes.
///
/// This is the userspace mirror of the kernel's `BootstrapInfo.service_id` derivation.
pub fn service_id_from_name(name: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325u64;
    for &b in name {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3u64);
    }
    h
}

impl MsgHeader {
    /// Creates a new header with the provided fields.
    pub const fn new(src: u32, dst: u32, ty: u16, flags: u16, len: u32) -> Self {
        Self { src, dst, ty, flags, len }
    }

    /// Serialises the header to a little-endian byte array.
    pub fn to_le_bytes(&self) -> [u8; 16] {
        let mut buf = [0_u8; 16];
        buf[0..4].copy_from_slice(&self.src.to_le_bytes());
        buf[4..8].copy_from_slice(&self.dst.to_le_bytes());
        buf[8..10].copy_from_slice(&self.ty.to_le_bytes());
        buf[10..12].copy_from_slice(&self.flags.to_le_bytes());
        buf[12..16].copy_from_slice(&self.len.to_le_bytes());
        buf
    }

    /// Deserialises a little-endian byte array into a header.
    pub fn from_le_bytes(bytes: [u8; 16]) -> Self {
        let src = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let dst = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        let ty = u16::from_le_bytes([bytes[8], bytes[9]]);
        let flags = u16::from_le_bytes([bytes[10], bytes[11]]);
        let len = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
        Self { src, dst, ty, flags, len }
    }
}

pub mod syscall;
// The syscall surface stays flat at the crate root — `nexus_abi::yield_`,
// `nexus_abi::sched::…`, `nexus_abi::page_flags::…` all keep resolving.
pub use syscall::*;

#[cfg(nexus_env = "os")]
pub mod slot_probe;

/// Deterministic userspace ABI syscall filter profile helpers.
pub mod abi_filter;

#[cfg(test)]
mod tests {
    use super::{IpcRecvV2Desc, MsgHeader};
    use core::mem::{align_of, size_of};

    #[test]
    fn header_layout() {
        assert_eq!(size_of::<MsgHeader>(), 16);
        assert_eq!(align_of::<MsgHeader>(), 4);
    }

    #[test]
    fn header_golden_vector() {
        // Inline golden vector (LE):
        // src=0x01020304, dst=0x11223344, ty=0x5566, flags=0x7788, len=0x99aabbcc
        const VECTOR: [u8; 16] = [
            0x04, 0x03, 0x02, 0x01, 0x44, 0x33, 0x22, 0x11, 0x66, 0x55, 0x88, 0x77, 0xCC, 0xBB,
            0xAA, 0x99,
        ];
        let header = MsgHeader::new(0x0102_0304, 0x1122_3344, 0x5566, 0x7788, 0x99aa_bbcc);
        assert_eq!(
            header.to_le_bytes(),
            VECTOR,
            "golden vector out of date; expected bytes: {:02x?}",
            header.to_le_bytes()
        );
        assert_eq!(MsgHeader::from_le_bytes(VECTOR), header);
    }

    #[test]
    fn round_trip() {
        let header = MsgHeader::new(1, 2, 3, 4, 5);
        assert_eq!(header, MsgHeader::from_le_bytes(header.to_le_bytes()));
    }

    #[test]
    fn recv_v2_desc_layout() {
        use core::mem::offset_of;

        assert_eq!(size_of::<IpcRecvV2Desc>(), 64);
        assert_eq!(align_of::<IpcRecvV2Desc>(), 8);

        // Offsets are part of the descriptor ABI contract.
        assert_eq!(offset_of!(IpcRecvV2Desc, magic), 0);
        assert_eq!(offset_of!(IpcRecvV2Desc, version), 4);
        assert_eq!(offset_of!(IpcRecvV2Desc, slot), 8);
        assert_eq!(offset_of!(IpcRecvV2Desc, _pad0), 12);
        assert_eq!(offset_of!(IpcRecvV2Desc, header_out_ptr), 16);
        assert_eq!(offset_of!(IpcRecvV2Desc, payload_out_ptr), 24);
        assert_eq!(offset_of!(IpcRecvV2Desc, payload_out_max), 32);
        assert_eq!(offset_of!(IpcRecvV2Desc, sender_service_id_out_ptr), 40);
        assert_eq!(offset_of!(IpcRecvV2Desc, sys_flags), 48);
        assert_eq!(offset_of!(IpcRecvV2Desc, _pad1), 52);
        assert_eq!(offset_of!(IpcRecvV2Desc, deadline_ns), 56);
    }
}

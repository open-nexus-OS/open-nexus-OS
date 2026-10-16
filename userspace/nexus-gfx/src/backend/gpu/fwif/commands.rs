// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The commands (TASK-0329 P1): a kernel command (type, flags, 80 bytes of payload), a
//! firmware-to-host command, a client command's header — with the typed constructors the
//! device uses (health check, MMU-cache flush, kick, cleanup) and the readers for what the
//! firmware sends. Part of [`super`] (`fwif`); the sizes are pinned there.

use super::{fwccb, kccb, CMD_MAGIC, CMD_MAGIC_MASK};

/// A kernel command: type, flags, 80 bytes of payload (the union).
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug)]
pub struct KccbCmd {
    pub cmd_type: u32,
    pub kccb_flags: u32,
    pub data: [u8; 80],
}

impl KccbCmd {
    pub const SIZE: usize = 88;

    const fn empty(cmd_type: u32) -> Self {
        Self { cmd_type, kccb_flags: 0, data: [0; 80] }
    }

    /// `HEALTH_CHECK`: no payload; the firmware marks the return slot executed.
    pub const fn health_check() -> Self {
        Self::empty(kccb::HEALTH_CHECK)
    }

    /// `MMUCACHE`: flush `flags`; the firmware writes `update_value` to the sync word at
    /// `sync_fw_addr` and marks the return slot.
    pub fn mmu_cache(flags: u32, sync_fw_addr: u32, update_value: u32) -> Self {
        let mut c = Self::empty(kccb::MMUCACHE);
        c.data[0..4].copy_from_slice(&flags.to_le_bytes());
        c.data[4..8].copy_from_slice(&sync_fw_addr.to_le_bytes());
        c.data[8..12].copy_from_slice(&update_value.to_le_bytes());
        c
    }

    /// `KICK`: the firmware common context at `context_fw_addr` has new client commands up to
    /// `client_woff` (wrap mask `client_wrap_mask`); no cleanup controls.
    pub fn kick(context_fw_addr: u32, client_woff: u32, client_wrap_mask: u32) -> Self {
        let mut c = Self::empty(kccb::KICK);
        c.data[0..4].copy_from_slice(&context_fw_addr.to_le_bytes());
        c.data[4..8].copy_from_slice(&client_woff.to_le_bytes());
        c.data[8..12].copy_from_slice(&client_wrap_mask.to_le_bytes());
        // num_cleanup_ctl = 0, cleanup_ctl_fw_addr[3] = 0, work_est_cmd_header_offset = 0.
        c
    }

    /// `KICK` of a job on a render-target data: as [`Self::kick`], with the data's cleanup
    /// control named so the firmware counts the command for it (the open kernel driver's
    /// `fill_cmd_kick_data`).
    pub fn kick_with_cleanup(
        context_fw_addr: u32,
        client_woff: u32,
        client_wrap_mask: u32,
        cleanup_ctl_fw_addr: u32,
    ) -> Self {
        let mut c = Self::kick(context_fw_addr, client_woff, client_wrap_mask);
        c.data[12..16].copy_from_slice(&1u32.to_le_bytes());
        c.data[16..20].copy_from_slice(&cleanup_ctl_fw_addr.to_le_bytes());
        c
    }

    /// `CLEANUP` of the firmware object of `kind` ([`super::cleanup`]) at `fw_addr`: the
    /// firmware lets go of it — or answers busy in the return slot while it still holds work
    /// on it (the open kernel driver's `pvr_fw_structure_cleanup`).
    pub fn cleanup(kind: u32, fw_addr: u32) -> Self {
        let mut c = Self::empty(kccb::CLEANUP);
        c.data[0..4].copy_from_slice(&kind.to_le_bytes());
        c.data[4..8].copy_from_slice(&fw_addr.to_le_bytes());
        c
    }

    /// The command as bytes.
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut b = [0u8; Self::SIZE];
        b[0..4].copy_from_slice(&self.cmd_type.to_le_bytes());
        b[4..8].copy_from_slice(&self.kccb_flags.to_le_bytes());
        b[8..].copy_from_slice(&self.data);
        b
    }
}

/// A firmware-to-host command, read from the buffer.
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug)]
pub struct FwccbCmd {
    pub cmd_type: u32,
    pub fwccb_flags: u32,
    pub data: [u8; 80],
}

impl FwccbCmd {
    pub const SIZE: usize = 88;

    pub fn from_bytes(b: &[u8; Self::SIZE]) -> Self {
        let mut data = [0u8; 80];
        data.copy_from_slice(&b[8..]);
        Self {
            cmd_type: u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            fwccb_flags: u32::from_le_bytes([b[4], b[5], b[6], b[7]]),
            data,
        }
    }

    /// Whether the type carries the magic — a command without it is ignored.
    pub const fn has_magic(&self) -> bool {
        self.cmd_type & CMD_MAGIC_MASK == CMD_MAGIC
    }

    /// The type without the magic (for a line).
    pub const fn kind(&self) -> u32 {
        self.cmd_type & !CMD_MAGIC_MASK
    }

    /// A context-reset notification's fields: `(server_common_context_id, reset_reason, dm,
    /// reset_job_ref, flags, pc_address, fault_address)`.
    pub fn context_reset(&self) -> Option<(u32, u32, u32, u32, u32, u64, u64)> {
        if self.cmd_type != fwccb::CONTEXT_RESET_NOTIFICATION {
            return None;
        }
        let d = &self.data;
        let w = |o: usize| u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]]);
        let q = |o: usize| u64::from_le_bytes(d[o..o + 8].try_into().unwrap_or([0; 8]));
        Some((w(0), w(4), w(8), w(12), w(16), q(24), q(32)))
    }
}

/// A client command's header; the payload follows, 8-byte aligned.
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug, Default)]
pub struct CcbCmdHeader {
    pub cmd_type: u32,
    pub cmd_size: u32,
    pub ext_job_ref: u32,
    pub int_job_ref: u32,
    /// Work-estimation kick data (unused: zero).
    pub work_est: [u8; 24],
}

impl CcbCmdHeader {
    pub const SIZE: usize = 40;

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut b = [0u8; Self::SIZE];
        b[0..4].copy_from_slice(&self.cmd_type.to_le_bytes());
        b[4..8].copy_from_slice(&self.cmd_size.to_le_bytes());
        b[8..12].copy_from_slice(&self.ext_job_ref.to_le_bytes());
        b[12..16].copy_from_slice(&self.int_job_ref.to_le_bytes());
        b[16..].copy_from_slice(&self.work_est);
        b
    }
}

// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The commands (TASK-0329 P1): a kernel command (type, flags, 80 bytes of payload), a
//! firmware-to-host command, a client command's header — with the typed constructors the
//! device uses (health check, MMU-cache flush, kick, cleanup) and the readers for what the
//! firmware sends. Part of [`super`] (`fwif`); the sizes are pinned there.

use super::{fwccb, kccb, reset, CMD_MAGIC, CMD_MAGIC_MASK, MAX_FREELISTS_TO_RECONSTRUCT};

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

    /// `FREELIST_GROW_UPDATE`: the answer to a growth request of the free list at `fw_addr` —
    /// `delta_pages` added (zero: none), the list's pages and ready pages after it.
    pub fn freelist_grow_update(
        fw_addr: u32,
        delta_pages: u32,
        new_pages: u32,
        ready_pages: u32,
    ) -> Self {
        let mut c = Self::empty(kccb::FREELIST_GROW_UPDATE);
        for (i, v) in [fw_addr, delta_pages, new_pages, ready_pages].iter().enumerate() {
            c.data[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
        c
    }

    /// `FREELISTS_RECONSTRUCTION_UPDATE`: the lists rebuilt after a reconstruction request (at
    /// most [`MAX_FREELISTS_TO_RECONSTRUCT`]), as the request named them.
    pub fn reconstruction_update(ids: &[u32]) -> Self {
        let mut c = Self::empty(kccb::FREELISTS_RECONSTRUCTION_UPDATE);
        let ids = &ids[..ids.len().min(MAX_FREELISTS_TO_RECONSTRUCT)];
        c.data[0..4].copy_from_slice(&(ids.len() as u32).to_le_bytes());
        for (i, id) in ids.iter().enumerate() {
            c.data[4 + i * 4..8 + i * 4].copy_from_slice(&id.to_le_bytes());
        }
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

/// A context-reset notification: the firmware reset a context's work (its hardware recovery).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContextReset {
    /// The common context's id, as the device numbered it at creation.
    pub context_id: u32,
    /// [`reset`]'s reasons.
    pub reason: u32,
    /// The data master the reset hit.
    pub dm: u32,
    pub job_ref: u32,
    /// [`reset::FLAG_PF`], [`reset::FLAG_ALL_CTXS`].
    pub flags: u32,
    /// The page catalogue of the faulting context (a page fault).
    pub pc_address: u64,
    /// The address that faulted (a page fault).
    pub fault_address: u64,
}

/// The context a reset is held against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blame {
    /// The context whose page catalogue sits at this address: its work touched an address its
    /// space does not map.
    Catalogue(u64),
    /// The context of this id: its own work was reset (a lockup, an overrun, a forced switch).
    Context(u32),
    /// The context of this id lost work to another's reset; nothing is held against it.
    Innocent(u32),
}

impl ContextReset {
    /// The reset followed a page fault: `pc_address` names the context that touched an address
    /// its space does not map.
    pub const fn page_fault(&self) -> bool {
        self.flags & reset::FLAG_PF != 0
    }

    /// The faulting context's page catalogue: the page `pc_address` lies in (its low bits carry
    /// the firmware's own information — the board reported `0x6db2001` for a catalogue at
    /// `0x6db2000`).
    pub const fn catalogue(&self) -> u64 {
        self.pc_address & !0xfff
    }

    /// Whom the reset is held against. A page fault names its context by the catalogue, whatever
    /// the reason says (the board reported a page fault as an innocent lockup, reason 2, of the
    /// only context there was); otherwise the reason decides about the context the notification
    /// names: an innocent one runs on, any other reason loses it.
    pub const fn blame(&self) -> Blame {
        if self.page_fault() {
            Blame::Catalogue(self.catalogue())
        } else if matches!(self.reason, reset::INNOCENT_LOCKUP | reset::INNOCENT_OVERRUNING) {
            Blame::Innocent(self.context_id)
        } else {
            Blame::Context(self.context_id)
        }
    }
}

/// A free-list reconstruction request: the lists to rebuild after the firmware's hardware
/// recovery, and the recovery's counter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reconstruction {
    pub hwr_counter: u32,
    count: usize,
    ids: [u32; MAX_FREELISTS_TO_RECONSTRUCT],
}

impl Reconstruction {
    /// The lists named, at most [`MAX_FREELISTS_TO_RECONSTRUCT`].
    pub fn ids(&self) -> &[u32] {
        &self.ids[..self.count]
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

    fn word(&self, at: usize) -> u32 {
        u32::from_le_bytes([self.data[at], self.data[at + 1], self.data[at + 2], self.data[at + 3]])
    }

    fn dword(&self, at: usize) -> u64 {
        u64::from(self.word(at)) | (u64::from(self.word(at + 4)) << 32)
    }

    /// A context-reset notification.
    pub fn context_reset(&self) -> Option<ContextReset> {
        (self.cmd_type == fwccb::CONTEXT_RESET_NOTIFICATION).then(|| ContextReset {
            context_id: self.word(0),
            reason: self.word(4),
            dm: self.word(8),
            job_ref: self.word(12),
            flags: self.word(16),
            pc_address: self.dword(24),
            fault_address: self.dword(32),
        })
    }

    /// A free list's growth request: the list's id.
    pub fn freelist_grow(&self) -> Option<u32> {
        (self.cmd_type == fwccb::FREELIST_GROW).then(|| self.word(0))
    }

    /// A free-list reconstruction request (a count beyond the message's room is held to it).
    pub fn reconstruction(&self) -> Option<Reconstruction> {
        if self.cmd_type != fwccb::FREELISTS_RECONSTRUCTION {
            return None;
        }
        let count = (self.word(0) as usize).min(MAX_FREELISTS_TO_RECONSTRUCT);
        let mut ids = [0u32; MAX_FREELISTS_TO_RECONSTRUCT];
        for (i, id) in ids.iter_mut().enumerate().take(count) {
            *id = self.word(8 + i * 4);
        }
        Some(Reconstruction { hwr_counter: self.word(4), count, ids })
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

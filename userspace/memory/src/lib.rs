// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Host-first VMO plumbing with bounded zero-copy semantics.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Host integration tests under `tests/` (`cargo test -p nexus-vmo`)
//! ADR: docs/rfcs/RFC-0040-zero-copy-vmos-v1-plumbing-host-first-os-gated.md

#![forbid(unsafe_code)]
#![deny(clippy::all, missing_docs)]
#![allow(unexpected_cfgs)]
#![cfg_attr(
    all(feature = "os-lite", nexus_env = "os", target_arch = "riscv64", target_os = "none"),
    no_std
)]

extern crate alloc;

use alloc::collections::BTreeSet;
#[cfg(nexus_env = "host")]
use alloc::vec;
#[cfg(nexus_env = "host")]
use alloc::vec::Vec;
use core::ops::{BitOr, BitOrAssign};
use core::sync::atomic::{AtomicU64, Ordering};

#[cfg(all(nexus_env = "host", nexus_env = "os"))]
compile_error!("nexus_env: both 'host' and 'os' set");

#[cfg(not(any(nexus_env = "host", nexus_env = "os")))]
compile_error!("nexus_env: missing. Set RUSTFLAGS='--cfg nexus_env=\"host\"' or '...\"os\"'");

const MAX_VMO_BYTES: usize = 8 * 1024 * 1024;
const TRANSFER_CONTROL_BYTES: u64 = 16;
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
const PAGE_SIZE_BYTES: usize = 4096;

static NEXT_VMO_ID: AtomicU64 = AtomicU64::new(1);

/// Result type used by this crate.
pub type Result<T> = core::result::Result<T, Error>;

/// Opaque identifier for a logical VMO instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct VmoId(u64);

impl VmoId {
    /// Returns the stable raw identifier.
    pub const fn raw(self) -> u64 {
        self.0
    }
}

/// Peer process identifier used for capability transfer decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct PeerPid(u32);

impl PeerPid {
    /// Creates a typed PID wrapper.
    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    /// Returns the raw kernel PID.
    pub const fn raw(self) -> u32 {
        self.0
    }
}

/// Capability-rights mask used during transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct TransferRights(u32);

impl TransferRights {
    /// No rights.
    pub const NONE: Self = Self(0);
    /// Permit endpoint send operations.
    pub const SEND: Self = Self(1 << 0);
    /// Permit endpoint receive operations.
    pub const RECV: Self = Self(1 << 1);
    /// Permit VMO mapping in the destination.
    pub const MAP: Self = Self(1 << 2);

    /// Returns true when all bits in `other` are present.
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    /// Returns the raw bit mask.
    pub const fn bits(self) -> u32 {
        self.0
    }
}

impl BitOr for TransferRights {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for TransferRights {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

/// Deterministic accounting counters for zero-copy honesty checks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VmoCounters {
    /// Number of fallback copies performed by transfer operations.
    pub copy_fallback_count: u64,
    /// Bytes moved over the control plane.
    pub control_plane_bytes: u64,
    /// Bytes moved over the bulk path (writes + fallback copies).
    pub bulk_bytes: u64,
    /// Number of map operations that reused an existing established map path.
    pub map_reuse_hits: u64,
    /// Number of map operations that had to establish a new mapping path.
    pub map_reuse_misses: u64,
}

/// VMO transfer outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use = "transfer outcomes carry behavior proof details"]
pub enum TransferOutcome {
    /// Host mode has no kernel-capability move path, so transfer copies bytes.
    HostCopyFallback {
        /// Number of bytes copied by the host fallback path.
        copied_bytes: usize,
    },
    /// OS mode moved the capability to a destination slot.
    #[cfg(all(nexus_env = "os", feature = "os-lite"))]
    OsTransferred {
        /// Destination capability slot allocated by kernel transfer.
        dst_slot: u32,
    },
}

/// Read-only mapping view over a VMO byte range.
#[derive(Debug, Clone, Copy)]
pub struct VmoMapping<'a> {
    bytes: &'a [u8],
}

impl<'a> VmoMapping<'a> {
    /// Returns the read-only mapped bytes.
    pub const fn as_slice(&self) -> &'a [u8] {
        self.bytes
    }
}

/// Read-only slice view used by host-side fixture-style consumers.
#[derive(Debug, Clone, Copy)]
pub struct VmoSlice<'a> {
    bytes: &'a [u8],
}

impl<'a> VmoSlice<'a> {
    /// Returns the read-only bytes.
    pub const fn as_slice(&self) -> &'a [u8] {
        self.bytes
    }

    /// Returns the slice length.
    pub const fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Returns true when the slice is empty.
    pub const fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

/// VMO operation errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use = "vmo errors represent safety or policy decisions"]
pub enum Error {
    /// Length or alignment violates bounded invariants.
    InvalidLength,
    /// Offset/length exceed VMO bounds.
    OutOfBounds,
    /// Caller attempted to write a sealed read-only VMO.
    ReadOnlyViolation,
    /// Destination peer is not authorized by local policy.
    UnauthorizedTransfer,
    /// Requested rights are missing required capabilities.
    RightsMismatch,
    /// Operation is unsupported on this backend.
    Unsupported,
    /// Kernel syscall failed.
    KernelFailure,
    /// Host file-range loader failed.
    IoFailure,
}

/// Typed userspace VMO abstraction.
pub struct Vmo {
    id: VmoId,
    len: usize,
    sealed_ro: bool,
    allowed_peers: BTreeSet<PeerPid>,
    counters: VmoCounters,
    mapped_once: bool,
    #[cfg(nexus_env = "host")]
    bytes: Vec<u8>,
    #[cfg(all(nexus_env = "os", feature = "os-lite"))]
    handle: nexus_abi::Handle,
}

impl Vmo {
    /// Creates a bounded VMO instance.
    pub fn create(len: usize) -> Result<Self> {
        if len == 0 || len > MAX_VMO_BYTES {
            return Err(Error::InvalidLength);
        }
        let id = VmoId(NEXT_VMO_ID.fetch_add(1, Ordering::Relaxed));
        #[cfg(nexus_env = "host")]
        {
            Ok(Self {
                id,
                len,
                sealed_ro: false,
                allowed_peers: BTreeSet::new(),
                counters: VmoCounters::default(),
                mapped_once: false,
                bytes: vec![0u8; len],
            })
        }
        #[cfg(all(nexus_env = "os", feature = "os-lite"))]
        {
            let handle = nexus_abi::vmo_create(len).map_err(map_ipc_error)?;
            Ok(Self {
                id,
                len,
                sealed_ro: false,
                allowed_peers: BTreeSet::new(),
                counters: VmoCounters::default(),
                mapped_once: false,
                handle,
            })
        }
        #[cfg(all(nexus_env = "os", not(feature = "os-lite")))]
        {
            let _ = id;
            Err(Error::Unsupported)
        }
    }

    /// Returns the typed VMO ID.
    pub const fn id(&self) -> VmoId {
        self.id
    }

    /// Returns the VMO length in bytes.
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Returns true when this VMO has zero length.
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Builds a VMO from an in-memory byte slice.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let mut vmo = Self::create(bytes.len())?;
        vmo.write(0, bytes)?;
        Ok(vmo)
    }

    /// Builds a VMO by loading a bounded file range (host-only helper).
    #[cfg(nexus_env = "host")]
    pub fn from_file_range(path: &std::path::Path, offset: u64, len: usize) -> Result<Self> {
        use std::io::{Read, Seek, SeekFrom};

        if len == 0 {
            return Err(Error::InvalidLength);
        }
        let mut file = std::fs::File::open(path).map_err(|_| Error::IoFailure)?;
        file.seek(SeekFrom::Start(offset)).map_err(|_| Error::IoFailure)?;
        let mut buf = vec![0u8; len];
        file.read_exact(&mut buf).map_err(|_| Error::IoFailure)?;
        Self::from_bytes(&buf)
    }

    /// Returns a snapshot of deterministic counters.
    pub const fn counters(&self) -> VmoCounters {
        self.counters
    }

    /// Seals the VMO as read-only from the crate's perspective.
    pub fn seal_ro(&mut self) {
        self.sealed_ro = true;
    }

    /// Marks a peer as authorized for transfer requests.
    pub fn authorize_transfer_to(&mut self, peer: PeerPid) {
        let _ = self.allowed_peers.insert(peer);
    }

    /// Writes bytes into the VMO at `offset`.
    pub fn write(&mut self, offset: usize, bytes: &[u8]) -> Result<()> {
        if self.sealed_ro {
            return Err(Error::ReadOnlyViolation);
        }
        if bytes.is_empty() {
            return Ok(());
        }
        let end = offset.checked_add(bytes.len()).ok_or(Error::OutOfBounds)?;
        if end > self.len {
            return Err(Error::OutOfBounds);
        }
        #[cfg(nexus_env = "host")]
        {
            self.bytes[offset..end].copy_from_slice(bytes);
        }
        #[cfg(all(nexus_env = "os", feature = "os-lite"))]
        {
            nexus_abi::vmo_write(self.handle, offset, bytes).map_err(map_ipc_error)?;
        }
        #[cfg(all(nexus_env = "os", not(feature = "os-lite")))]
        {
            return Err(Error::Unsupported);
        }
        self.counters.bulk_bytes = self.counters.bulk_bytes.saturating_add(bytes.len() as u64);
        Ok(())
    }

    /// Maps a bounded read-only byte range on host builds.
    pub fn map_ro(&mut self, offset: usize, len: usize) -> Result<VmoMapping<'_>> {
        if len == 0 {
            return Err(Error::InvalidLength);
        }
        let end = offset.checked_add(len).ok_or(Error::OutOfBounds)?;
        if end > self.len {
            return Err(Error::OutOfBounds);
        }
        self.record_map_reuse();
        #[cfg(nexus_env = "host")]
        {
            Ok(VmoMapping { bytes: &self.bytes[offset..end] })
        }
        #[cfg(nexus_env = "os")]
        {
            let _ = (offset, end);
            Err(Error::Unsupported)
        }
    }

    /// Returns a bounded host-side read-only slice.
    pub fn slice(&self, offset: usize, len: usize) -> Result<VmoSlice<'_>> {
        if len == 0 {
            return Err(Error::InvalidLength);
        }
        let end = offset.checked_add(len).ok_or(Error::OutOfBounds)?;
        if end > self.len {
            return Err(Error::OutOfBounds);
        }
        #[cfg(nexus_env = "host")]
        {
            Ok(VmoSlice { bytes: &self.bytes[offset..end] })
        }
        #[cfg(nexus_env = "os")]
        {
            let _ = (offset, end);
            Err(Error::Unsupported)
        }
    }

    /// Transfers the VMO capability to `peer` with rights filtering.
    #[must_use = "transfer decisions must be handled explicitly"]
    pub fn transfer_to(
        &mut self,
        peer: PeerPid,
        rights: TransferRights,
    ) -> Result<TransferOutcome> {
        if !self.allowed_peers.contains(&peer) {
            return Err(Error::UnauthorizedTransfer);
        }
        if !rights.contains(TransferRights::MAP) {
            return Err(Error::RightsMismatch);
        }
        self.counters.control_plane_bytes =
            self.counters.control_plane_bytes.saturating_add(TRANSFER_CONTROL_BYTES);
        #[cfg(nexus_env = "host")]
        {
            self.counters.copy_fallback_count = self.counters.copy_fallback_count.saturating_add(1);
            self.counters.bulk_bytes = self.counters.bulk_bytes.saturating_add(self.len as u64);
            Ok(TransferOutcome::HostCopyFallback { copied_bytes: self.len })
        }
        #[cfg(all(nexus_env = "os", feature = "os-lite"))]
        {
            let os_rights = map_transfer_rights(rights)?;
            let dst_slot = nexus_abi::cap_transfer(peer.raw(), self.handle, os_rights)
                .map_err(map_abi_error)?;
            Ok(TransferOutcome::OsTransferred { dst_slot })
        }
        #[cfg(all(nexus_env = "os", not(feature = "os-lite")))]
        {
            let _ = (peer, rights);
            Err(Error::Unsupported)
        }
    }

    /// Transfers the VMO capability to a specific destination slot in `peer`.
    #[must_use = "slot-directed transfer decisions must be handled explicitly"]
    pub fn transfer_to_slot(
        &mut self,
        peer: PeerPid,
        rights: TransferRights,
        dst_slot: u32,
    ) -> Result<TransferOutcome> {
        if !self.allowed_peers.contains(&peer) {
            return Err(Error::UnauthorizedTransfer);
        }
        if !rights.contains(TransferRights::MAP) {
            return Err(Error::RightsMismatch);
        }
        self.counters.control_plane_bytes =
            self.counters.control_plane_bytes.saturating_add(TRANSFER_CONTROL_BYTES);
        #[cfg(nexus_env = "host")]
        {
            let _ = dst_slot;
            Err(Error::Unsupported)
        }
        #[cfg(all(nexus_env = "os", feature = "os-lite"))]
        {
            let os_rights = map_transfer_rights(rights)?;
            let slot =
                nexus_abi::cap_transfer_to_slot(peer.raw(), self.handle, os_rights, dst_slot)
                    .map_err(map_abi_error)?;
            Ok(TransferOutcome::OsTransferred { dst_slot: slot })
        }
        #[cfg(all(nexus_env = "os", not(feature = "os-lite")))]
        {
            let _ = (peer, rights, dst_slot);
            Err(Error::Unsupported)
        }
    }

    fn record_map_reuse(&mut self) {
        if self.mapped_once {
            self.counters.map_reuse_hits = self.counters.map_reuse_hits.saturating_add(1);
        } else {
            self.counters.map_reuse_misses = self.counters.map_reuse_misses.saturating_add(1);
            self.mapped_once = true;
        }
    }

    /// Maps page-aligned, read-only pages into the current address space on OS builds.
    #[cfg(all(nexus_env = "os", feature = "os-lite"))]
    pub fn map_ro_pages(&mut self, va: usize, offset: usize, len: usize) -> Result<()> {
        if len == 0 || (va % PAGE_SIZE_BYTES) != 0 || (offset % PAGE_SIZE_BYTES) != 0 {
            return Err(Error::InvalidLength);
        }
        if (len % PAGE_SIZE_BYTES) != 0 {
            return Err(Error::InvalidLength);
        }
        let end = offset.checked_add(len).ok_or(Error::OutOfBounds)?;
        if end > self.len {
            return Err(Error::OutOfBounds);
        }
        self.record_map_reuse();
        let flags = nexus_abi::page_flags::VALID
            | nexus_abi::page_flags::READ
            | nexus_abi::page_flags::USER;
        let mut mapped = 0usize;
        while mapped < len {
            nexus_abi::vmo_map_page_sys(self.handle, va + mapped, offset + mapped, flags)
                .map_err(map_abi_error)?;
            mapped += PAGE_SIZE_BYTES;
        }
        Ok(())
    }

    /// Returns the raw OS handle when available.
    #[cfg(all(nexus_env = "os", feature = "os-lite"))]
    pub const fn raw_handle(&self) -> nexus_abi::Handle {
        self.handle
    }
}

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
fn map_transfer_rights(rights: TransferRights) -> Result<nexus_abi::Rights> {
    let mut out = nexus_abi::Rights::empty();
    if rights.contains(TransferRights::SEND) {
        out |= nexus_abi::Rights::SEND;
    }
    if rights.contains(TransferRights::RECV) {
        out |= nexus_abi::Rights::RECV;
    }
    if rights.contains(TransferRights::MAP) {
        out |= nexus_abi::Rights::MAP;
    }
    if out.is_empty() {
        return Err(Error::RightsMismatch);
    }
    Ok(out)
}

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
fn map_ipc_error(err: nexus_abi::IpcError) -> Error {
    match err {
        nexus_abi::IpcError::Unsupported => Error::Unsupported,
        _ => Error::KernelFailure,
    }
}

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
fn map_abi_error(err: nexus_abi::AbiError) -> Error {
    match err {
        nexus_abi::AbiError::Unsupported | nexus_abi::AbiError::InvalidSyscall => {
            Error::Unsupported
        }
        _ => Error::KernelFailure,
    }
}

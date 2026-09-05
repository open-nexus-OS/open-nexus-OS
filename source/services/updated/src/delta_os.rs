// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(all(nexus_env = "os", feature = "os-lite"))]
#![forbid(unsafe_code)]

//! CONTEXT: the OS half of `boot-image-delta` staging (RFC-0090,
//! TASK-0034). `RemoteBase` gives the generic `DeltaAdapter` byte-
//! addressed reads of the ACTIVE slot's image body (virtioblkd already
//! grants `updated` both boot partitions — the engine scopes the SLOT:
//! writes go only to the inactive one) plus the base identity from the
//! active NXBD sector, which the LOADER verified this very boot (the
//! RFC-0090 O(1) base binding; a direct-kernel dev boot has no valid
//! active NXBD and honestly rejects `delta-base`). `StageSink` is the
//! per-kind dispatch the engine drives: kind 1 = the plain `SlotSink`,
//! kind 3 = `DeltaAdapter<RemoteBase, SlotSink>` — reconstruction flows
//! through the UNCHANGED readback/NXBD-last tail.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: adapter logic host-proven in
//! tests/updates_host/tests/component_set_delta.rs; transport via the
//! QEMU delta markers (TASK-0034).
//! ADR: docs/rfcs/RFC-0090-nxdelta-boot-image-delta-stream-format.md

extern crate alloc;

use alloc::vec::Vec;

use storage::{blockproto, remote_blk::RemoteBlockDevice, BlockDevice};
use updates::bundle_delta::SetSink;
use updates::component_set::{
    ComponentMeta, ComponentSink, RejectReason, KIND_BOOT_IMAGE, KIND_BOOT_IMAGE_DELTA,
    KIND_BUNDLE, KIND_BUNDLE_DELTA, KIND_SYSTEM_VOLUME,
};
use updates::delta_apply::{BaseIdentity, BaseRead, DeltaAdapter};
use updates::volume_apply::VolumeAssembler;
use updates::Slot;

use crate::apply_os::{SlotSink, IMAGE_START_SECTOR, SECTOR};
use crate::volume_os::{BlockVolumeDev, VolumeMarkers};

/// Byte-addressed reader over the ACTIVE slot's image body.
pub(crate) struct RemoteBase {
    dev: RemoteBlockDevice,
    scratch: Vec<u8>,
}

impl RemoteBase {
    /// Attaches the active slot and reads its NXBD for the base identity.
    /// No valid descriptor (direct-kernel boot, blank slot) is an honest
    /// `delta-base` — there is nothing loader-verified to bind against.
    fn attach(active: Slot) -> Result<(Self, BaseIdentity), RejectReason> {
        let part = match active {
            Slot::A => blockproto::PART_BOOT_A,
            Slot::B => blockproto::PART_BOOT_B,
        };
        let dev = RemoteBlockDevice::open_with_deadline(
            blockproto::CLIENT_REQ_SLOT,
            blockproto::CLIENT_REPLY_SEND_SLOT,
            blockproto::CLIENT_REPLY_RECV_SLOT,
            part,
            2_000_000_000,
        )
        .ok_or(RejectReason::Io)?;
        let mut sector = [0u8; SECTOR];
        dev.read_blocks(0, &mut sector).map_err(|_| RejectReason::Io)?;
        let identity = updates::delta_apply::base_identity_from_sector(&sector)?;
        Ok((Self { dev, scratch: alloc::vec![0u8; 32 * SECTOR] }, identity))
    }
}

impl BaseRead for RemoteBase {
    fn read_at(&mut self, off: u64, buf: &mut [u8]) -> Result<(), RejectReason> {
        let mut done = 0usize;
        while done < buf.len() {
            let abs = off + done as u64;
            let sector = IMAGE_START_SECTOR + abs / SECTOR as u64;
            let skip = (abs % SECTOR as u64) as usize;
            let want = buf.len() - done;
            // One bounded scratch window per pass: sector-padded span that
            // covers `skip + take` bytes.
            let take = want.min(self.scratch.len() - skip);
            let padded = (skip + take).div_ceil(SECTOR) * SECTOR;
            self.dev
                .read_blocks(sector, &mut self.scratch[..padded])
                .map_err(|_| RejectReason::Io)?;
            buf[done..done + take].copy_from_slice(&self.scratch[skip..skip + take]);
            done += take;
        }
        Ok(())
    }
}

/// The engine-facing per-kind dispatch: kinds 1/3 = boot slot (per
/// component), kinds 6/2 = the system-volume assembler (RFC-0089 §12.4,
/// TASK-0321 P3) which lives ACROSS components and commits at
/// `commit_set` (NXSV last).
pub(crate) struct StageSink {
    active: Slot,
    inactive: Slot,
    state: State,
    /// The set's boot-image digest (from its NXBD) — the volume's pairing
    /// target; a volume-only set pairs with the ACTIVE NXBD instead.
    boot_digest: Option<[u8; 32]>,
    /// Kinds 6/2/4 — the assembler behind the kind-4 dispatcher
    /// (TASK-0035 P3: `bundle-delta` bases are fresh read handles on the
    /// ACTIVE system partition).
    volume: Option<VolumeSet>,
}

type VolumeSet = SetSink<
    BlockVolumeDev,
    BlockVolumeDev,
    VolumeMarkers,
    alloc::boxed::Box<dyn FnMut() -> Option<BlockVolumeDev>>,
>;

enum State {
    Idle,
    Full(SlotSink),
    Delta(DeltaAdapter<RemoteBase, SlotSink>),
    /// Kinds 6/2 — the assembler in `StageSink::volume`.
    Volume,
}

impl StageSink {
    pub(crate) fn new(active: Slot, inactive: Slot) -> Self {
        Self { active, inactive, state: State::Idle, boot_digest: None, volume: None }
    }
}

impl ComponentSink for StageSink {
    fn begin(&mut self, meta: &ComponentMeta) -> Result<(), RejectReason> {
        // Fresh per component (the engine re-begins on multi-component
        // sets exactly as it did with the bare SlotSink).
        self.state = State::Idle;
        if meta.kind == KIND_BOOT_IMAGE {
            let mut sink = SlotSink::attach(self.inactive)?;
            sink.begin(meta)?;
            self.state = State::Full(sink);
            self.boot_digest = Some(meta.sha256);
            return Ok(());
        }
        if meta.kind == KIND_SYSTEM_VOLUME {
            // TASK-0321 P3: the assembler over the INACTIVE system slot,
            // reusing unchanged bundles from the ACTIVE one; pairing target
            // = this set's boot image, else the loader-verified active NXBD.
            let pair =
                self.boot_digest.or_else(|| crate::volume_os::active_nxbd_digest(self.active));
            if pair.is_none() {
                return Err(RejectReason::VolumeBinding);
            }
            let inactive = BlockVolumeDev::attach(self.inactive)?;
            let active = BlockVolumeDev::attach(self.active).ok();
            let assembler = VolumeAssembler::new(inactive, active, pair, VolumeMarkers);
            let active_slot = self.active;
            let base_factory: alloc::boxed::Box<dyn FnMut() -> Option<BlockVolumeDev>> =
                alloc::boxed::Box::new(move || BlockVolumeDev::attach(active_slot).ok());
            let mut set = SetSink::new(assembler, base_factory);
            set.begin(meta)?;
            self.volume = Some(set);
            self.state = State::Volume;
            return Ok(());
        }
        if meta.kind == KIND_BUNDLE || meta.kind == KIND_BUNDLE_DELTA {
            let set = self.volume.as_mut().ok_or(RejectReason::Order)?;
            set.begin(meta)?;
            self.state = State::Volume;
            return Ok(());
        }
        if meta.kind != KIND_BOOT_IMAGE_DELTA {
            return Err(RejectReason::KindUnsupported);
        }
        let (base, identity) = RemoteBase::attach(self.active)?;
        let inner = SlotSink::attach(self.inactive)?;
        let mut adapter = DeltaAdapter::new(base, inner, identity);
        adapter.begin(meta)?;
        self.state = State::Delta(adapter);
        self.boot_digest = bootfmt::nxbd::decode(&meta.kind_data).ok().map(|(d, _)| d.image_sha256);
        Ok(())
    }

    fn chunk(&mut self, offset: u64, bytes: &[u8]) -> Result<(), RejectReason> {
        match &mut self.state {
            State::Full(sink) => sink.chunk(offset, bytes),
            State::Delta(adapter) => adapter.chunk(offset, bytes),
            State::Volume => self.volume.as_mut().ok_or(RejectReason::Io)?.chunk(offset, bytes),
            State::Idle => Err(RejectReason::Io),
        }
    }

    fn finish(&mut self, meta: &ComponentMeta) -> Result<(), RejectReason> {
        match &mut self.state {
            State::Full(sink) => sink.finish(meta),
            State::Delta(adapter) => adapter.finish(meta),
            State::Volume => self.volume.as_mut().ok_or(RejectReason::Io)?.finish(meta),
            State::Idle => Err(RejectReason::Io),
        }
    }

    fn commit_set(&mut self) -> Result<(), RejectReason> {
        match self.volume.as_mut() {
            Some(set) => set.commit_set(),
            None => Ok(()),
        }
    }
}

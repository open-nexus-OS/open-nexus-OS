// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

//! CONTEXT: `bundle-delta` (RFC-0089 §12.4 kind 4, TASK-0035 P3) on the
//! system-volume seam: one `.nxdelta` stream per changed bundle whose BASE
//! is the ACTIVE volume's window with the digest the component names in
//! `kind_data`. `VolumeBase` gives the unchanged RFC-0090 `DeltaAdapter`
//! byte-addressed reads of that window; `SetSink` is the per-set
//! dispatcher over kinds 6/2/4 that hands the assembler to the adapter for
//! a kind-4 component and takes it back at `finish`. Base binding happens
//! before any write (`delta-base`); the reconstructed bytes go through the
//! assembler's normal window path (readback-verified against the NEW
//! index), so the assembled volume stays byte-identical to the host build.
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: tests/updates_host/tests/component_set_bundle_delta.rs
//!   (accept + `test_reject_delta_base_bundle` + byte-identity)
//! ADR: docs/adr/0060-verified-system-volume-bundlemgrd-verifier-init-spawner.md

#[cfg(all(feature = "os-lite", not(feature = "std")))]
use alloc::vec::Vec;
#[cfg(feature = "std")]
use std::vec::Vec;

use crate::component_set::{
    ComponentMeta, ComponentSink, RejectReason, KIND_BUNDLE, KIND_BUNDLE_DELTA, KIND_SYSTEM_VOLUME,
};
use crate::delta_apply::{BaseIdentity, BaseRead, DeltaAdapter};
use crate::volume_apply::{VolumeAssembler, VolumeDev, VolumeEvents, VOLUME_START_SECTOR};

const SECTOR: usize = 512;

/// Byte-addressed reads of ONE window of the ACTIVE volume (the delta base).
pub struct VolumeBase<A: VolumeDev> {
    dev: A,
    /// Window start as a volume byte offset (data section + row offset).
    start: u64,
    len: u64,
    scratch: Vec<u8>,
}

impl<A: VolumeDev> VolumeBase<A> {
    pub fn new(dev: A, start: u64, len: u64) -> Self {
        Self { dev, start, len, scratch: Vec::new() }
    }
}

impl<A: VolumeDev> BaseRead for VolumeBase<A> {
    fn read_at(&mut self, off: u64, buf: &mut [u8]) -> Result<(), RejectReason> {
        let end = off.checked_add(buf.len() as u64).ok_or(RejectReason::DeltaFormat)?;
        if end > self.len {
            return Err(RejectReason::DeltaFormat);
        }
        let abs = self.start + off;
        let lba = VOLUME_START_SECTOR + abs / SECTOR as u64;
        let skew = (abs % SECTOR as u64) as usize;
        let padded = (skew + buf.len()).div_ceil(SECTOR) * SECTOR;
        if self.scratch.len() < padded {
            self.scratch.resize(padded, 0);
        }
        self.dev.read(lba, &mut self.scratch[..padded])?;
        buf.copy_from_slice(&self.scratch[skew..skew + buf.len()]);
        Ok(())
    }
}

/// Per-set dispatcher over the volume kinds: 6/2 go to the assembler, 4
/// wraps the assembler in a `DeltaAdapter` over the active-volume base for
/// that one component. `base_factory` opens a fresh read handle on the
/// ACTIVE volume (the OS attaches a partition client; tests clone a fake).
pub struct SetSink<D, A, E, F>
where
    D: VolumeDev,
    A: VolumeDev,
    E: VolumeEvents,
    F: FnMut() -> Option<A>,
{
    assembler: Option<VolumeAssembler<D, A, E>>,
    delta: Option<DeltaAdapter<VolumeBase<A>, VolumeAssembler<D, A, E>>>,
    base_factory: F,
}

impl<D, A, E, F> SetSink<D, A, E, F>
where
    D: VolumeDev,
    A: VolumeDev,
    E: VolumeEvents,
    F: FnMut() -> Option<A>,
{
    pub fn new(assembler: VolumeAssembler<D, A, E>, base_factory: F) -> Self {
        Self { assembler: Some(assembler), delta: None, base_factory }
    }

    /// The assembler (None only while a kind-4 component is in flight).
    pub fn assembler(&self) -> Option<&VolumeAssembler<D, A, E>> {
        self.assembler.as_ref()
    }

    /// The assembler — also recovered from an adapter left behind by a
    /// rejected kind-4 component (the engine aborts mid-component).
    pub fn into_assembler(self) -> Option<VolumeAssembler<D, A, E>> {
        match (self.assembler, self.delta) {
            (Some(a), _) => Some(a),
            (None, Some(adapter)) => Some(adapter.into_inner()),
            (None, None) => None,
        }
    }
}

impl<D, A, E, F> ComponentSink for SetSink<D, A, E, F>
where
    D: VolumeDev,
    A: VolumeDev,
    E: VolumeEvents,
    F: FnMut() -> Option<A>,
{
    fn begin(&mut self, meta: &ComponentMeta) -> Result<(), RejectReason> {
        match meta.kind {
            KIND_SYSTEM_VOLUME | KIND_BUNDLE => {
                self.assembler.as_mut().ok_or(RejectReason::Order)?.begin(meta)
            }
            KIND_BUNDLE_DELTA => {
                let mut assembler = self.assembler.take().ok_or(RejectReason::Order)?;
                let mut base_sha = [0u8; 32];
                if meta.kind_data.len() != 32 {
                    self.assembler = Some(assembler);
                    return Err(RejectReason::Bounds);
                }
                base_sha.copy_from_slice(&meta.kind_data);
                // Base binding BEFORE any write: the window must exist on
                // the ACTIVE volume (its index is the locator; the adapter
                // re-checks the stream header against sha + length).
                let located = assembler.active_window(&base_sha);
                let (start, len) = match located {
                    Ok(w) => w,
                    Err(e) => {
                        self.assembler = Some(assembler);
                        return Err(e);
                    }
                };
                let Some(dev) = (self.base_factory)() else {
                    self.assembler = Some(assembler);
                    return Err(RejectReason::DeltaBase);
                };
                let base = VolumeBase::new(dev, start, len);
                let identity = BaseIdentity { image_sha256: base_sha, image_size: len };
                let mut adapter = DeltaAdapter::for_bundle(base, assembler, identity);
                if let Err(e) = adapter.begin(meta) {
                    self.assembler = Some(adapter.into_inner());
                    return Err(e);
                }
                self.delta = Some(adapter);
                Ok(())
            }
            _ => Err(RejectReason::KindUnsupported),
        }
    }

    fn chunk(&mut self, offset: u64, bytes: &[u8]) -> Result<(), RejectReason> {
        match self.delta.as_mut() {
            Some(adapter) => adapter.chunk(offset, bytes),
            None => self.assembler.as_mut().ok_or(RejectReason::Io)?.chunk(offset, bytes),
        }
    }

    fn finish(&mut self, meta: &ComponentMeta) -> Result<(), RejectReason> {
        match self.delta.take() {
            Some(mut adapter) => {
                let outcome = adapter.finish(meta);
                let mut assembler = adapter.into_inner();
                if outcome.is_ok() {
                    assembler.events_mut().bundle_reconstructed(&meta.name);
                }
                self.assembler = Some(assembler);
                outcome
            }
            None => self.assembler.as_mut().ok_or(RejectReason::Io)?.finish(meta),
        }
    }

    fn commit_set(&mut self) -> Result<(), RejectReason> {
        if self.delta.is_some() {
            return Err(RejectReason::Io);
        }
        self.assembler.as_mut().ok_or(RejectReason::Io)?.commit_set()
    }
}

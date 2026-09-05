// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the `boot-image-delta` reconstruction adapter (RFC-0090,
//! TASK-0034) — cfg-free and generic so the HOST suite proves the exact
//! logic the OS stage path executes. Sits between the engine and a plain
//! boot-image sink: the engine streams (signature-bound) DELTA bytes in;
//! the adapter drives the bounded `.nxdelta` decoder, binds the stream's
//! base to the ACTIVE slot's loader-verified NXBD (digest + size, O(1) —
//! reject `delta-base` before any write), reads COPY windows from the
//! base through one reused buffer, and feeds RECONSTRUCTED bytes to the
//! inner sink under a target-shaped meta (size/sha256 from the component
//! NXBD). The inner sink's readback gate and NXBD-last commit therefore
//! run UNCHANGED — the RFC-0089 §11 payoff.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: tests/updates_host/tests/component_set_delta.rs
//! ADR: docs/rfcs/RFC-0090-nxdelta-boot-image-delta-stream-format.md

#[cfg(not(feature = "std"))]
use alloc::vec::Vec;
#[cfg(feature = "std")]
use std::vec::Vec;

use nxdelta::decode::{Decoder, Event, PushError};

use crate::component_set::{
    ComponentMeta, ComponentSink, RejectReason, KIND_BOOT_IMAGE, KIND_BUNDLE,
};

/// Bounded read window for COPY records (one reused buffer, no per-record
/// allocation — os-service bump-heap discipline).
const COPY_WINDOW: usize = 16 * 1024;

/// Byte-addressed read access to the ACTIVE slot's image body.
pub trait BaseRead {
    /// Fills `buf` from base image byte offset `off` (bounds are the
    /// decoder's job; an implementation error maps to `Io`).
    fn read_at(&mut self, off: u64, buf: &mut [u8]) -> Result<(), RejectReason>;
}

/// The expected base identity — the ACTIVE slot's loader-verified NXBD
/// fields (RFC-0090 base binding).
#[derive(Clone, Copy, Debug)]
pub struct BaseIdentity {
    pub image_sha256: [u8; 32],
    pub image_size: u64,
}

/// Decodes an ACTIVE-slot NXBD sector into the base identity (an invalid
/// descriptor — direct-kernel boot, blank slot — is an honest
/// `delta-base`: nothing loader-verified exists to bind against).
pub fn base_identity_from_sector(sector: &[u8]) -> Result<BaseIdentity, RejectReason> {
    let (nxbd, _sig) = bootfmt::nxbd::decode(sector).map_err(|_| RejectReason::DeltaBase)?;
    Ok(BaseIdentity { image_sha256: nxbd.image_sha256, image_size: nxbd.image_size })
}

/// `ComponentSink` adapter: delta stream in, reconstructed image out.
pub struct DeltaAdapter<B: BaseRead, S: ComponentSink> {
    base: B,
    inner: S,
    base_id: BaseIdentity,
    /// TASK-0035 P3 (kind 4 `bundle-delta`): the TARGET identity comes
    /// from the stream header, not an NXBD — the inner sink's `begin` is
    /// deferred to that header (the assembler then binds the target to
    /// the signature-bound NEW index: an unknown target is
    /// `bundle-not-in-index`).
    target_from_header: bool,
    pending_meta: Option<ComponentMeta>,
    inner_meta: Option<ComponentMeta>,
    dec: Decoder,
    out_off: u64,
    window: Vec<u8>,
}

impl<B: BaseRead, S: ComponentSink> DeltaAdapter<B, S> {
    pub fn new(base: B, inner: S, base_id: BaseIdentity) -> Self {
        Self {
            base,
            inner,
            base_id,
            target_from_header: false,
            pending_meta: None,
            inner_meta: None,
            dec: Decoder::new(),
            out_off: 0,
            window: Vec::new(),
        }
    }

    /// A `bundle-delta` adapter (RFC-0089 §12.4 kind 4, TASK-0035 P3): the
    /// base identity is the ACTIVE volume window named by the component's
    /// `kind_data` (its sha256 + length); the target is what the stream
    /// header declares and what the inner sink binds to the NEW index.
    pub fn for_bundle(base: B, inner: S, base_id: BaseIdentity) -> Self {
        let mut adapter = Self::new(base, inner, base_id);
        adapter.target_from_header = true;
        adapter
    }

    /// The inner sink (for callers that need it back after finish).
    pub fn into_inner(self) -> S {
        self.inner
    }
}

impl<B: BaseRead, S: ComponentSink> ComponentSink for DeltaAdapter<B, S> {
    fn begin(&mut self, meta: &ComponentMeta) -> Result<(), RejectReason> {
        self.dec = Decoder::new();
        self.out_off = 0;
        self.window.clear();
        self.window.resize(COPY_WINDOW, 0);
        if self.target_from_header {
            // Kind 4: the inner `begin` waits for the stream header.
            self.pending_meta = Some(meta.clone());
            self.inner_meta = None;
            return Ok(());
        }
        // Target-shaped meta from the component's NXBD: the inner sink
        // sees exactly what a full-image stage would hand it.
        let (nxbd, _sig) =
            bootfmt::nxbd::decode(&meta.kind_data).map_err(|_| RejectReason::Bounds)?;
        let inner_meta = ComponentMeta {
            kind: KIND_BOOT_IMAGE,
            name: meta.name.clone(),
            size: nxbd.image_size,
            sha256: nxbd.image_sha256,
            kind_data: meta.kind_data.clone(),
        };
        self.inner.begin(&inner_meta)?;
        self.inner_meta = Some(inner_meta);
        Ok(())
    }

    fn chunk(&mut self, _offset: u64, bytes: &[u8]) -> Result<(), RejectReason> {
        // Target truth: the NXBD (kind 3) — or, for kind 4, whatever the
        // header declares, which the inner sink's `begin` then binds to
        // the signature-bound index.
        let known_target = self.inner_meta.as_ref().map(|m| (m.size, m.sha256));
        if known_target.is_none() && !self.target_from_header {
            return Err(RejectReason::Io);
        }
        let pending = &mut self.pending_meta;
        let inner_meta_slot = &mut self.inner_meta;
        let base_id = self.base_id;
        let base = &mut self.base;
        let inner = &mut self.inner;
        let out_off = &mut self.out_off;
        let window = &mut self.window;
        let result = self.dec.push(bytes, &mut |ev: Event<'_>| -> Result<(), RejectReason> {
            match ev {
                Event::Header(h) => {
                    // Base binding FIRST (O(1) against the loader-verified
                    // active NXBD / the active index window), then
                    // stream/target agreement.
                    if h.base_sha256 != base_id.image_sha256 || h.base_size != base_id.image_size {
                        return Err(RejectReason::DeltaBase);
                    }
                    match known_target {
                        Some((size, sha)) => {
                            if h.target_size != size || h.target_sha256 != sha {
                                return Err(RejectReason::DeltaFormat);
                            }
                        }
                        None => {
                            let meta = pending.take().ok_or(RejectReason::Io)?;
                            let inner_meta = ComponentMeta {
                                kind: KIND_BUNDLE,
                                name: meta.name,
                                size: h.target_size,
                                sha256: h.target_sha256,
                                kind_data: Vec::new(),
                            };
                            inner.begin(&inner_meta)?;
                            *inner_meta_slot = Some(inner_meta);
                        }
                    }
                    Ok(())
                }
                Event::Copy { base_off, len } => {
                    let mut done = 0u64;
                    while done < u64::from(len) {
                        let take = (u64::from(len) - done).min(window.len() as u64) as usize;
                        base.read_at(base_off + done, &mut window[..take])?;
                        inner.chunk(*out_off, &window[..take])?;
                        *out_off += take as u64;
                        done += take as u64;
                    }
                    Ok(())
                }
                Event::Add(data) => {
                    inner.chunk(*out_off, data)?;
                    *out_off += data.len() as u64;
                    Ok(())
                }
            }
        });
        match result {
            Ok(()) => Ok(()),
            Err(PushError::Format) => Err(RejectReason::DeltaFormat),
            Err(PushError::Sink(reason)) => Err(reason),
        }
    }

    fn finish(&mut self, _meta: &ComponentMeta) -> Result<(), RejectReason> {
        // Stream closure (END seen, totals exact) — a truncated delta must
        // never reach the inner readback gate looking complete.
        self.dec.finish().map_err(|_| RejectReason::DeltaFormat)?;
        let inner_meta = self.inner_meta.take().ok_or(RejectReason::Io)?;
        self.inner.finish(&inner_meta)
    }
}

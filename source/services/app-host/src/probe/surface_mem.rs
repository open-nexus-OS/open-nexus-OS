// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the surface's buffer as app-host draws it (RFC-0114 §1, §3–§5): the object gpud made
//! for this app, its capability moved here with the create's answer (`OP_SURFACE_BUFFERS`),
//! mapped read-write. A frame is drawn straight into the slot windowd released with the last
//! present's ack — first the last present's rects come forward from the slot shown (the slot
//! being drawn holds the frame before last), then the frame's own rows — its writes are cleaned
//! toward memory for a GPU that does not snoop the caches, and the slot is presented by number.
//! No pixel goes through a syscall; the arithmetic is `crate::slot_math` (host-tested).
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `crate::slot_math` host tests; the lanes' `apphost: surface mapped (`
//! ADR: docs/adr/0092-a-surface-is-a-buffer-the-app-draws-and-the-gpu-reads-gpud-allocates-it-for-its-owner-windowd-composes-by-reference.md

use crate::band_math::PresentDamage;
use crate::slot_math::{clean_spans, copy_forward, Rect, SlotLayout, MAX_SPANS};
use nexus_display_proto::client_surface::Buffers;

/// The surface's buffer, mapped, and where the next frame goes.
pub(crate) struct SurfaceMem {
    map: nexus_abi::SharedVmo,
    layout: SlotLayout,
    /// The GPU does not snoop the caches: what a frame wrote is cleaned before its present.
    clean: bool,
    /// The slot the next frame is drawn into (windowd released it with the last ack).
    draw: u8,
    /// The slot of the last present (`None`: nothing presented yet).
    shown: Option<u8>,
    /// What the last present changed — copied into `draw` before the next frame is drawn.
    forward: PresentDamage,
    /// What the frame being drawn copied forward (cleaned with the frame's own rows).
    copied: PresentDamage,
    /// The slot being drawn holds the newest frame (forwarded, or the frame draws everything).
    forwarded: bool,
}

impl SurfaceMem {
    /// The buffer `b` describes, its capability `cap` (taken: closed with the mapping, or here
    /// when the map is refused).
    pub(crate) fn map(b: &Buffers, cap: u32) -> Result<Self, &'static str> {
        let map = nexus_abi::SharedVmo::map(cap, b.object_bytes())
            .map_err(|_| "apphost: FAIL surface map")?;
        Ok(Self {
            map,
            layout: SlotLayout {
                stride: b.stride as usize,
                rows: u32::from(b.rows),
                width: u32::from(b.width),
                slots: b.slots,
            },
            clean: b.cache_block != 0,
            draw: 0,
            shown: None,
            forward: PresentDamage::default(),
            copied: PresentDamage::default(),
            forwarded: false,
        })
    }

    /// Bytes mapped (the line that says the surface is mapped).
    pub(crate) fn bytes(&self) -> usize {
        self.map.len()
    }

    /// A frame begins. Unless it draws everything again (`full`: the base repainted, every band
    /// invalidated), the last present's rects come forward from the slot shown — once per slot:
    /// the slot being drawn then holds the newest frame everywhere before this one draws over it.
    pub(crate) fn begin_frame(&mut self, full: bool) {
        if self.forwarded || full {
            return;
        }
        if let Some(shown) = self.shown.filter(|s| *s != self.draw) {
            let rects = self.forward;
            copy_forward(self.map.bytes_mut(), &self.layout, (shown, self.draw), rects.rects());
            self.copied = rects;
        }
        self.forwarded = true;
    }

    /// The buffer this one replaces (a re-create at the same size: a store that outgrew its rows)
    /// holds its newest frame in its shown slot: its rows — base and store — come over into the
    /// slot this one draws first, so nothing is painted again. The rows copied (`None`: the old
    /// buffer showed nothing, or its rows are not this one's width).
    pub(crate) fn carry_from(&mut self, old: &SurfaceMem) -> Option<u32> {
        let shown = old.shown?;
        if old.layout.width != self.layout.width {
            return None;
        }
        let rows = old.layout.rows.min(self.layout.rows);
        let len = self.layout.width as usize * 4;
        for y in 0..rows {
            let (from, to) = (old.layout.row_at(shown, y)?, self.layout.row_at(self.draw, y)?);
            let src = old.map.bytes().get(from..from + len)?;
            self.map.bytes_mut().get_mut(to..to + len)?.copy_from_slice(src);
        }
        Some(rows)
    }

    /// Row `y` of the slot being drawn: its `width × 4` bytes (`None` past the slot).
    pub(crate) fn row_mut(&mut self, y: u32) -> Option<&mut [u8]> {
        let at = self.layout.row_at(self.draw, y)?;
        let end = at + self.layout.width as usize * 4;
        self.map.bytes_mut().get_mut(at..end)
    }

    /// `bytes` at column `x` of row `y` of the slot being drawn; `false` past the slot.
    pub(crate) fn write(&mut self, y: u32, x: u32, bytes: &[u8]) -> bool {
        let at = x as usize * 4;
        match self.row_mut(y).and_then(|row| row.get_mut(at..at + bytes.len())) {
            Some(dst) => {
                dst.copy_from_slice(bytes);
                true
            }
            None => false,
        }
    }

    /// The frame is drawn (`damage`: what it changed): its writes — its own rows and what it
    /// copied forward — are cleaned for a GPU that does not snoop the caches, it becomes the
    /// slot shown, and the next frame goes to the other. Returns the slot to present.
    pub(crate) fn end_frame(&mut self, damage: &PresentDamage) -> u8 {
        if self.clean {
            let mut rects: [Rect; MAX_SPANS] = [(0, 0, 0, 0); MAX_SPANS];
            let mut n = 0;
            for r in self.copied.rects().iter().chain(damage.rects()).take(MAX_SPANS) {
                rects[n] = *r;
                n += 1;
            }
            let mut spans = [(0, 0); MAX_SPANS];
            let k = clean_spans(&self.layout, self.draw, &rects[..n], &mut spans);
            for &(at, len) in &spans[..k] {
                let _ = self.map.clean(at, len);
            }
        }
        let slot = self.draw;
        self.shown = Some(slot);
        self.forward = *damage;
        self.copied = PresentDamage::default();
        self.forwarded = false;
        self.draw = self.layout.next(slot);
        slot
    }
}

// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: windowd compositor runtime — a client surface's BUFFER (RFC-0114, ADR-0092). gpud
//! makes it for the app (`OP_SURFACE_ALLOC`: the app's kernel id is the owner, charged for it),
//! windowd moves its capability to the app with the create's answer (`OP_SURFACE_BUFFERS`), the
//! frames name the surface's buffer and newest slot, and a present's ack — the release — leaves
//! once no frame still in flight reads any other slot (`release_presents`). A surface its client
//! dropped shows its last frame until its successor presents (a resize never flashes an empty
//! window); then its buffer goes back (`OP_SURFACE_FREE`: gpud unbinds it once the last frame
//! that read it retired). windowd never waits on gpud (ADR-0075): a request its queue cannot
//! take now leaves on a later pass, in order.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the table and the release rule host-tested (`crate::client_surface`); the
//!   lanes' `gpud: surface alloc ok (`, `windowd: compose copies=0 (` and (many-windows)
//!   `windowd: windows drawn (n=12)`
//! ADR: docs/adr/0092-a-surface-is-a-buffer-the-app-draws-and-the-gpu-reads-gpud-allocates-it-for-its-owner-windowd-composes-by-reference.md

use super::client_send::ClientSend;
use super::*;
use crate::client_surface::{Delivery, SurfaceBuffer};
use alloc::collections::VecDeque;
use nexus_display_proto::client_surface::{self as wire, Buffers};
use nexus_display_proto::surface_alloc::{self as alloc_wire, AllocRequest, Allocated};

/// Slots of a surface the CPU draws: the app draws one while the frames read the other.
const SLOTS: u8 = 2;

/// The first frame composed by reference was named (once per windowd).
static BY_REFERENCE_SAID: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

/// A request for gpud that waits for room in its queue.
#[derive(Clone, Copy)]
enum Request {
    Alloc(AllocRequest),
    Free(u32),
}

/// What the buffers need between frames: the surfaces the frame being built reads, the acks a
/// release takes, the requests gpud's queue had no room for, the desktop's previous surface.
pub(crate) struct BufferState {
    /// The surfaces the frame being built reads (recorded with its seq once it left).
    frame_reads: Vec<u32>,
    /// App windows the frame being built draws from their own buffers, and the most a frame
    /// that left drew on this boot (RFC-0114 §7: the tables grow with the windows).
    pub(super) frame_windows: usize,
    windows_drawn: usize,
    /// App windows this frame left out, covered by the windows above them (TASK-0351 P1).
    pub(super) frame_occluded: usize,
    windows_occluded: usize,
    releases: Vec<(u32, u32, u64)>,
    requests: VecDeque<Request>,
    /// The desktop surface its client dropped and its layers, shown until its successor
    /// presents — and its owner's kernel id: the greeter a shell replaces still owns its last
    /// surface (its late presents there are stale, never a stranger's).
    pub(crate) desktop_retiring: Option<u32>,
    pub(crate) desktop_retiring_layers: super::surface_layers::SurfaceLayers,
    pub(crate) desktop_retiring_owner: u64,
}

impl BufferState {
    pub(crate) fn new() -> Self {
        Self {
            frame_reads: Vec::new(),
            frame_windows: 0,
            windows_drawn: 0,
            frame_occluded: 0,
            windows_occluded: 0,
            releases: Vec::new(),
            requests: VecDeque::new(),
            desktop_retiring: None,
            desktop_retiring_layers: super::surface_layers::SurfaceLayers::EMPTY,
            desktop_retiring_owner: 0,
        }
    }
}

/// Which window a surface belongs to.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Owner {
    Desktop,
    App(usize),
}

impl DisplayServerRuntime {
    /// Asks gpud for surface `id`'s buffer of `width` × `rows` (the answer lands in
    /// [`Self::on_surface_allocated`], the cookie is the surface's id).
    pub(super) fn request_surface_buffer(&mut self, id: u32, owner_sid: u64, size: (u16, u16)) {
        let (width, rows) = size;
        let req = AllocRequest {
            cookie: id,
            owner_sid,
            width,
            rows,
            slots: SLOTS,
            usage: alloc_wire::USAGE_SAMPLE,
        };
        self.buffers.requests.push_back(Request::Alloc(req));
        self.flush_gpud_requests();
    }

    /// Source `buffer` goes back to gpud.
    pub(super) fn free_surface_buffer(&mut self, buffer: u32) {
        self.buffers.requests.push_back(Request::Free(buffer));
        self.flush_gpud_requests();
    }

    /// Every loop pass: the requests gpud's queue had no room for, in order. The send drains no
    /// reply first: an answer handled inside it could queue (and flush) a request of its own
    /// while this one is still at the front — sent twice.
    pub(crate) fn flush_gpud_requests(&mut self) {
        if !self.ensure_gpud_client() {
            return;
        }
        while let Some(req) = self.buffers.requests.front().copied() {
            let mut frame = [0u8; alloc_wire::ALLOC_REQUEST_LEN];
            let len = match req {
                Request::Alloc(r) => {
                    frame = alloc_wire::encode_alloc_request(&r);
                    frame.len()
                }
                Request::Free(b) => {
                    frame[..alloc_wire::FREE_LEN].copy_from_slice(&alloc_wire::encode_free(b));
                    alloc_wire::FREE_LEN
                }
            };
            let sent = self
                .gpud_client
                .as_ref()
                .is_some_and(|c| c.send(&frame[..len], Wait::NonBlocking).is_ok());
            if !sent {
                return; // gpud's queue is full: the next pass (its next reply wakes us) goes on
            }
            self.buffers.requests.pop_front();
        }
    }

    /// gpud's answer to [`Self::request_surface_buffer`], wherever on the route it landed.
    pub(super) fn on_surface_allocated(&mut self, a: Allocated, cap: Option<u32>) {
        let id = a.cookie;
        let waiting =
            self.client_surfaces.get_by_id(id).is_some_and(|s| !s.retiring && s.buffer.is_none());
        let made = a.status == nexus_display_proto::STATUS_OK && a.buffer != 0;
        let (true, true, Some(cap)) = (waiting, made, cap) else {
            // The create is gone (its window closed, its client left) or gpud refused it.
            if let Some(cap) = cap {
                crate::client_outbox::close_cap(cap);
            }
            if made {
                self.free_surface_buffer(a.buffer);
            }
            if waiting {
                let _ = debug_println(&alloc::format!(
                    "WINDOWD: FAIL surface buffer (id={id} status={})",
                    a.status
                ));
                self.refuse_surface(id, wire::SURFACE_STATUS_QUOTA);
            }
            return;
        };
        let buffer = SurfaceBuffer {
            source: a.buffer,
            stride: a.stride,
            width: a.width,
            rows: a.rows,
            slots: a.slots,
            cache_block: a.cache_block,
        };
        if let Err(status) = self.client_surfaces.bind_buffer(id, buffer, Some(cap)) {
            crate::client_outbox::close_cap(cap);
            self.free_surface_buffer(a.buffer);
            self.refuse_surface(id, status);
            return;
        }
        self.deliver_buffers(id);
    }

    /// The channel of surface `id`'s client.
    fn surface_channel(&self, id: u32) -> Option<u32> {
        match self.owner_of(id)? {
            Owner::Desktop => self.desktop_channel,
            Owner::App(idx) => self.apps[idx].event_channel,
        }
    }

    /// The create's answer to its client: the buffer's geometry, its capability moved along.
    /// Without a channel yet (a desktop create that raced its attach) the capability stays
    /// windowd's until the channel is bound ([`Self::deliver_owed_buffers`]).
    pub(super) fn deliver_buffers(&mut self, id: u32) {
        let Some(s) = self.client_surfaces.get_by_id(id).copied() else { return };
        let (Some(b), Some(_)) = (s.buffer, s.buffer_cap) else { return };
        let Some(channel) = self.surface_channel(id) else { return };
        let Some(cap) = self.client_surfaces.take_buffer_cap(id) else { return };
        let frame = wire::encode_surface_buffers(&Buffers {
            status: wire::SURFACE_STATUS_OK,
            surface_id: id,
            stride: b.stride,
            width: b.width,
            rows: b.rows,
            slots: b.slots,
            cache_block: b.cache_block,
        });
        match self.client_send_cap(channel, &frame, cap) {
            ClientSend::Sent | ClientSend::Queued => {}
            ClientSend::Dead => {
                if let Some(Owner::App(idx)) = self.owner_of(id) {
                    self.note_app_channel_dead(idx);
                }
            }
            ClientSend::Refused => {
                let _ = debug_println("WINDOWD: FAIL surface buffers send (client far behind)");
            }
        }
    }

    /// A channel was bound: every answer that waited for one leaves.
    pub(super) fn deliver_owed_buffers(&mut self) {
        let owed: Vec<u32> = self.client_surfaces.owed_caps().collect();
        for id in owed {
            self.deliver_buffers(id);
        }
    }

    /// The create of surface `id` failed after it was taken (no buffer for it): the surface goes
    /// and its client is told (`OP_SURFACE_BUFFERS` with the status, no capability). A window
    /// with nothing else to show hides.
    pub(super) fn refuse_surface(&mut self, id: u32, status: u8) {
        let owner = self.owner_of(id);
        let channel = self.surface_channel(id);
        self.drop_surface(id);
        match owner {
            Some(Owner::Desktop) => {
                self.desktop_surface_id = None;
                if self.buffers.desktop_retiring.is_none() {
                    self.hide_window(crate::window_scene::WindowId::Desktop);
                }
            }
            Some(Owner::App(idx)) => {
                self.apps[idx].surface_id = None;
                if self.apps[idx].retiring.is_none() && !self.apps[idx].starting {
                    self.apps[idx].win.visible = false;
                    self.hide_window(crate::window_scene::WindowId::App(idx as u16));
                }
            }
            None => {}
        }
        if let Some(channel) = channel {
            let frame = wire::encode_surface_buffers(&Buffers::refused(status));
            let _ = self.client_send(channel, &frame, Delivery::Ack);
        }
        self.queue_full_frame_damage();
    }

    /// The window surface `id` belongs to: its current surface or the one it replaces.
    pub(super) fn owner_of(&self, id: u32) -> Option<Owner> {
        if self.desktop_surface_id == Some(id) || self.buffers.desktop_retiring == Some(id) {
            return Some(Owner::Desktop);
        }
        let idx = self.apps.iter().position(|a| a.surface_id == Some(id) || a.retiring == Some(id));
        idx.map(Owner::App)
    }

    /// The surface `owner` shows: its current one once it presented, else the one it replaces.
    pub(super) fn shown_surface(&self, owner: Owner) -> Option<u32> {
        let (current, retiring) = match owner {
            Owner::Desktop => (self.desktop_surface_id, self.buffers.desktop_retiring),
            Owner::App(idx) => (self.apps[idx].surface_id, self.apps[idx].retiring),
        };
        let presented =
            |id: &u32| self.client_surfaces.get_by_id(*id).is_some_and(|s| s.shown().is_some());
        current.filter(presented).or(retiring.filter(presented))
    }

    /// `owner`'s surface `id` was dropped by its client (`SURFACE_DESTROY`, or a create that
    /// replaces it): it shows its last frame — with the layers it declared — until its successor
    /// presents, so a resize never shows a window half drawn. A surface it replaced before goes
    /// now.
    pub(super) fn retire_surface(&mut self, owner: Owner, id: u32) {
        let _ = self.client_surfaces.retire(id);
        // A surface that never presented has nothing to show: the one it replaced keeps showing.
        let presented = self.client_surfaces.get_by_id(id).is_some_and(|s| s.shown().is_some());
        let showing = match owner {
            Owner::Desktop => self.buffers.desktop_retiring,
            Owner::App(idx) => self.apps[idx].retiring,
        };
        if !presented && showing.is_some() {
            self.drop_surface(id);
            return;
        }
        let previous = match owner {
            Owner::Desktop => {
                self.buffers.desktop_retiring_layers = self.desktop_layers;
                self.buffers.desktop_retiring.replace(id)
            }
            Owner::App(idx) => {
                self.apps[idx].retiring_layers = self.apps[idx].layers;
                self.apps[idx].retiring.replace(id)
            }
        };
        if let Some(prev) = previous.filter(|p| *p != id) {
            self.drop_surface(prev);
        }
    }

    /// `owner`'s current surface presented: the one it replaced goes.
    pub(super) fn end_retiring(&mut self, owner: Owner) {
        let previous = match owner {
            Owner::Desktop => self.buffers.desktop_retiring.take(),
            Owner::App(idx) => self.apps[idx].retiring.take(),
        };
        if let Some(prev) = previous {
            self.drop_surface(prev);
        }
    }

    /// Surface `id` is gone: its record; its buffer back to gpud (unbound once the last frame
    /// that read it retired); windowd's capability to it when its client never got it.
    pub(super) fn drop_surface(&mut self, id: u32) {
        let Ok(s) = self.client_surfaces.destroy(id) else { return };
        if let Some(cap) = s.buffer_cap {
            crate::client_outbox::close_cap(cap);
        }
        if let Some(b) = s.buffer {
            self.free_surface_buffer(b.source);
        }
    }

    /// The frame being built reads surface `id`'s newest slot.
    pub(super) fn note_frame_reads(&mut self, id: u32) {
        self.buffers.frame_reads.push(id);
    }

    /// The frame being built starts: it reads nothing yet.
    pub(super) fn begin_frame_reads(&mut self) {
        self.buffers.frame_reads.clear();
    }

    /// The frame left for gpud as present `seq`: what it reads is read until `seq` retired.
    pub(super) fn frame_reads_sent(&mut self, seq: u32) {
        let reads = core::mem::take(&mut self.buffers.frame_reads);
        if let Some(&first) = reads.first() {
            self.client_surfaces.note_read(&reads, seq);
            if !BY_REFERENCE_SAID.swap(true, core::sync::atomic::Ordering::Relaxed) {
                let source = self
                    .client_surfaces
                    .get_by_id(first)
                    .and_then(|s| s.shown())
                    .map_or(0, |s| s.buffer.source);
                // Honest by construction: no app row is copied anywhere in windowd any more —
                // this frame read the app's own memory (RFC-0114 §6).
                let _ = debug_println(&alloc::format!(
                    "windowd: compose copies=0 (by reference: surfaces={} source={source} seq={seq})",
                    reads.len()
                ));
            }
        }
        self.buffers.frame_reads = reads;
        self.buffers.frame_reads.clear();
        // A new most of app windows one frame drew from their own pixels (one line per new
        // high, a handful per boot): the proof that no window table stops at a fixed count.
        let drawn = core::mem::take(&mut self.buffers.frame_windows);
        if drawn > self.buffers.windows_drawn {
            self.buffers.windows_drawn = drawn;
            let _ = debug_println(&alloc::format!("windowd: windows drawn (n={drawn})"));
        }
        // A new most of windows one frame left out as covered (TASK-0351 P1): the proof that an
        // occluded window costs the frame nothing.
        let occluded = core::mem::take(&mut self.buffers.frame_occluded);
        if occluded > self.buffers.windows_occluded {
            self.buffers.windows_occluded = occluded;
            let _ = debug_println(&alloc::format!("windowd: windows occluded (n={occluded})"));
        }
    }

    /// The release (RFC-0114 §5): every present whose other slots no frame in flight reads any
    /// more is acked to its client now — the client draws its next frame into one of them.
    pub(super) fn release_presents(&mut self) {
        let retired = self.presents.retired();
        let mut due = core::mem::take(&mut self.buffers.releases);
        due.clear();
        self.client_surfaces.take_releases(retired, &mut due);
        let now = nexus_abi::nsec().unwrap_or(0);
        for &(id, seq, since) in &due {
            crate::compositor::frame_causes::work_release(now.saturating_sub(since) / 1000);
            let ok = wire::SURFACE_STATUS_OK;
            let _ = self.send_surface_frame(
                id,
                &wire::encode_surface_ack(wire::OP_SURFACE_PRESENT, ok, seq),
            );
        }
        due.clear();
        self.buffers.releases = due;
    }
}

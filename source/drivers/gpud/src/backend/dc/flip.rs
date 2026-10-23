// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The board display's flips through the controller's latch (RFC-0101 §8, TASK-0342): a drawn
//! present's plane is written with the configuration latch at once, at any line — the
//! controller takes it at its next frame boundary — and the flip's interrupt is armed while a
//! flip is latched: the latch taken (`cfg-ready-clear`), the vsync behind it. A flip has landed
//! when the latch reads clear: at the interrupt's wake, or at the request loop's next pass (a
//! wake that came while gpud drew the next frame). The present that waited behind it is latched
//! the moment it lands — unless the cadence holds it (RFC-0110, `frame_pace.rs`): a frame drawn
//! before its slot is held, the interrupt armed, until the boundary before the slot. A landing is
//! timed at the boundary its flip was taken at, from the scan line counter, and acked with that
//! time. Board cycles 59–60 waited for a wake inside the 45-line blank instead — behind a GPU wait
//! of ~35 ms — and served an interrupt storm: the enable word was read as a mask and the clears
//! went to the read-only raw word.

use nexus_abi::nsec;
use nexus_gfx::backend::dc::regs;
use nexus_gfx::backend::error::GfxError;
use nexus_ux_budgets::Hop;

use super::{emit, ring, DcDisplay};
use crate::backend::display::Landed;

/// Notifications drained per wake (xhcid's pattern): a coalesced burst, never a spin.
const DRAIN_MAX: u32 = 8;
/// Served wakes the witness line waits for.
const WITNESS_WAKES: u32 = 32;
/// Landed flips per `gpud: dc flips (…)` line.
const FLIPS_LINE: u32 = 64;
/// Scan-out underflows named one by one (`gpud: dc underflow (…)`), then counted only.
const UNDERFLOWS_NAMED: u32 = 16;

/// The flip's interrupt, bound: its line, the endpoint it notifies, and what its wakes found.
pub(super) struct FlipIrq {
    irq: u32,
    slot: u32,
    /// The witness (`gpud: dc vblank ok (min_period_us= line= n= taken= vsyncs=)` after the
    /// first `WITNESS_WAKES`): the shortest interval between two wakes — a frame or more when
    /// one source per flip raises the line, a fraction of a millisecond in a storm — and the
    /// scan line the first wake saw.
    wakes: u32,
    last_ns: u64,
    min_period_us: u64,
    first_line: u32,
    witnessed: bool,
    first_flip_said: bool,
    /// `gpud: dc flips (…)` every `FLIPS_LINE` landings: flips landed at a wake and at a pass of
    /// the loop, presents that waited behind a latched flip, wakes that found the latch still
    /// held, and what raised the wakes — the latch taken, the vsync, nothing, an underflow (the
    /// scan starved), every source seen in the pipeline's and the output control's raw words.
    landed: u32,
    at_wake: u32,
    at_pass: u32,
    behind: u32,
    held: u32,
    taken: u32,
    vsyncs: u32,
    unraised: u32,
    underflows: u32,
    seen_onl2: u32,
    seen_out: u32,
    /// Frames held for their slot by the cadence (RFC-0110).
    paced: u32,
    /// The frame chain's hops in gpud (RFC-0108 §hops): a present received → drawn, drawn → on
    /// screen — `gpud: hops frame (…)` every `FRAME_WINDOW` landed presents.
    hop_work: Hop,
    hop_flip: Hop,
}

impl FlipIrq {
    pub(super) fn new(irq: u32, slot: u32) -> Self {
        Self {
            irq,
            slot,
            wakes: 0,
            last_ns: 0,
            min_period_us: u64::MAX,
            first_line: 0,
            witnessed: false,
            first_flip_said: false,
            landed: 0,
            at_wake: 0,
            at_pass: 0,
            behind: 0,
            held: 0,
            taken: 0,
            vsyncs: 0,
            unraised: 0,
            underflows: 0,
            seen_onl2: 0,
            seen_out: 0,
            paced: 0,
            hop_work: Hop::new(),
            hop_flip: Hop::new(),
        }
    }

    /// One served wake: what raised it, the line it saw, when.
    fn note_wake(&mut self, (onl2, out): (u32, u32), line: u32, now: u64) {
        if self.wakes == 0 {
            self.first_line = line;
        } else {
            self.min_period_us = self.min_period_us.min(now.saturating_sub(self.last_ns) / 1000);
        }
        self.wakes = self.wakes.saturating_add(1);
        self.last_ns = now;
        self.taken += u32::from(onl2 & regs::INT_CFG_TAKEN != 0);
        self.vsyncs += u32::from(onl2 & regs::INT_VSYNC != 0);
        self.unraised += u32::from(onl2 == 0 && out == 0);
        if onl2 & regs::INT_UNDERFLOW != 0 {
            self.underflows += 1;
            // The flicker's suspect (cycle 61: 8 a session, not repeatable): each named with its
            // time, the line the wake saw and whether a flip was taken at the same boundary.
            if self.underflows <= UNDERFLOWS_NAMED {
                emit(&alloc::format!(
                    "gpud: dc underflow (n={} at_ms={} line={line} taken={} flips={})",
                    self.underflows,
                    now / 1_000_000,
                    u8::from(onl2 & regs::INT_CFG_TAKEN != 0),
                    self.landed
                ));
            }
        }
        self.seen_onl2 |= onl2;
        self.seen_out |= out;
        if !self.witnessed && self.wakes >= WITNESS_WAKES {
            self.witnessed = true;
            emit(&alloc::format!(
                "gpud: dc vblank ok (min_period_us={} line={} n={} taken={} vsyncs={})",
                self.min_period_us,
                self.first_line,
                self.wakes,
                self.taken,
                self.vsyncs
            ));
        }
    }

    /// One landed flip: where it was found, its present's hops, the lines when due (the cadence
    /// and the landings past their slot from the pace).
    fn note_landed(
        &mut self,
        at_wake: bool,
        stamps: Option<ring::Stamps>,
        now: u64,
        (cadence, late): (u8, u32),
    ) {
        self.landed = self.landed.saturating_add(1);
        self.at_wake += u32::from(at_wake);
        self.at_pass += u32::from(!at_wake);
        if self.landed % FLIPS_LINE == 0 {
            emit(&alloc::format!(
                "gpud: dc flips (n={} at_wake={} at_pass={} behind={} wakes={} held={} taken={} \
                 vsyncs={} unraised={} underflows={} onl2=0x{:x} out=0x{:x} cadence={cadence} \
                 paced={} late={late})",
                self.landed,
                self.at_wake,
                self.at_pass,
                self.behind,
                self.wakes,
                self.held,
                self.taken,
                self.vsyncs,
                self.unraised,
                self.underflows,
                self.seen_onl2,
                self.seen_out,
                self.paced
            ));
        }
        let Some(s) = stamps.filter(|s| s.received_ns != 0) else { return };
        self.hop_work.note(s.drawn_ns.saturating_sub(s.received_ns) / 1000);
        self.hop_flip.note(now.saturating_sub(s.drawn_ns) / 1000);
        if self.hop_work.count() >= nexus_ux_budgets::FRAME_WINDOW {
            let (wn, work_avg, work_max) = self.hop_work.take();
            let (fn_, flip_avg, flip_max) = self.hop_flip.take();
            emit(&alloc::format!(
                "gpud: hops frame (recv>drawn={work_avg}/{work_max}/{wn} \
                 drawn>shown={flip_avg}/{flip_max}/{fn_})"
            ));
        }
    }
}

impl DcDisplay {
    /// The running present is drawn: its flip latched now when none is (or held for its slot),
    /// else it waits behind the latched one (latched when that one lands).
    pub(super) fn queue_flip(&mut self) -> Result<(), GfxError> {
        match self.ring.drawn().map_err(|_| GfxError::ResourceExhausted)? {
            Some(slot) => self.latch_or_hold(slot),
            None => {
                if let Some(f) = self.flip_irq.as_mut() {
                    f.behind = f.behind.saturating_add(1);
                }
            }
        }
        Ok(())
    }

    /// The boundary the frame being scanned began at (the line counter's lines before now).
    fn boundary_now(&self) -> u64 {
        let line = self.live.as_ref().map_or(0, |l| l.controller.line());
        self.pace.timing().boundary(nsec().unwrap_or(0), line)
    }

    /// `slot`'s flip latched now — or, at a cadence whose slot it would come before, held: the
    /// flip's interrupt armed, the next wake at a boundary looks again (`flip_wake`).
    fn latch_or_hold(&mut self, slot: usize) {
        if self.pace.may_latch(self.boundary_now()) {
            self.latch(slot);
            return;
        }
        self.held = Some(slot);
        if let Some(live) = self.live.as_ref() {
            live.controller.flip_irq_arm();
        }
        if let Some(f) = self.flip_irq.as_mut() {
            f.paced = f.paced.saturating_add(1);
        }
    }

    /// `slot`'s plane written with the latch at whatever line the scan is on, and the flip's
    /// interrupt armed until it lands.
    fn latch(&mut self, slot: usize) {
        let (w, h) = self.mode;
        let Some(live) = self.live.as_ref() else { return };
        let Some(fb) = live.framebuffer.as_ref() else { return };
        live.controller.flip_latched(&fb.plane_at(ring::ROWS[slot], w, h));
        live.controller.flip_irq_arm();
        self.latched_ns = nsec().unwrap_or(0);
        if let Some(f) = self.flip_irq.as_mut().filter(|f| !f.first_flip_said) {
            f.first_flip_said = true;
            emit(&alloc::format!(
                "gpud: dc flip ok (slot={slot} line={} latched)",
                live.controller.line()
            ));
        }
    }

    /// The loop's pass over the display (the flip's interrupt woke it, or a request was served):
    /// the notifications drained and what raised them cleared; a held frame latched once its
    /// slot is next; a latched flip whose latch reads clear has landed — its present's ack
    /// returned with the boundary it was taken at, the one waiting behind it latched (or held).
    /// `None`: nothing landed.
    pub(super) fn flip_wake(&mut self) -> Option<Landed> {
        let woke = self.serve_flip_irq()?;
        if let Some(slot) = self.held {
            if self.pace.may_latch(self.boundary_now()) {
                self.held = None;
                self.latch(slot);
            }
            return None;
        }
        self.ring.latched()?;
        if self.live.as_ref()?.controller.latch_pending() {
            if let Some(f) = self.flip_irq.as_mut().filter(|_| woke) {
                f.held = f.held.saturating_add(1);
            }
            return None;
        }
        let landed = self.ring.landed()?;
        // A ring flip taken: the scan has left the splash (the first one after the reveal).
        if let Some(live) = self.live.as_mut() {
            live.release_splash();
        }
        let at_ns = self.pace.taken_at(self.latched_ns, self.boundary_now());
        self.pace.note_shown(at_ns);
        match landed.next {
            Some(next) => self.latch_or_hold(next),
            None => {
                if let Some(live) = self.live.as_ref() {
                    live.controller.flip_irq_disarm();
                }
            }
        }
        let now = nsec().unwrap_or(0);
        let stamps = landed.drawn.ack.map(|_| landed.drawn.stamps);
        let pace = (self.pace.cadence(), self.pace.late);
        self.flip_irq.as_mut()?.note_landed(woke, stamps, now, pace);
        let (seq, status) = landed.drawn.ack?;
        let shown = nexus_display_proto::frame_time::Shown {
            at_ns,
            period_ns: u32::try_from(self.pace.timing().period_ns).unwrap_or(u32::MAX),
            cadence: self.pace.cadence(),
        };
        Some(Landed { seq, status, shown })
    }

    /// Drains the flip interrupt's notifications: `Some(true)` when it woke the loop (what
    /// raised it read, counted and cleared, the interrupt completed), `Some(false)` when not,
    /// `None` without a bound interrupt.
    fn serve_flip_irq(&mut self) -> Option<bool> {
        let f = self.flip_irq.as_mut()?;
        let mut drained = 0u32;
        let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut note = [0u8; 16];
        while drained < DRAIN_MAX
            && nexus_abi::ipc_recv_v1_nb(f.slot, &mut hdr, &mut note, true).is_ok()
        {
            drained += 1;
        }
        if drained == 0 {
            return Some(false);
        }
        if let Some(live) = self.live.as_ref() {
            // What raised the line, cleared at once — exactly those sources: a status left set
            // keeps the level line up (cycles 59–60's storm).
            let raised = live.controller.irq_raised();
            live.controller.irq_clear(raised);
            f.note_wake(raised, live.controller.line(), nsec().unwrap_or(0));
        }
        let _ = nexus_abi::irq_complete(f.irq);
        Some(true)
    }
}

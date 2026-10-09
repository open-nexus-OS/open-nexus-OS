// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: windowd's screen-capture machine (RFC-0095, ADR-0071, TASK-0068) — the decisions,
//! pure and host-tested; the runtime (`compositor/runtime/capture.rs`) drives the IPC.
//!
//! screencapd lends its frame VMO once (`ATTACH`). A `FREEZE` first takes the pointer out of the
//! frame when the display draws it in (the GL build-up and the software blend do; a hardware
//! overlay does not): the machine waits until a present composed without it was issued, then
//! asks gpud's ONE readback for the whole display into the VMO with the freeze flag. From the
//! moment that request is sent the screen is frozen — every present behind it in gpud's queue
//! lands over the frozen base, so windows and overlays leave the composition (they are in the
//! frame) and only the desktop surface, which hosts the capture UI, stays. `THAW` ends it, and
//! so does `FREEZE_MAX_NS` at the first wake after it: a capture service that never thaws can
//! not keep the user's screen. `PROBE` reads a rectangle without freezing (the selftest's path).
//!
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: inline + `tests/capture_gate.rs`
//! RFC: docs/rfcs/RFC-0095-screen-capture-v1-readback-freeze-shell-ui.md

use nexus_display_proto::readback::{Readback, READBACK_FREEZE};
use nexus_display_proto::surface_capture::{
    CaptureRequest, CaptureWindow, CAPTURE_ATTACH, CAPTURE_BUSY, CAPTURE_DENIED, CAPTURE_FAILED,
    CAPTURE_FREEZE, CAPTURE_MALFORMED, CAPTURE_NO_DISPLAY, CAPTURE_OK, CAPTURE_PROBE, CAPTURE_THAW,
    CAPTURE_WINDOWS_MAX,
};

/// A freeze ends by itself this long after it began (at the first wake after).
pub const FREEZE_MAX_NS: u64 = 60_000_000_000;
/// A readback gpud has not answered in this time fails the capture (and ends its freeze).
pub const READ_MAX_NS: u64 = 2_000_000_000;

/// Where the machine is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Idle,
    /// The pointer is out of the frame; the readback goes out once a present at or after
    /// `seq` was issued — the first one composed without it. `None`: the hide itself has not
    /// reached gpud yet (its queue was full), so no present counts so far.
    Hiding {
        seq: Option<u32>,
        readback: Readback,
        since_ns: u64,
    },
    /// The readback is with gpud.
    Reading {
        freeze: bool,
        since_ns: u64,
    },
    /// The frame is the base layer.
    Frozen {
        since_ns: u64,
    },
}

/// What the runtime knows when a request arrives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Facts {
    /// The kernel-stamped sender is screencapd.
    pub from_screencapd: bool,
    /// The login phase owns the display.
    pub greeter: bool,
    /// The display mode; `None` when windowd runs display-less.
    pub mode: Option<(u32, u32)>,
    /// screencapd's frame VMO is attached.
    pub vmo: bool,
    /// The display draws the pointer into the frame (GL build-up or software blend).
    pub pointer_in_frame: bool,
}

/// What an admitted request does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plan {
    /// Keep the moved VMO as the frame VMO.
    Attach,
    /// End the freeze.
    Thaw,
    /// Read through gpud; first take the pointer out of the frame when `hide_pointer`.
    Read { readback: Readback, hide_pointer: bool },
}

/// Admits a request or names its refusal status. Identity first (only screencapd's kernel sid),
/// then the login phase (no freeze while the greeter owns the display — a probe returns only
/// a verdict to the selftest and stays allowed), then the phase, the display and the VMO.
pub fn admit(req: &CaptureRequest, facts: &Facts, phase: Phase) -> Result<Plan, u8> {
    if !facts.from_screencapd {
        return Err(CAPTURE_DENIED);
    }
    let reading = matches!(phase, Phase::Hiding { .. } | Phase::Reading { .. });
    match req.cmd {
        CAPTURE_ATTACH if reading => Err(CAPTURE_BUSY),
        CAPTURE_ATTACH => Ok(Plan::Attach),
        CAPTURE_THAW if matches!(phase, Phase::Frozen { .. }) => Ok(Plan::Thaw),
        CAPTURE_THAW => Err(CAPTURE_BUSY),
        CAPTURE_FREEZE | CAPTURE_PROBE => {
            let freeze = req.cmd == CAPTURE_FREEZE;
            if freeze && facts.greeter {
                return Err(CAPTURE_DENIED);
            }
            if phase != Phase::Idle {
                return Err(CAPTURE_BUSY);
            }
            let (w, h) = facts.mode.ok_or(CAPTURE_NO_DISPLAY)?;
            if !facts.vmo {
                return Err(CAPTURE_FAILED);
            }
            let readback = if freeze {
                let clamp = |v: u32| v.min(u32::from(u16::MAX)) as u16;
                Readback { x: 0, y: 0, w: clamp(w), h: clamp(h), flags: READBACK_FREEZE }
            } else {
                let r = req.rect;
                Readback { x: r.x, y: r.y, w: r.w, h: r.h, flags: 0 }
            };
            if !readback.fits(w, h) {
                return Err(CAPTURE_MALFORMED);
            }
            Ok(Plan::Read { readback, hide_pointer: freeze && facts.pointer_in_frame })
        }
        _ => Err(CAPTURE_MALFORMED),
    }
}

/// What a step asks the runtime to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Nothing yet.
    Wait,
    /// Send this readback now (the pointer, if hidden, may come back with it).
    SendReadback(Readback),
    /// Answer the parked request with this status; `thaw` = the freeze it began must end.
    Answer { status: u8, thaw: bool },
    /// The freeze outlived `FREEZE_MAX_NS`: end it (nobody is waiting for an answer).
    Expired,
}

/// The machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Machine {
    phase: Phase,
}

impl Default for Machine {
    fn default() -> Self {
        Self::new()
    }
}

impl Machine {
    pub const fn new() -> Self {
        Self { phase: Phase::Idle }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// Windows and overlays are out of the composition: from the moment the freeze readback
    /// was sent until the thaw.
    pub fn frozen(&self) -> bool {
        matches!(self.phase, Phase::Reading { freeze: true, .. } | Phase::Frozen { .. })
    }

    /// The pointer is withheld from the frame.
    pub fn pointer_hidden(&self) -> bool {
        matches!(self.phase, Phase::Hiding { .. })
    }

    /// Starts an admitted read. `hide_seq`: when the pointer leaves the frame, the seq the next
    /// present will carry once the hide is on its way (`None`: not yet, see [`Self::hide_sent`]).
    pub fn start(
        &mut self,
        readback: Readback,
        hide_pointer: bool,
        hide_seq: Option<u32>,
        now: u64,
    ) -> Step {
        if hide_pointer {
            self.phase = Phase::Hiding { seq: hide_seq, readback, since_ns: now };
            return Step::Wait;
        }
        self.phase =
            Phase::Reading { freeze: readback.flags & READBACK_FREEZE != 0, since_ns: now };
        Step::SendReadback(readback)
    }

    /// The hide is still to be sent.
    pub fn hide_owed(&self) -> bool {
        matches!(self.phase, Phase::Hiding { seq: None, .. })
    }

    /// The hide went out; `next_seq` is the seq the next present will carry.
    pub fn hide_sent(&mut self, next_seq: u32) {
        if let Phase::Hiding { seq: seq @ None, .. } = &mut self.phase {
            *seq = Some(next_seq);
        }
    }

    /// A present pass ran: `last_issued` is the newest seq sent to gpud. While hiding, the
    /// readback goes out once a present composed without the pointer was issued.
    pub fn presented(&mut self, last_issued: u32, now: u64) -> Step {
        let Phase::Hiding { seq: Some(seq), readback, .. } = self.phase else {
            return Step::Wait;
        };
        // Seqs grow by one per present; the wrapping distance keeps the test honest at 2^32.
        if last_issued.wrapping_sub(seq) >= u32::MAX / 2 {
            return Step::Wait;
        }
        self.phase =
            Phase::Reading { freeze: readback.flags & READBACK_FREEZE != 0, since_ns: now };
        Step::SendReadback(readback)
    }

    /// The readback could not be sent (no gpud route): the capture fails, nothing froze.
    pub fn send_failed(&mut self) -> Step {
        let thaw = self.frozen();
        self.phase = Phase::Idle;
        Step::Answer { status: CAPTURE_FAILED, thaw }
    }

    /// gpud answered `status` (`0` = done).
    pub fn readback_done(&mut self, status: u8, now: u64) -> Step {
        let Phase::Reading { freeze, .. } = self.phase else { return Step::Wait };
        if status != CAPTURE_OK {
            self.phase = Phase::Idle;
            return Step::Answer { status: CAPTURE_FAILED, thaw: freeze };
        }
        self.phase = if freeze { Phase::Frozen { since_ns: now } } else { Phase::Idle };
        Step::Answer { status: CAPTURE_OK, thaw: false }
    }

    /// An admitted `THAW`.
    pub fn thaw(&mut self) {
        self.phase = Phase::Idle;
    }

    /// The bounds: a freeze past `FREEZE_MAX_NS` thaws (nobody is waiting, so no answer); a
    /// read past `READ_MAX_NS` fails its parked request (and thaws if it had frozen).
    pub fn expire(&mut self, now: u64) -> Step {
        match self.phase {
            Phase::Frozen { since_ns } if now.saturating_sub(since_ns) >= FREEZE_MAX_NS => {
                self.phase = Phase::Idle;
                Step::Expired
            }
            Phase::Hiding { since_ns, .. } | Phase::Reading { since_ns, .. }
                if now.saturating_sub(since_ns) >= READ_MAX_NS =>
            {
                let thaw = self.frozen();
                self.phase = Phase::Idle;
                Step::Answer { status: CAPTURE_FAILED, thaw }
            }
            _ => Step::Wait,
        }
    }
}

/// The on-screen windows of a freeze reply, front to back: each `(id, x, y, w, h)` clamped into
/// the wire's ranges, empty ones dropped, at most `CAPTURE_WINDOWS_MAX`.
pub fn capture_windows(
    front_to_back: impl IntoIterator<Item = (u32, i32, i32, u32, u32)>,
) -> ([CaptureWindow; CAPTURE_WINDOWS_MAX], usize) {
    let mut out = [CaptureWindow::default(); CAPTURE_WINDOWS_MAX];
    let mut n = 0;
    for (id, x, y, w, h) in front_to_back {
        if n == CAPTURE_WINDOWS_MAX {
            break;
        }
        if w == 0 || h == 0 {
            continue;
        }
        let pos = |v: i32| v.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
        let len = |v: u32| v.min(u32::from(u16::MAX)) as u16;
        out[n] = CaptureWindow { id, x: pos(x), y: pos(y), w: len(w), h: len(h) };
        n += 1;
    }
    (out, n)
}

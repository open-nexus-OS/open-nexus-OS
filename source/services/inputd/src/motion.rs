// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The pointer's samples of one HID batch (RFC-0113 §2, the `frame` class's history).
//! inputd sums a batch's relative motion into ONE accelerated step — the cursor's feel stays as it
//! was — and keeps where the pointer was at each report on the way: a report's share of the
//! batch's raw motion places it along the step, so the last sample IS the cursor. An absolute
//! source reports positions; each report's is a sample. A report is the run of events with one
//! timestamp; its sample carries that time and the sequence of its first event, and rides the
//! next push to windowd (`input_live_protocol::motion_push`). hidrawd stamps a delivery once (the
//! time it saw it): reports that reached it together share that time and fold into one sample —
//! a controller-side time per report is the USB driver's to add.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `tests/pointer_motion.rs` (through `InputdService`)

use hid::{AbsoluteAxis, HidEvent, HidEventKind, RelativeAxis};
use nexus_display_proto::surface_motion::{MotionBatch, MotionSample, MOTION_SAMPLES_MAX};
use pointer_state::PointerState;

#[derive(Clone, Copy, Default)]
struct Report {
    t_ns: u64,
    seq: u32,
    dx: i32,
    dy: i32,
    abs_x: Option<i32>,
    abs_y: Option<i32>,
}

/// One batch's reports, oldest first (past the bound the two oldest merge: their motion summed,
/// the later one's time and sequence — the fold rule of the batch they become).
pub(crate) struct Reports {
    list: [Report; MOTION_SAMPLES_MAX],
    len: usize,
}

impl Reports {
    pub(crate) fn new() -> Self {
        Self { list: [Report::default(); MOTION_SAMPLES_MAX], len: 0 }
    }

    /// Notes one event of the batch (`seq` = its sequence); motion only, the rest is not a sample.
    pub(crate) fn note(&mut self, event: &HidEvent, seq: u32) {
        let (kind, code, value) = (event.kind(), event.code().raw(), event.value().raw());
        let axis = match kind {
            HidEventKind::Rel if code == RelativeAxis::X.event_code() => 0,
            HidEventKind::Rel if code == RelativeAxis::Y.event_code() => 1,
            HidEventKind::Abs if code == AbsoluteAxis::X.event_code() => 2,
            HidEventKind::Abs if code == AbsoluteAxis::Y.event_code() => 3,
            _ => return,
        };
        let report = self.report_at(event.timestamp().raw(), seq);
        match axis {
            0 => report.dx = report.dx.saturating_add(value),
            1 => report.dy = report.dy.saturating_add(value),
            2 => report.abs_x = Some(value),
            _ => report.abs_y = Some(value),
        }
    }

    fn report_at(&mut self, t_ns: u64, seq: u32) -> &mut Report {
        if self.len == 0 || self.list[self.len - 1].t_ns != t_ns {
            if self.len == MOTION_SAMPLES_MAX {
                let first = self.list[0];
                let second = &mut self.list[1];
                second.dx = second.dx.saturating_add(first.dx);
                second.dy = second.dy.saturating_add(first.dy);
                second.abs_x = second.abs_x.or(first.abs_x);
                second.abs_y = second.abs_y.or(first.abs_y);
                self.list.copy_within(1.., 0);
                self.len -= 1;
            }
            self.list[self.len] = Report { t_ns, seq, ..Report::default() };
            self.len += 1;
        }
        &mut self.list[self.len - 1]
    }

    /// The samples of a RELATIVE step from `start` (the cursor before it) to `end` (after it):
    /// each report at its share of the batch's raw motion along the step, clamped like the
    /// cursor; the last report at `end`.
    pub(crate) fn place_relative(
        &self,
        start: PointerState,
        end: (i32, i32),
        out: &mut MotionBatch,
    ) {
        let reports = &self.list[..self.len];
        let total = reports
            .iter()
            .fold((0i64, 0i64), |(x, y), r| (x + i64::from(r.dx), y + i64::from(r.dy)));
        let from = start.display_position();
        let step = (i64::from(end.0 - from.x), i64::from(end.1 - from.y));
        let mut part = (0i64, 0i64);
        for (i, report) in reports.iter().enumerate() {
            part = (part.0 + i64::from(report.dx), part.1 + i64::from(report.dy));
            let (x, y) = if i + 1 == reports.len() {
                end
            } else {
                let mut probe = start;
                let p = probe
                    .apply_relative(along(step.0, part.0, total.0), along(step.1, part.1, total.1));
                (p.x, p.y)
            };
            out.push(MotionSample { x, y, t_ns: report.t_ns, seq: report.seq });
        }
    }

    /// The samples of an ABSOLUTE source from `start`: each report's position (an axis a report
    /// leaves out keeps the one before).
    pub(crate) fn place_absolute(&self, start: PointerState, out: &mut MotionBatch) {
        let mut probe = start;
        for report in &self.list[..self.len] {
            if report.abs_x.is_none() && report.abs_y.is_none() {
                continue;
            }
            let p = probe.apply_absolute(report.abs_x, report.abs_y);
            out.push(MotionSample { x: p.x, y: p.y, t_ns: report.t_ns, seq: report.seq });
        }
    }
}

/// `step`'s share `part / whole` (a batch whose raw motion nets out stays at the start).
fn along(step: i64, part: i64, whole: i64) -> i32 {
    if whole == 0 {
        return 0;
    }
    (step * part / whole).clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

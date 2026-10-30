// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: The device streams' sequence numbers (RFC-0113 §1). The first hop that sees an event
//! numbers it in its device's stream (hidrawd: [`SeqStreams::stamp`]); every later hop checks that
//! a batch continues its stream where the last one ended ([`SeqStreams::check`]) — a skipped
//! number is an event lost on the way, counted and named, never guessed at. Fixed storage for
//! [`STREAMS_MAX`] devices; a device past that shares the overflow stream on both sides.
//! OWNERS: @runtime @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests below (continuity, a gap, a restart, wrapping, the overflow stream)

/// Devices with a stream of their own.
pub const STREAMS_MAX: usize = 8;

/// What a batch's first sequence says about its stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeqCheck {
    /// It continues its stream (or opens it).
    InOrder,
    /// This many events of the stream never arrived.
    Gap(u32),
    /// The stream started over (its stamping hop restarted): resynchronized, nothing lost here.
    Restart,
}

/// The sequence state of the device streams one hop sees: the stamping hop's next number per
/// device, or the receiving hop's expected one.
#[derive(Debug, Clone, Copy, Default)]
pub struct SeqStreams {
    ids: [u16; STREAMS_MAX],
    /// The next sequence per stream; index [`STREAMS_MAX`] is the overflow stream.
    next: [u32; STREAMS_MAX + 1],
    open: [bool; STREAMS_MAX + 1],
}

impl SeqStreams {
    #[must_use]
    pub const fn new() -> Self {
        Self { ids: [0; STREAMS_MAX], next: [0; STREAMS_MAX + 1], open: [false; STREAMS_MAX + 1] }
    }

    /// The stamping side: the first sequence of `len` events of `device`; its stream moves past
    /// them.
    pub fn stamp(&mut self, device: u16, len: u32) -> u32 {
        let (i, _) = self.stream(device);
        let first = self.next[i];
        self.next[i] = first.wrapping_add(len);
        first
    }

    /// The receiving side: a batch of `len` events of `device` starting at `seq_first`. The
    /// stream continues from the batch either way — a gap is counted once, not on every batch
    /// after it.
    pub fn check(&mut self, device: u16, seq_first: u32, len: u32) -> SeqCheck {
        let (i, opened) = self.stream(device);
        let expected = self.next[i];
        self.next[i] = seq_first.wrapping_add(len);
        if opened {
            return SeqCheck::InOrder;
        }
        match seq_first.wrapping_sub(expected) {
            0 => SeqCheck::InOrder,
            ahead if ahead <= i32::MAX as u32 => SeqCheck::Gap(ahead),
            _ => SeqCheck::Restart,
        }
    }

    /// `device`'s stream index, and whether this call opened it.
    fn stream(&mut self, device: u16) -> (usize, bool) {
        if let Some(i) = (0..STREAMS_MAX).find(|&i| self.open[i] && self.ids[i] == device) {
            return (i, false);
        }
        let i = (0..STREAMS_MAX).find(|&i| !self.open[i]).unwrap_or(STREAMS_MAX);
        let opened = !self.open[i];
        self.open[i] = true;
        if i < STREAMS_MAX {
            self.ids[i] = device;
        }
        (i, opened)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stamped_stream_checks_in_order_per_device() {
        let (mut tx, mut rx) = (SeqStreams::new(), SeqStreams::new());
        for (device, len) in [(1u16, 3u32), (2, 5), (1, 2), (2, 1), (1, 7)] {
            let first = tx.stamp(device, len);
            assert_eq!(rx.check(device, first, len), SeqCheck::InOrder, "device {device}");
        }
        assert_eq!(tx.stamp(1, 1), 12, "device 1 numbered 3 + 2 + 7 events before");
    }

    #[test]
    fn a_lost_batch_is_a_gap_counted_once() {
        let (mut tx, mut rx) = (SeqStreams::new(), SeqStreams::new());
        let a = tx.stamp(4, 2);
        assert_eq!(rx.check(4, a, 2), SeqCheck::InOrder);
        let _lost = tx.stamp(4, 3);
        let c = tx.stamp(4, 1);
        assert_eq!(rx.check(4, c, 1), SeqCheck::Gap(3));
        let d = tx.stamp(4, 1);
        assert_eq!(rx.check(4, d, 1), SeqCheck::InOrder, "the stream continues after the gap");
    }

    #[test]
    fn a_restarted_stamper_is_a_restart_not_a_gap() {
        let mut rx = SeqStreams::new();
        assert_eq!(rx.check(1, 5_000, 4), SeqCheck::InOrder, "a stream opens where it says");
        assert_eq!(rx.check(1, 0, 1), SeqCheck::Restart);
        assert_eq!(rx.check(1, 1, 1), SeqCheck::InOrder);
    }

    #[test]
    fn the_sequence_wraps_without_a_gap() {
        let mut rx = SeqStreams::new();
        assert_eq!(rx.check(3, u32::MAX - 1, 2), SeqCheck::InOrder);
        assert_eq!(rx.check(3, 0, 1), SeqCheck::InOrder, "u32::MAX - 1 + 2 wraps to 0");
    }

    #[test]
    fn devices_past_the_table_share_the_overflow_stream_on_both_sides() {
        let (mut tx, mut rx) = (SeqStreams::new(), SeqStreams::new());
        for device in 0..(STREAMS_MAX as u16 + 3) {
            let first = tx.stamp(100 + device, 2);
            assert_eq!(rx.check(100 + device, first, 2), SeqCheck::InOrder, "device {device}");
        }
        let first = tx.stamp(100 + STREAMS_MAX as u16, 1);
        assert_eq!(first, 6, "three overflow devices numbered 2 events each in one stream");
        assert_eq!(rx.check(100 + STREAMS_MAX as u16, first, 1), SeqCheck::InOrder);
    }
}

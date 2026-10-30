// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The pointer's motion history (RFC-0113 §2, the `frame` delivery class): every sample
//! the device reported since the last frame — its position, its event's time and sequence — in
//! ONE bounded batch. Beyond the bound the oldest samples fold into one (the folded sample keeps
//! the newest of them: a position is absolute, so it stands for the stretch it ends). One type
//! for both hops: inputd → windowd (display space, behind the visible state's push) and
//! windowd → app (surface space, behind a MOVE or DRAG frame). Compact on the wire — i16
//! positions, a time as microseconds before the batch's newest — so a full batch fits a client
//! event frame. A later `stream` class delivers the same samples at the device's rate; the
//! format does not change.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable (append-only)
//! TEST_COVERAGE: unit tests below (the fold, the round trip, `test_reject_*` for a malformed
//!   batch)

/// Samples one batch keeps; at a 1000 Hz device and a 60 Hz frame the oldest few fold.
pub const MOTION_SAMPLES_MAX: usize = 16;
/// One sample on the wire: x i16, y i16, age µs u32 (before the newest), seq u32.
pub const MOTION_SAMPLE_WIRE_LEN: usize = 12;
/// The batch head: count u8, folded u16, the newest sample's time u64 (ns).
pub const MOTION_HEAD_LEN: usize = 11;
/// The largest batch on the wire.
pub const MOTION_WIRE_MAX: usize = MOTION_HEAD_LEN + MOTION_SAMPLES_MAX * MOTION_SAMPLE_WIRE_LEN;

/// One reported pointer position.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MotionSample {
    pub x: i32,
    pub y: i32,
    /// The device event's time (RFC-0113 §1 `t_ns`).
    pub t_ns: u64,
    /// The device event's sequence in its stream (RFC-0113 §1 `seq`).
    pub seq: u32,
}

/// The samples of one frame, oldest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MotionBatch {
    samples: [MotionSample; MOTION_SAMPLES_MAX],
    len: u8,
    /// Samples folded into the oldest one since the batch began.
    folded: u16,
}

impl Default for MotionBatch {
    fn default() -> Self {
        Self::new()
    }
}

impl MotionBatch {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            samples: [MotionSample { x: 0, y: 0, t_ns: 0, seq: 0 }; MOTION_SAMPLES_MAX],
            len: 0,
            folded: 0,
        }
    }

    /// Appends `sample` (the newest); a full batch first folds its two oldest into one.
    pub fn push(&mut self, sample: MotionSample) {
        if usize::from(self.len) == MOTION_SAMPLES_MAX {
            self.samples.copy_within(1.., 0);
            self.len -= 1;
            self.folded = self.folded.saturating_add(1);
        }
        self.samples[usize::from(self.len)] = sample;
        self.len += 1;
    }

    /// Appends a LATER batch: its samples after these, its folds counted.
    pub fn append(&mut self, later: &Self) {
        for sample in later.samples() {
            self.push(*sample);
        }
        self.folded = self.folded.saturating_add(later.folded);
    }

    #[must_use]
    pub fn samples(&self) -> &[MotionSample] {
        &self.samples[..usize::from(self.len)]
    }

    #[must_use]
    pub const fn folded(&self) -> u16 {
        self.folded
    }

    #[must_use]
    pub fn newest(&self) -> Option<MotionSample> {
        self.samples().last().copied()
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn clear(&mut self) {
        self.len = 0;
        self.folded = 0;
    }

    /// Every position moved by `(dx, dy)` — display space into a surface's.
    #[must_use]
    pub fn shifted(&self, dx: i32, dy: i32) -> Self {
        let mut out = *self;
        for sample in &mut out.samples[..usize::from(self.len)] {
            sample.x = sample.x.saturating_add(dx);
            sample.y = sample.y.saturating_add(dy);
        }
        out
    }

    /// The bytes this batch takes on the wire.
    #[must_use]
    pub const fn wire_len(&self) -> usize {
        MOTION_HEAD_LEN + self.len as usize * MOTION_SAMPLE_WIRE_LEN
    }

    /// Writes the batch into `out`; `None` when `out` is too short. Positions clamp to i16, an
    /// age to u32 µs (71 minutes).
    pub fn encode_into(&self, out: &mut [u8]) -> Option<usize> {
        let n = self.wire_len();
        let out = out.get_mut(..n)?;
        let newest = self.newest().map_or(0, |s| s.t_ns);
        out[0] = self.len;
        out[1..3].copy_from_slice(&self.folded.to_le_bytes());
        out[3..11].copy_from_slice(&newest.to_le_bytes());
        for (i, sample) in self.samples().iter().enumerate() {
            let at = MOTION_HEAD_LEN + i * MOTION_SAMPLE_WIRE_LEN;
            let age_us = newest.saturating_sub(sample.t_ns) / 1_000;
            out[at..at + 2].copy_from_slice(&clamp_i16(sample.x).to_le_bytes());
            out[at + 2..at + 4].copy_from_slice(&clamp_i16(sample.y).to_le_bytes());
            out[at + 4..at + 8]
                .copy_from_slice(&(age_us.min(u64::from(u32::MAX)) as u32).to_le_bytes());
            out[at + 8..at + 12].copy_from_slice(&sample.seq.to_le_bytes());
        }
        Some(n)
    }

    /// The batch that EXACTLY fills `bytes` — `None` for a count past the bound, a length that
    /// does not match it, or ages that are not oldest-first ending at the newest (fail-closed).
    #[must_use]
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let (&count, rest) = bytes.split_first()?;
        let len = usize::from(count);
        if len > MOTION_SAMPLES_MAX || bytes.len() != MOTION_HEAD_LEN + len * MOTION_SAMPLE_WIRE_LEN
        {
            return None;
        }
        let folded = u16::from_le_bytes([rest[0], rest[1]]);
        let newest = u64::from_le_bytes(rest[2..10].try_into().ok()?);
        let mut batch = Self { folded, ..Self::new() };
        let mut last_age = u32::MAX;
        for i in 0..len {
            let at = MOTION_HEAD_LEN + i * MOTION_SAMPLE_WIRE_LEN;
            let field = |o: usize, w: usize| &bytes[at + o..at + o + w];
            let age = u32::from_le_bytes(field(4, 4).try_into().ok()?);
            if age > last_age {
                return None;
            }
            last_age = age;
            batch.samples[i] = MotionSample {
                x: i32::from(i16::from_le_bytes(field(0, 2).try_into().ok()?)),
                y: i32::from(i16::from_le_bytes(field(2, 2).try_into().ok()?)),
                t_ns: newest.saturating_sub(u64::from(age) * 1_000),
                seq: u32::from_le_bytes(field(8, 4).try_into().ok()?),
            };
        }
        if len > 0 && last_age != 0 {
            return None;
        }
        batch.len = count;
        Some(batch)
    }
}

const fn clamp_i16(v: i32) -> i16 {
    if v > i16::MAX as i32 {
        i16::MAX
    } else if v < i16::MIN as i32 {
        i16::MIN
    } else {
        v as i16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(i: u32) -> MotionSample {
        MotionSample {
            x: i as i32 * 3,
            y: -(i as i32),
            t_ns: 1_000_000 + u64::from(i) * 1_000_000,
            seq: 10 + i,
        }
    }

    #[test]
    fn a_full_batch_folds_its_oldest_and_keeps_the_newest() {
        let mut batch = MotionBatch::new();
        for i in 0..20 {
            batch.push(sample(i));
        }
        assert_eq!(batch.samples().len(), MOTION_SAMPLES_MAX);
        assert_eq!(batch.folded(), 4);
        assert_eq!(
            batch.samples()[0],
            sample(4),
            "the folded sample keeps the newest it stands for"
        );
        assert_eq!(batch.newest(), Some(sample(19)));
    }

    #[test]
    fn a_later_batch_appends_with_its_folds() {
        let (mut a, mut b) = (MotionBatch::new(), MotionBatch::new());
        a.push(sample(0));
        for i in 1..=17 {
            b.push(sample(i));
        }
        assert_eq!(b.folded(), 1);
        a.append(&b);
        assert_eq!(a.samples().len(), MOTION_SAMPLES_MAX);
        assert_eq!(a.folded(), 1 + 1, "b's fold, and one push past the bound (1 + 16 samples)");
        assert_eq!(a.newest(), Some(sample(17)));
    }

    #[test]
    fn a_batch_round_trips_at_microseconds() {
        let mut batch = MotionBatch::new();
        for i in 0..5 {
            batch.push(sample(i));
        }
        let mut buf = [0u8; MOTION_WIRE_MAX];
        let n = batch.encode_into(&mut buf).expect("fits");
        assert_eq!(n, batch.wire_len());
        assert_eq!(MotionBatch::decode(&buf[..n]), Some(batch));
        let shifted = batch.shifted(-10, 4);
        assert_eq!(shifted.samples()[2].x, 6 - 10);
        assert_eq!(shifted.samples()[2].t_ns, batch.samples()[2].t_ns);
        let empty = MotionBatch::new();
        let n = empty.encode_into(&mut buf).expect("fits");
        assert_eq!(MotionBatch::decode(&buf[..n]), Some(empty));
    }

    #[test]
    fn test_reject_a_malformed_batch() {
        let mut batch = MotionBatch::new();
        for i in 0..3 {
            batch.push(sample(i));
        }
        let mut buf = [0u8; MOTION_WIRE_MAX];
        let n = batch.encode_into(&mut buf).expect("fits");
        assert_eq!(MotionBatch::decode(&buf[..n - 1]), None, "truncated");
        let mut long = buf;
        long[0] = MOTION_SAMPLES_MAX as u8 + 1;
        assert_eq!(MotionBatch::decode(&long), None, "past the bound");
        let mut swapped = buf;
        swapped[MOTION_HEAD_LEN + 4..MOTION_HEAD_LEN + 8].copy_from_slice(&0u32.to_le_bytes());
        assert_eq!(
            MotionBatch::decode(&swapped[..n]),
            None,
            "an older sample younger than a newer"
        );
        let mut late = buf;
        let at = MOTION_HEAD_LEN + 2 * MOTION_SAMPLE_WIRE_LEN + 4;
        late[at..at + 4].copy_from_slice(&5u32.to_le_bytes());
        assert_eq!(MotionBatch::decode(&late[..n]), None, "the newest must be the newest");
        assert_eq!(MotionBatch::decode(&[]), None);
    }
}

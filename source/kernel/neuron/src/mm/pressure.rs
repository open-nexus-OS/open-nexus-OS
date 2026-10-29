// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: memory pressure as a rule (TASK-0333 6e, RFC-0100 §3, ADR-0085). The pool's free
//! frames are read against three watermarks — `warn`, `critical`, `emergency` — that are
//! fractions of the frames each bank had free when the pool was built (25 %, 10 %, 5 %), summed
//! over the banks and floored at a page count, so the same code prints different absolutes on a
//! 320 MiB QEMU and a 4 GiB board. The level worsens the moment free frames cross a watermark
//! and recovers only once they clear it by an eighth (hysteresis): a boundary never flaps into
//! a storm of notifications. A level change is one frame — `OP_MEMORY_PRESSURE {level, free,
//! total}` — into the one endpoint a service bound (`mm::pressure_endpoint`); this module is the
//! arithmetic and the wire form, pure and host-tested.
//!
//! The level is the pool's, not a bank's: the buddies fill the banks in tree order, so a
//! per-bank alarm would fire on the first bank with the second still free.
//!
//! The same sink carries the swap stage's two notices to the pager (6f-2, ADR-0088): a page-in
//! request `OP_PAGE_IN {token, slot}` and the slots a dead object frees `OP_SWAP_DROP {slots}` —
//! their wire forms are here too.
//!
//! NOT target-gated (pure, like `lazy`, `growth` and `accounting`).
//!
//! OWNERS: @kernel-mm-team
//! STATUS: Functional (TASK-0333 6e-0)
//! API_STABILITY: Internal (the wire frame is `nexus-abi`'s `OP_MEMORY_PRESSURE`)
//! TEST_COVERAGE: unit tests below — the fractions and floors, the ordering on a tiny pool,
//!   the level per free count, worsening without delay, recovery only past the hysteresis,
//!   the frame round trip and the rejects of a short or foreign frame; the swap notices' round
//!   trip, the drops in batches, a malformed notice rejected

/// IPC op byte of a pressure notification (the kernel-originated events' range, next to
/// `OP_IRQ_FIRED` = 0x70). Payload: `[op, level, free: u64 LE, total: u64 LE]`.
pub const OP_MEMORY_PRESSURE: u8 = 0x71;

/// Bytes of one notification frame.
pub const FRAME_LEN: usize = 1 + 1 + 8 + 8;

/// Free frames at or below which the level is `Warn`, as a percentage of a bank's boot-time
/// free frames.
pub const WARN_PERCENT: u64 = 25;
/// `Critical`, likewise.
pub const CRITICAL_PERCENT: u64 = 10;
/// `Emergency`, likewise.
pub const EMERGENCY_PERCENT: u64 = 5;

/// The emergency watermark never sits below this many frames (1 MiB of 4 KiB pages); the
/// critical floor is twice it, the warn floor four times — the ordering holds on any pool.
pub const FLOOR_FRAMES: u64 = 256;

/// Recovery needs free frames above a watermark by this fraction of it (an eighth).
pub const HYSTERESIS_DIV: u64 = 8;

/// How tight memory is, from the pool's free frames.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// Above every watermark.
    #[default]
    Normal = 0,
    /// At or below `warn`: purgeable pages may go, caches should shrink.
    Warn = 1,
    /// At or below `critical`: the hidden bands are killed, lowest first.
    Critical = 2,
    /// At or below `emergency`: whatever is not foreground goes.
    Emergency = 3,
}

impl Level {
    /// The level a wire byte names, `None` for one this kernel does not speak.
    #[must_use]
    pub fn from_raw(raw: u8) -> Option<Level> {
        match raw {
            0 => Some(Level::Normal),
            1 => Some(Level::Warn),
            2 => Some(Level::Critical),
            3 => Some(Level::Emergency),
            _ => None,
        }
    }

    /// The level's name on the console.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Level::Normal => "normal",
            Level::Warn => "warn",
            Level::Critical => "critical",
            Level::Emergency => "emergency",
        }
    }
}

/// The three thresholds, in free frames.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Watermarks {
    pub warn: u64,
    pub critical: u64,
    pub emergency: u64,
}

impl Watermarks {
    /// The watermarks over banks that had `boot_free` frames free each when the pool was built:
    /// each bank contributes its fraction, the sum is floored.
    #[must_use]
    pub fn from_banks<I: IntoIterator<Item = u64>>(boot_free: I) -> Self {
        let mut marks = Watermarks::default();
        for free in boot_free {
            marks.warn += free * WARN_PERCENT / 100;
            marks.critical += free * CRITICAL_PERCENT / 100;
            marks.emergency += free * EMERGENCY_PERCENT / 100;
        }
        Watermarks {
            warn: marks.warn.max(FLOOR_FRAMES * 4),
            critical: marks.critical.max(FLOOR_FRAMES * 2),
            emergency: marks.emergency.max(FLOOR_FRAMES),
        }
    }

    /// `warn > critical > emergency > 0` — what `from_banks` guarantees and the boot proves.
    #[must_use]
    pub fn well_formed(self) -> bool {
        self.warn > self.critical && self.critical > self.emergency && self.emergency > 0
    }

    /// The level `free` frames mean, without hysteresis.
    #[must_use]
    pub fn level(self, free: u64) -> Level {
        if free <= self.emergency {
            Level::Emergency
        } else if free <= self.critical {
            Level::Critical
        } else if free <= self.warn {
            Level::Warn
        } else {
            Level::Normal
        }
    }

    /// The level `free` frames mean for a recovery: every watermark raised by its eighth.
    fn level_for_recovery(self, free: u64) -> Level {
        let raised = |mark: u64| mark + mark / HYSTERESIS_DIV;
        Watermarks {
            warn: raised(self.warn),
            critical: raised(self.critical),
            emergency: raised(self.emergency),
        }
        .level(free)
    }
}

/// The level over time: worsens at once, recovers past the hysteresis.
#[derive(Clone, Copy, Debug)]
pub struct Tracker {
    marks: Watermarks,
    level: Level,
}

impl Tracker {
    /// A tracker at the level `free` frames mean now.
    #[must_use]
    pub fn new(marks: Watermarks, free: u64) -> Self {
        Tracker { marks, level: marks.level(free) }
    }

    /// The watermarks.
    #[must_use]
    pub fn watermarks(&self) -> Watermarks {
        self.marks
    }

    /// The level now.
    #[must_use]
    pub fn level(&self) -> Level {
        self.level
    }

    /// Note `free` frames; the new level when it changed.
    pub fn observe(&mut self, free: u64) -> Option<Level> {
        let raw = self.marks.level(free);
        let next = if raw > self.level {
            raw
        } else {
            // Recovery: only past the raised watermarks, and never below what `free` means.
            self.marks.level_for_recovery(free).max(raw).min(self.level)
        };
        if next == self.level {
            return None;
        }
        self.level = next;
        Some(next)
    }
}

/// One notification frame.
#[must_use]
pub fn encode(level: Level, free: u64, total: u64) -> [u8; FRAME_LEN] {
    let mut frame = [0u8; FRAME_LEN];
    frame[0] = OP_MEMORY_PRESSURE;
    frame[1] = level as u8;
    frame[2..10].copy_from_slice(&free.to_le_bytes());
    frame[10..18].copy_from_slice(&total.to_le_bytes());
    frame
}

/// `(level, free, total)` of a notification frame; `None` for anything else.
#[must_use]
pub fn decode(frame: &[u8]) -> Option<(Level, u64, u64)> {
    if frame.len() != FRAME_LEN || frame[0] != OP_MEMORY_PRESSURE {
        return None;
    }
    let level = Level::from_raw(frame[1])?;
    let word = |at: usize| -> Option<u64> {
        let bytes: [u8; 8] = frame[at..at + 8].try_into().ok()?;
        Some(u64::from_le_bytes(bytes))
    };
    Some((level, word(2)?, word(10)?))
}

/// IPC op byte of a page-in request (TASK-0333 6f-2, ADR-0088): a thread touched a swapped page
/// and is parked until the pager hands the page's sealed record back (`swap_page_in`, 73) — one
/// request per page, however many threads wait. Payload: `[op, token: u64 LE, slot: u32 LE]`.
pub const OP_PAGE_IN: u8 = 0x72;
/// IPC op byte of the pager's slots an object no longer needs — it died or was purged with
/// swapped pages (ADR-0088). Payload: `[op, count: u8, slot: u32 LE × count]`.
pub const OP_SWAP_DROP: u8 = 0x73;
/// Bytes of a page-in request.
pub const PAGE_IN_LEN: usize = 1 + 8 + 4;
/// Slots one drop frame carries at most.
pub const DROP_BATCH: usize = 32;
/// Bytes of the largest drop frame.
pub const DROP_MAX_LEN: usize = 2 + DROP_BATCH * 4;

/// One page-in request.
#[must_use]
pub fn encode_page_in(token: u64, slot: u32) -> [u8; PAGE_IN_LEN] {
    let mut frame = [0u8; PAGE_IN_LEN];
    frame[0] = OP_PAGE_IN;
    frame[1..9].copy_from_slice(&token.to_le_bytes());
    frame[9..13].copy_from_slice(&slot.to_le_bytes());
    frame
}

/// `(token, slot)` of a page-in request; `None` for anything else.
#[must_use]
pub fn decode_page_in(frame: &[u8]) -> Option<(u64, u32)> {
    if frame.len() != PAGE_IN_LEN || frame[0] != OP_PAGE_IN {
        return None;
    }
    let token = u64::from_le_bytes(frame[1..9].try_into().ok()?);
    Some((token, u32::from_le_bytes(frame[9..13].try_into().ok()?)))
}

/// The first [`DROP_BATCH`] of `slots` as one drop frame in `out`; answers the frame's length
/// (the caller sends `out[..len]` and keeps the slots past the batch for the next frame).
pub fn encode_drops(slots: &[u32], out: &mut [u8; DROP_MAX_LEN]) -> usize {
    let n = slots.len().min(DROP_BATCH);
    out[0] = OP_SWAP_DROP;
    out[1] = n as u8;
    for (i, slot) in slots[..n].iter().enumerate() {
        out[2 + i * 4..6 + i * 4].copy_from_slice(&slot.to_le_bytes());
    }
    2 + n * 4
}

/// The slots of a drop frame into `out`; answers how many. `None` for anything else, or a frame
/// whose count disagrees with its length.
pub fn decode_drops(frame: &[u8], out: &mut [u32; DROP_BATCH]) -> Option<usize> {
    let (&op, rest) = frame.split_first()?;
    let (&count, slots) = rest.split_first()?;
    let n = usize::from(count);
    if op != OP_SWAP_DROP || n > DROP_BATCH || slots.len() != n * 4 {
        return None;
    }
    for (i, bytes) in slots.chunks_exact(4).enumerate() {
        out[i] = u32::from_le_bytes(bytes.try_into().ok()?);
    }
    Some(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_watermarks_are_fractions_of_each_banks_boot_free_summed() {
        // The board: two banks, 996 710 frames free at boot in all (cycle 72b).
        let marks = Watermarks::from_banks([500_000, 496_710]);
        assert_eq!(marks, Watermarks { warn: 249_177, critical: 99_671, emergency: 49_835 });
        assert!(marks.well_formed());
        // QEMU: one bank, 43 383 free.
        let qemu = Watermarks::from_banks([43_383]);
        assert_eq!(qemu, Watermarks { warn: 10_845, critical: 4_338, emergency: 2_169 });
        assert!(qemu.well_formed());
    }

    #[test]
    fn a_tiny_pool_sits_on_the_floors_and_stays_ordered() {
        let marks = Watermarks::from_banks([1000]);
        assert_eq!(marks, Watermarks { warn: 1024, critical: 512, emergency: 256 });
        assert!(marks.well_formed());
        assert!(Watermarks::from_banks([]).well_formed(), "no bank: the floors");
        assert!(!Watermarks { warn: 5, critical: 5, emergency: 1 }.well_formed());
        assert!(!Watermarks { warn: 9, critical: 5, emergency: 0 }.well_formed());
    }

    #[test]
    fn the_level_is_read_at_or_below_each_watermark() {
        let marks = Watermarks { warn: 1000, critical: 400, emergency: 200 };
        assert_eq!(marks.level(5000), Level::Normal);
        assert_eq!(marks.level(1001), Level::Normal);
        assert_eq!(marks.level(1000), Level::Warn);
        assert_eq!(marks.level(401), Level::Warn);
        assert_eq!(marks.level(400), Level::Critical);
        assert_eq!(marks.level(200), Level::Emergency);
        assert_eq!(marks.level(0), Level::Emergency);
    }

    #[test]
    fn the_tracker_worsens_at_once_and_recovers_only_past_an_eighth() {
        let marks = Watermarks { warn: 1000, critical: 400, emergency: 200 };
        let mut tracker = Tracker::new(marks, 5000);
        assert_eq!(tracker.level(), Level::Normal);
        assert_eq!(tracker.observe(1000), Some(Level::Warn));
        assert_eq!(tracker.observe(999), None, "still warn");
        // Back above the mark, but not by an eighth: the level holds.
        assert_eq!(tracker.observe(1100), None);
        assert_eq!(tracker.observe(1125), None, "exactly the raised mark is still warn");
        assert_eq!(tracker.observe(1126), Some(Level::Normal));
        // Two levels at once, down and up.
        assert_eq!(tracker.observe(150), Some(Level::Emergency));
        assert_eq!(tracker.observe(230), Some(Level::Critical), "past 200 + 25");
        assert_eq!(tracker.observe(5000), Some(Level::Normal));
        // A pool born under pressure starts there.
        assert_eq!(Tracker::new(marks, 300).level(), Level::Critical);
    }

    #[test]
    fn the_frame_round_trips_and_the_rejects_hold() {
        let frame = encode(Level::Critical, 4_338, 77_261);
        assert_eq!(frame[0], OP_MEMORY_PRESSURE);
        assert_eq!(decode(&frame), Some((Level::Critical, 4_338, 77_261)));
        assert_eq!(Level::from_raw(3), Some(Level::Emergency));
        assert_eq!(Level::Emergency.name(), "emergency");
    }

    #[test]
    fn test_reject_a_short_foreign_or_unknown_level_frame() {
        let frame = encode(Level::Warn, 1, 2);
        assert_eq!(decode(&frame[..FRAME_LEN - 1]), None, "short");
        let mut foreign = frame;
        foreign[0] = 0x70;
        assert_eq!(decode(&foreign), None, "another op");
        let mut unknown = frame;
        unknown[1] = 9;
        assert_eq!(decode(&unknown), None, "a level this kernel does not speak");
        assert_eq!(Level::from_raw(4), None);
    }

    /// The pager's two notices (ADR-0088): a request round trips; drops leave in batches.
    #[test]
    fn the_swap_notices_round_trip_and_drops_leave_in_batches() {
        let token = (7u64 << 32) | 12;
        let frame = encode_page_in(token, 4_099);
        assert_eq!(decode_page_in(&frame), Some((token, 4_099)));
        let slots: [u32; 40] = core::array::from_fn(|i| 100 + i as u32);
        let mut out = [0u8; DROP_MAX_LEN];
        let len = encode_drops(&slots, &mut out);
        assert_eq!(len, DROP_MAX_LEN, "the first frame is full");
        let mut back = [0u32; DROP_BATCH];
        assert_eq!(decode_drops(&out[..len], &mut back), Some(DROP_BATCH));
        assert_eq!(back, slots[..DROP_BATCH]);
        let len = encode_drops(&slots[DROP_BATCH..], &mut out);
        assert_eq!(decode_drops(&out[..len], &mut back), Some(8), "the rest in the next");
        assert_eq!(back[..8], slots[DROP_BATCH..]);
        assert_eq!(encode_drops(&[], &mut out), 2, "an empty batch is a header");
    }

    #[test]
    fn test_reject_a_malformed_swap_notice() {
        let frame = encode_page_in(1, 2);
        assert_eq!(decode_page_in(&frame[..PAGE_IN_LEN - 1]), None, "short");
        assert_eq!(decode_page_in(&encode(Level::Warn, 1, 2)), None, "another op");
        let mut out = [0u8; DROP_MAX_LEN];
        let len = encode_drops(&[5, 6, 7], &mut out);
        let mut back = [0u32; DROP_BATCH];
        assert_eq!(decode_drops(&out[..len - 1], &mut back), None, "a slot cut short");
        out[1] = 4;
        assert_eq!(decode_drops(&out[..len], &mut back), None, "a count past the bytes");
        let mut over = [0u8; 2 + 33 * 4];
        over[0] = OP_SWAP_DROP;
        over[1] = 33;
        assert_eq!(decode_drops(&over, &mut back), None, "more than a batch");
        assert_eq!(decode_drops(&frame, &mut back), None, "a request is no drop");
    }
}

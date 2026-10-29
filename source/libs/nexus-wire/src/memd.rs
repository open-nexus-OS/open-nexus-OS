// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: memd wire v1 (RFC-0100 §3, ADR-0086, TASK-0333 6e-2) — the ONE memory authority's
//! frames in the `'M','D'` envelope. The lifecycle authority (abilitymgr) PUSHES each app
//! process's band (`BAND`, one-way, taken from its kernel sender id alone); the proof harness
//! asks the authority's state (`STATUS`) and, on a QEMU lane only, runs a pressure DRILL: a
//! simulated level over the real bands — the victim, the order, the kill and the reap are the
//! live path. Identity is the kernel sender id, never a payload byte. The kernel's own frame
//! (`OP_MEMORY_PRESSURE`) is `nexus-abi`'s, not this module's.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable (v1; ops append-only)
//! TEST_COVERAGE: round-trips + reject matrix below
//! RFC: docs/rfcs/RFC-0100-memory-object-model-v2-commit-on-touch-unified-gpu-memory-pressure.md

/// Envelope magic byte 0.
pub const MAGIC0: u8 = b'M';
/// Envelope magic byte 1.
pub const MAGIC1: u8 = b'D';
/// Wire version.
pub const VERSION: u8 = 1;

/// One app process's band changed (a one-way push from the lifecycle authority; never answered).
pub const OP_BAND: u8 = 1;
/// The authority's state: the level, the sink, the bands it knows. Reply: [`encode_status_reply`].
pub const OP_STATUS: u8 = 2;
/// A pressure drill (the harness, a QEMU lane): run the kill policy at `level` over the real
/// bands. Reply: [`encode_drill_reply`].
pub const OP_DRILL: u8 = 3;
/// The level, told onward (TASK-0333 6e-3): memd's one-way push to gpud (its caches) and to
/// windowd (every app window, as a DSL event through app-host). Never answered; taken from
/// memd's kernel sender id alone.
pub const OP_LEVEL: u8 = 4;
/// GPU memory charged to its client (TASK-0333 6e-4, RFC-0100 §3): gpud's one-way push of what
/// it holds FOR `client` (a kernel service id) — the framebuffer it lent, the buffers of the
/// client's GPU context — as an absolute figure that replaces the last one (zero frees the row).
/// Never answered; taken from gpud's kernel sender id alone.
pub const OP_GPU_CHARGE: u8 = 5;

/// `OP_LEVEL`'s source: the kernel's pressure frame — the pool IS this tight.
pub const SOURCE_KERNEL: u8 = 0;
/// `OP_LEVEL`'s source: the proof harness's drill — a simulated level, the real path.
pub const SOURCE_DRILL: u8 = 1;

/// The source's name on the console.
#[must_use]
pub const fn source_name(source: u8) -> &'static str {
    match source {
        SOURCE_KERNEL => "oom",
        SOURCE_DRILL => "drill",
        _ => "?",
    }
}

/// The band of the focused window's process — never killed.
pub const BAND_FOREGROUND: u8 = 0;
/// On screen, not focused — killed at `emergency` only.
pub const BAND_VISIBLE: u8 = 1;
/// Hidden, holding `nexus.permission.BACKGROUND` (runs while hidden) — killed at `emergency`.
pub const BAND_DECLARED: u8 = 2;
/// Hidden, the freeze grace running — killed at `critical`, after the suspended.
pub const BAND_HIDDEN: u8 = 3;
/// Frozen — the first to go at `critical`.
pub const BAND_SUSPENDED: u8 = 4;
/// The process is gone (its death reported): the entry leaves the table.
pub const BAND_GONE: u8 = 0xFF;

/// The band's name on the console (`"?"` for one this wire does not speak).
#[must_use]
pub const fn band_name(band: u8) -> &'static str {
    match band {
        BAND_FOREGROUND => "foreground",
        BAND_VISIBLE => "visible",
        BAND_DECLARED => "declared",
        BAND_HIDDEN => "hidden",
        BAND_SUSPENDED => "suspended",
        BAND_GONE => "gone",
        _ => "?",
    }
}

/// `true` for a band a push may carry.
#[must_use]
pub const fn band_known(band: u8) -> bool {
    band <= BAND_SUSPENDED || band == BAND_GONE
}

/// Reply status: served.
pub const STATUS_OK: u8 = 0;
/// Reply status: the request did not decode or named a level this wire does not speak.
pub const STATUS_MALFORMED: u8 = 1;
/// Reply status: refused — the sender may not ask this.
pub const STATUS_DENIED: u8 = 2;
/// Reply status: the drill found no process the policy may kill at that level.
pub const STATUS_NO_VICTIM: u8 = 3;
/// Reply status: the authority's kill order was not carried out (the lifecycle authority refused
/// or is gone).
pub const STATUS_FAILED: u8 = 4;
/// Reply status: the drill's level squeezed the named process instead of killing one
/// (TASK-0333 6f-1, ADR-0087) — at `critical` a suspended app is squeezed before any kill.
pub const STATUS_SQUEEZED: u8 = 5;
/// Reply status: the drill's level swapped the named process out instead of killing one
/// (TASK-0333 6f-2, ADR-0088) — at `critical` a squeezed suspended app goes to the pager's disk
/// before any kill.
pub const STATUS_SWAPPED: u8 = 6;

/// A buffer that holds any request of this protocol (the level push is the longest).
pub const REQUEST_MAX_BYTES: usize = 4 + 1 + 1 + 8 + 8;
/// A buffer that holds any reply of this protocol.
pub const REPLY_MAX_BYTES: usize = 4 + 1 + 1 + 1 + 1 + 1 + 2 + 2 + 8 + 8;

crate::frames! {
    protocol(magic0 = MAGIC0, magic1 = MAGIC1, version = VERSION);

    /// Band push: `[M, D, ver, OP_BAND, pid:u32, band]`.
    request fixed encode_band / decode_band (op = OP_BAND) {
        pid: u32le,
        band: u8,
    }
    /// Status: `[M, D, ver, OP_STATUS, 0]`.
    request fixed encode_status / decode_status (op = OP_STATUS) {
        reserved: pad(1),
    }
    /// Status reply: `[M, D, ver, op|0x80, status, level, sink, bands, suspended, resumed:u16,
    /// kills:u16, free:u64, total:u64]` — `sink` 1 when the kernel's pressure endpoint is this
    /// service's; `bands` the live processes known; `suspended` those in the suspended band;
    /// `resumed` the suspended-to-running transitions seen; `kills` the kills ordered; the
    /// frames of the last pressure frame.
    #[allow(clippy::too_many_arguments)]
    reply fixed encode_status_reply / decode_status_reply (op = caller) {
        status: u8,
        level: u8,
        sink: u8,
        bands: u8,
        suspended: u8,
        resumed: u16le,
        kills: u16le,
        free: u64le,
        total: u64le,
    }
    /// Drill: `[M, D, ver, OP_DRILL, level]`.
    request fixed encode_drill / decode_drill (op = OP_DRILL) {
        level: u8,
    }
    /// Drill reply: `[M, D, ver, op|0x80, status, pid:u32, band]` — the victim ordered killed.
    reply fixed encode_drill_reply / decode_drill_reply (op = caller) {
        status: u8,
        pid: u32le,
        band: u8,
    }
    /// Level push: `[M, D, ver, OP_LEVEL, level, source, free:u64, total:u64]` — the pool's
    /// frames as the kernel's frame carried them (a drill repeats the last ones).
    request fixed encode_level / decode_level (op = OP_LEVEL) {
        level: u8,
        source: u8,
        free: u64le,
        total: u64le,
    }
    /// GPU charge push: `[M, D, ver, OP_GPU_CHARGE, client:u64, bytes:u64]` — what gpud holds
    /// for `client` now (absolute; replaces the last figure).
    request fixed encode_gpu_charge / decode_gpu_charge (op = OP_GPU_CHARGE) {
        client: u64le,
        bytes: u64le,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn band_push_round_trips_and_names_its_band() {
        let f = encode_band(106, BAND_SUSPENDED);
        assert_eq!(f, [b'M', b'D', 1, OP_BAND, 106, 0, 0, 0, BAND_SUSPENDED]);
        assert_eq!(decode_band(&f), Some((106, BAND_SUSPENDED)));
        assert_eq!(band_name(BAND_SUSPENDED), "suspended");
        assert_eq!(band_name(BAND_GONE), "gone");
        assert!(band_known(BAND_GONE) && band_known(BAND_FOREGROUND) && !band_known(5));
    }

    #[test]
    fn gpu_charge_push_round_trips_and_fits_the_request_buffer() {
        let f = encode_gpu_charge(0xAB11_0000_0000_0001, 41_943_040);
        assert_eq!(&f[..4], &[b'M', b'D', 1, OP_GPU_CHARGE]);
        assert!(f.len() <= REQUEST_MAX_BYTES);
        assert_eq!(decode_gpu_charge(&f), Some((0xAB11_0000_0000_0001, 41_943_040)));
        assert_eq!(decode_gpu_charge(&f[..f.len() - 1]), None, "short");
        assert_eq!(decode_gpu_charge(&encode_level(1, SOURCE_DRILL, 1, 2)), None, "another op");
    }

    #[test]
    fn level_push_round_trips_and_is_the_longest_request() {
        let f = encode_level(1, SOURCE_DRILL, 4_338, 77_261);
        assert_eq!(f.len(), REQUEST_MAX_BYTES);
        assert_eq!(&f[..6], &[b'M', b'D', 1, OP_LEVEL, 1, SOURCE_DRILL]);
        assert_eq!(decode_level(&f), Some((1, SOURCE_DRILL, 4_338, 77_261)));
        assert_eq!(decode_level(&f[..f.len() - 1]), None, "short");
        assert_eq!(decode_level(&encode_band(1, 2)), None, "another op");
        assert_eq!(source_name(SOURCE_KERNEL), "oom");
        assert_eq!(source_name(SOURCE_DRILL), "drill");
    }

    #[test]
    fn status_and_drill_round_trip() {
        assert_eq!(encode_status(), [b'M', b'D', 1, OP_STATUS, 0]);
        assert_eq!(decode_status(&encode_status()), Some(()));
        let r = encode_status_reply(OP_STATUS, STATUS_OK, 2, 1, 3, 1, 7, 9, 4_338, 77_261);
        assert_eq!(r.len(), REPLY_MAX_BYTES);
        assert_eq!(
            decode_status_reply(OP_STATUS, &r),
            Some((STATUS_OK, 2, 1, 3, 1, 7, 9, 4_338, 77_261))
        );
        let d = encode_drill(2);
        assert_eq!(decode_drill(&d), Some(2));
        let dr = encode_drill_reply(OP_DRILL, STATUS_OK, 106, BAND_SUSPENDED);
        assert_eq!(decode_drill_reply(OP_DRILL, &dr), Some((STATUS_OK, 106, BAND_SUSPENDED)));
    }

    #[test]
    fn test_reject_short_foreign_and_mismatched_frames() {
        let mut f = encode_band(1, BAND_HIDDEN);
        assert!(decode_band(&f[..8]).is_none(), "short");
        f[0] = b'X';
        assert!(decode_band(&f).is_none(), "foreign magic");
        let mut s = encode_status();
        s[2] = 2;
        assert!(decode_status(&s).is_none(), "foreign version");
        let r = encode_drill_reply(OP_DRILL, STATUS_OK, 1, 0);
        assert!(decode_drill_reply(OP_STATUS, &r).is_none(), "a reply to another op");
        assert!(decode_drill(&encode_status()).is_none(), "another request's op");
    }
}

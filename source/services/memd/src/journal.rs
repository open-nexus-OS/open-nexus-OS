// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: memd's kill journal (RFC-0100 §5, TASK-0333 6f-3) — the ledger of the kills this
//! authority ordered and saw carried out, kept in the state store under [`JOURNAL_KEY`] so it
//! outlives the boot: the victim (this boot's pid, its band), why (`oom` — the kernel's level;
//! `drill` — the proof harness's), how tight memory was when the level was judged (the level,
//! free and total frames) and when it was proven (milliseconds since boot). A ring of
//! [`MAX_RECORDS`]: the oldest record goes first; the sequence number runs on across boots, so
//! a reader sees which kills it has not seen yet. Counts and identities of processes only —
//! never anything an app held. [`Ledger`] is the boot's side: it reads the journal at the first
//! kill, appends every kill, writes the whole ring back and reads it again (the read-back is the
//! proof); a kill proven while the store could not be read waits in memory. Pure and host-tested
//! over [`Store`]: the OS loop hands it statefsd's client.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Internal (the record layout is versioned: [`VERSION`])
//! TEST_COVERAGE: tests/journal.rs (the round trip, the ring, the sequence across a reload, the
//!   ledger over a store that refuses or answers other bytes, `test_reject_*` on a foreign, a
//!   truncated and an overfull record)

use alloc::vec::Vec;

use crate::authority::{Pending, Source};

/// The state-store key of the journal.
pub const JOURNAL_KEY: &str = "/state/memd/journal";
/// Records the journal keeps (the newest).
pub const MAX_RECORDS: usize = 16;
/// The record's magic.
pub const MAGIC: [u8; 4] = *b"NXMJ";
/// The record's layout version.
pub const VERSION: u8 = 1;
/// The header: magic, version, count, two bytes reserved, the next sequence number.
pub const HEADER_BYTES: usize = 4 + 1 + 1 + 2 + 4;
/// One kill's bytes.
pub const RECORD_BYTES: usize = 24;
/// The largest journal.
pub const MAX_BYTES: usize = HEADER_BYTES + MAX_RECORDS * RECORD_BYTES;

/// The kernel's level made the decision.
pub const REASON_OOM: u8 = 1;
/// The proof harness's drill made it.
pub const REASON_DRILL: u8 = 2;

/// The reason's word on the console.
#[must_use]
pub const fn reason_name(reason: u8) -> &'static str {
    match reason {
        REASON_OOM => "oom",
        REASON_DRILL => "drill",
        _ => "?",
    }
}

/// One kill, as recorded.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KillRecord {
    /// The journal's sequence number (runs on across boots).
    pub seq: u32,
    /// The victim (its pid in the boot that killed it).
    pub pid: u32,
    /// Its band when chosen (`nexus_wire::memd::BAND_*`).
    pub band: u8,
    /// [`REASON_OOM`] or [`REASON_DRILL`].
    pub reason: u8,
    /// The pressure level judged (`MEM_PRESSURE_*`).
    pub level: u8,
    /// Free frames when the level was judged (saturated at `u32::MAX`).
    pub free: u32,
    /// Total frames of the pool.
    pub total: u32,
    /// Milliseconds since boot when the kill was proven (saturated).
    pub uptime_ms: u32,
}

impl KillRecord {
    fn encode(&self, out: &mut [u8]) {
        out[0..4].copy_from_slice(&self.seq.to_le_bytes());
        out[4..8].copy_from_slice(&self.pid.to_le_bytes());
        out[8] = self.band;
        out[9] = self.reason;
        out[10] = self.level;
        out[11] = 0;
        out[12..16].copy_from_slice(&self.free.to_le_bytes());
        out[16..20].copy_from_slice(&self.total.to_le_bytes());
        out[20..24].copy_from_slice(&self.uptime_ms.to_le_bytes());
    }

    fn decode(bytes: &[u8]) -> Option<Self> {
        let word = |at: usize| Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?));
        Some(Self {
            seq: word(0)?,
            pid: word(4)?,
            band: *bytes.get(8)?,
            reason: *bytes.get(9)?,
            level: *bytes.get(10)?,
            free: word(12)?,
            total: word(16)?,
            uptime_ms: word(20)?,
        })
    }
}

/// The journal: the newest [`MAX_RECORDS`] kills, oldest first, and the next sequence number.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Journal {
    records: Vec<KillRecord>,
    next_seq: u32,
}

impl Default for Journal {
    fn default() -> Self {
        Self::new()
    }
}

impl Journal {
    /// An empty journal; the first kill is number 1.
    #[must_use]
    pub const fn new() -> Self {
        Self { records: Vec::new(), next_seq: 1 }
    }

    /// The journal stored in `bytes`; `None` unless it is exactly this layout — the magic, the
    /// version, a count within the ring, the bytes of that many records and nothing after.
    #[must_use]
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < HEADER_BYTES || bytes[..4] != MAGIC || bytes[4] != VERSION {
            return None;
        }
        let count = usize::from(bytes[5]);
        if count > MAX_RECORDS || bytes.len() != HEADER_BYTES + count * RECORD_BYTES {
            return None;
        }
        let next_seq = u32::from_le_bytes(bytes[8..12].try_into().ok()?);
        let records = bytes[HEADER_BYTES..]
            .chunks_exact(RECORD_BYTES)
            .map(KillRecord::decode)
            .collect::<Option<Vec<_>>>()?;
        Some(Self { records, next_seq })
    }

    /// The bytes the state store keeps.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = alloc::vec![0u8; HEADER_BYTES + self.records.len() * RECORD_BYTES];
        out[..4].copy_from_slice(&MAGIC);
        out[4] = VERSION;
        out[5] = self.records.len() as u8;
        out[8..12].copy_from_slice(&self.next_seq.to_le_bytes());
        for (record, at) in
            self.records.iter().zip(out[HEADER_BYTES..].chunks_exact_mut(RECORD_BYTES))
        {
            record.encode(at);
        }
        out
    }

    /// Records one kill: it takes the next sequence number, the oldest record goes when the ring
    /// is full. Answers the record as kept.
    pub fn record(&mut self, kill: KillRecord) -> KillRecord {
        let kept = KillRecord { seq: self.next_seq, ..kill };
        self.next_seq = self.next_seq.wrapping_add(1).max(1);
        if self.records.len() == MAX_RECORDS {
            self.records.remove(0);
        }
        self.records.push(kept);
        kept
    }

    /// Records kept.
    #[must_use]
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// No record kept.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// The newest record.
    #[must_use]
    pub fn last(&self) -> Option<KillRecord> {
        self.records.last().copied()
    }

    /// The records, oldest first.
    #[must_use]
    pub fn records(&self) -> &[KillRecord] {
        &self.records
    }
}

impl KillRecord {
    /// The record of the kill `pending` names, proven `uptime_ms` after boot (the sequence
    /// number is the journal's to give: [`Journal::record`]).
    #[must_use]
    pub fn of(pending: &Pending, uptime_ms: u64) -> Self {
        let word = |v: u64| u32::try_from(v).unwrap_or(u32::MAX);
        Self {
            seq: 0,
            pid: pending.pid,
            band: pending.band,
            reason: match pending.source {
                Source::Kernel => REASON_OOM,
                Source::Drill => REASON_DRILL,
            },
            level: pending.level,
            free: word(pending.free),
            total: word(pending.total),
            uptime_ms: word(uptime_ms),
        }
    }
}

/// The state store as the journal sees it (the OS: statefsd through its client; the host
/// tests: a map).
pub trait Store {
    /// The bytes under `key`: `Ok(None)` when the key is unset, `Err` when the store refused.
    fn get(&mut self, key: &str) -> Result<Option<Vec<u8>>, StoreRefused>;
    /// Replaces the bytes under `key`.
    fn put(&mut self, key: &str, value: &[u8]) -> Result<(), StoreRefused>;
}

/// The store refused the request (no route, no grant, an I/O error: the kill stands).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StoreRefused;

/// What one write did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Written {
    /// Written and read back: the newest record as the store keeps it, the records it holds;
    /// `reset` — the stored record was not this layout and a fresh journal replaced it.
    Proven { kept: KillRecord, records: usize, reset: bool },
    /// The store could not be read; the kill waits in memory for the next write.
    ReadRefused,
    /// The write was refused; the journal (this kill in it) is written again at the next kill.
    WriteRefused,
    /// The store answered other bytes than the ones written.
    ReadBackDiffers,
}

/// The journal across one boot: read once from the store, at the first kill; every kill since
/// is in it, written or not. Kills proven before the store could be read wait in `unread` (the
/// newest [`MAX_RECORDS`]) and take their numbers when it could.
#[derive(Debug, Default)]
pub struct Ledger {
    journal: Option<Journal>,
    unread: Vec<KillRecord>,
}

impl Ledger {
    /// Nothing read yet.
    #[must_use]
    pub const fn new() -> Self {
        Self { journal: None, unread: Vec::new() }
    }

    /// Records `kill`, writes the journal to `store` and reads it back.
    pub fn record(&mut self, kill: KillRecord, store: &mut dyn Store) -> Written {
        if self.unread.len() == MAX_RECORDS {
            self.unread.remove(0);
        }
        self.unread.push(kill);
        let mut reset = false;
        let journal = match self.journal.take() {
            Some(journal) => journal,
            None => match store.get(JOURNAL_KEY) {
                Ok(Some(bytes)) => Journal::decode(&bytes).unwrap_or_else(|| {
                    reset = true;
                    Journal::new()
                }),
                Ok(None) => Journal::new(),
                Err(StoreRefused) => return Written::ReadRefused,
            },
        };
        let journal = self.journal.insert(journal);
        let mut kept = KillRecord::default();
        for kill in self.unread.drain(..) {
            kept = journal.record(kill);
        }
        if store.put(JOURNAL_KEY, &journal.encode()).is_err() {
            return Written::WriteRefused;
        }
        match store.get(JOURNAL_KEY) {
            Ok(Some(bytes)) => match Journal::decode(&bytes) {
                Some(back) if back == *journal => {
                    Written::Proven { kept, records: back.len(), reset }
                }
                _ => Written::ReadBackDiffers,
            },
            _ => Written::ReadBackDiffers,
        }
    }

    /// The journal as this boot knows it (`None` until the store was read).
    #[must_use]
    pub fn journal(&self) -> Option<&Journal> {
        self.journal.as_ref()
    }
}

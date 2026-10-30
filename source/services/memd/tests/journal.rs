// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! TASK-0333 6f-3 host proofs: memd's kill journal. A kill is recorded with the next sequence
//! number and round-trips through the bytes the state store keeps; the ring keeps the newest
//! sixteen and the numbering runs on across a reload (a new boot reading the old journal); a
//! record that is not exactly this layout is refused (`test_reject_*`), never half-read. The
//! ledger writes every kill and reads it back; a kill proven while the store cannot be read, or a
//! write the store refused, is written with the next kill — numbered in the order the kills came.

use std::collections::BTreeMap;

use memd::authority::{Pending, Source};
use memd::journal::{
    reason_name, Journal, KillRecord, Ledger, Store, StoreRefused, Written, HEADER_BYTES,
    JOURNAL_KEY, MAGIC, MAX_BYTES, MAX_RECORDS, REASON_DRILL, REASON_OOM, RECORD_BYTES, VERSION,
};
use nexus_abi::syscall::memory::MEM_PRESSURE_CRITICAL;
use nexus_wire::memd::BAND_SUSPENDED;

/// A state store over a map that refuses what it is told to.
#[derive(Default)]
struct MapStore {
    keys: BTreeMap<String, Vec<u8>>,
    refuse_get: bool,
    refuse_put: bool,
    /// Takes the put, keeps the old bytes (a store that answers other bytes than written).
    drop_puts: bool,
}

impl Store for MapStore {
    fn get(&mut self, key: &str) -> Result<Option<Vec<u8>>, StoreRefused> {
        if self.refuse_get {
            return Err(StoreRefused);
        }
        Ok(self.keys.get(key).cloned())
    }

    fn put(&mut self, key: &str, value: &[u8]) -> Result<(), StoreRefused> {
        if self.refuse_put {
            return Err(StoreRefused);
        }
        if !self.drop_puts {
            self.keys.insert(key.to_string(), value.to_vec());
        }
        Ok(())
    }
}

fn proven(written: Written) -> (KillRecord, usize, bool) {
    match written {
        Written::Proven { kept, records, reset } => (kept, records, reset),
        other => panic!("not proven: {other:?}"),
    }
}

fn kill(pid: u32, reason: u8) -> KillRecord {
    KillRecord {
        seq: 0,
        pid,
        band: BAND_SUSPENDED,
        reason,
        level: MEM_PRESSURE_CRITICAL,
        free: 5_790,
        total: 77_227,
        uptime_ms: 123_456,
    }
}

#[test]
fn a_kill_is_recorded_and_round_trips() {
    let mut journal = Journal::new();
    assert!(journal.is_empty());
    let kept = journal.record(kill(117, REASON_OOM));
    assert_eq!(kept.seq, 1, "the first kill is number 1");
    assert_eq!(kept, KillRecord { seq: 1, ..kill(117, REASON_OOM) });
    let bytes = journal.encode();
    assert_eq!(bytes.len(), HEADER_BYTES + RECORD_BYTES);
    assert_eq!(&bytes[..4], &MAGIC);
    let back = Journal::decode(&bytes).expect("a journal");
    assert_eq!(back, journal);
    assert_eq!(back.last(), Some(kept));
    assert_eq!((reason_name(REASON_OOM), reason_name(REASON_DRILL)), ("oom", "drill"));
}

#[test]
fn the_ring_keeps_the_newest_and_the_numbers_run_on_across_a_reload() {
    let mut journal = Journal::new();
    for pid in 0..(MAX_RECORDS as u32 + 5) {
        journal.record(kill(100 + pid, REASON_DRILL));
    }
    assert_eq!(journal.len(), MAX_RECORDS);
    assert_eq!(journal.records()[0].seq, 6, "the five oldest went");
    assert_eq!(journal.last().map(|r| r.seq), Some(MAX_RECORDS as u32 + 5));
    assert!(journal.encode().len() <= MAX_BYTES);
    // The next boot reads the journal back and goes on counting.
    let mut next_boot = Journal::decode(&journal.encode()).expect("a journal");
    let kept = next_boot.record(kill(90, REASON_OOM));
    assert_eq!(kept.seq, MAX_RECORDS as u32 + 6);
    assert_eq!(next_boot.len(), MAX_RECORDS);
}

#[test]
fn test_reject_a_record_that_is_not_this_journal() {
    let mut journal = Journal::new();
    journal.record(kill(1, REASON_OOM));
    journal.record(kill(2, REASON_OOM));
    let good = journal.encode();
    let mut magic = good.clone();
    magic[0] = b'X';
    assert_eq!(Journal::decode(&magic), None, "another magic");
    let mut version = good.clone();
    version[4] = VERSION + 1;
    assert_eq!(Journal::decode(&version), None, "another layout");
    assert_eq!(Journal::decode(&good[..good.len() - 1]), None, "a record cut short");
    let mut trailing = good.clone();
    trailing.push(0);
    assert_eq!(Journal::decode(&trailing), None, "bytes after the last record");
    let mut count = good.clone();
    count[5] = 3;
    assert_eq!(Journal::decode(&count), None, "a count past the bytes");
    assert_eq!(Journal::decode(&[]), None, "nothing");
}

#[test]
fn test_reject_a_journal_over_the_ring() {
    let mut over = vec![0u8; HEADER_BYTES + (MAX_RECORDS + 1) * RECORD_BYTES];
    over[..4].copy_from_slice(&MAGIC);
    over[4] = VERSION;
    over[5] = (MAX_RECORDS + 1) as u8;
    assert_eq!(Journal::decode(&over), None, "more records than the ring holds");
}

#[test]
fn a_kill_record_names_the_pending_kill() {
    let pending = Pending {
        pid: 41,
        band: BAND_SUSPENDED,
        source: Source::Kernel,
        level: MEM_PRESSURE_CRITICAL,
        free: 4_450,
        total: u64::from(u32::MAX) + 7,
    };
    let record = KillRecord::of(&pending, 61_000);
    assert_eq!(record.reason, REASON_OOM);
    assert_eq!(
        (record.pid, record.band, record.level),
        (41, BAND_SUSPENDED, MEM_PRESSURE_CRITICAL)
    );
    assert_eq!((record.free, record.total, record.uptime_ms), (4_450, u32::MAX, 61_000));
    let drill = KillRecord::of(&Pending { source: Source::Drill, ..pending }, u64::MAX);
    assert_eq!(
        (drill.reason, drill.uptime_ms),
        (REASON_DRILL, u32::MAX),
        "saturated, never wrapped"
    );
}

#[test]
fn the_ledger_writes_reads_back_and_the_next_boot_goes_on() {
    let mut store = MapStore::default();
    let mut ledger = Ledger::new();
    let (kept, records, reset) = proven(ledger.record(kill(117, REASON_DRILL), &mut store));
    assert_eq!((kept.seq, kept.pid, records, reset), (1, 117, 1, false));
    let (kept, records, _) = proven(ledger.record(kill(118, REASON_OOM), &mut store));
    assert_eq!((kept.seq, records), (2, 2));
    // The next boot: a fresh ledger reads the stored journal at its first kill.
    let mut next_boot = Ledger::new();
    let (kept, records, reset) = proven(next_boot.record(kill(9, REASON_OOM), &mut store));
    assert_eq!((kept.seq, kept.pid, records, reset), (3, 9, 3, false));
    let stored = Journal::decode(&store.keys[JOURNAL_KEY]).expect("a journal");
    assert_eq!(stored.records().iter().map(|r| r.pid).collect::<Vec<_>>(), [117, 118, 9]);
}

#[test]
fn a_kill_waits_while_the_store_cannot_be_read() {
    let mut store = MapStore { refuse_get: true, ..MapStore::default() };
    let mut ledger = Ledger::new();
    assert_eq!(ledger.record(kill(5, REASON_OOM), &mut store), Written::ReadRefused);
    assert!(ledger.journal().is_none(), "nothing read, nothing overwritten");
    store.refuse_get = false;
    let (kept, records, _) = proven(ledger.record(kill(6, REASON_OOM), &mut store));
    assert_eq!((kept.seq, kept.pid, records), (2, 6, 2), "the waiting kill took number 1");
    let stored = Journal::decode(&store.keys[JOURNAL_KEY]).expect("a journal");
    assert_eq!(stored.records()[0].pid, 5);
}

#[test]
fn a_refused_write_is_written_with_the_next_kill() {
    let mut store = MapStore { refuse_put: true, ..MapStore::default() };
    let mut ledger = Ledger::new();
    assert_eq!(ledger.record(kill(7, REASON_DRILL), &mut store), Written::WriteRefused);
    assert!(!store.keys.contains_key(JOURNAL_KEY));
    store.refuse_put = false;
    let (kept, records, _) = proven(ledger.record(kill(8, REASON_DRILL), &mut store));
    assert_eq!((kept.seq, records), (2, 2));
}

#[test]
fn test_reject_a_foreign_record_starts_a_fresh_journal() {
    let mut store = MapStore::default();
    store.keys.insert(JOURNAL_KEY.to_string(), b"not a journal".to_vec());
    let mut ledger = Ledger::new();
    let (kept, records, reset) = proven(ledger.record(kill(3, REASON_OOM), &mut store));
    assert_eq!((kept.seq, records, reset), (1, 1, true), "replaced, and said so");
    let (_, _, reset) = proven(ledger.record(kill(4, REASON_OOM), &mut store));
    assert!(!reset, "said once");
}

#[test]
fn test_reject_a_read_back_that_differs() {
    let mut store = MapStore { drop_puts: true, ..MapStore::default() };
    let mut ledger = Ledger::new();
    assert_eq!(ledger.record(kill(1, REASON_OOM), &mut store), Written::ReadBackDiffers);
}

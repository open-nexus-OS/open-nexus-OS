// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: One console LINE at a time across harts — the pure owner logic, kept OUT of the
//! target-gated `hal` so it is host-unit-tested (same rationale as `waitset`/`fence`/
//! `ipc_eof`). `hal::console_line` binds it to the console funnel. Why it exists (TASK-0327B P4
//! H0d): on the four-hart board two console writers interleave BYTE by byte — measured
//! 2026-09-29, init's `init: up <svc>` ladder and `stage: platform` came out woven into a
//! user-fault dump another hart printed (`0i0n0i0t0:0 0u0p0 …`), unreadable to every grep the
//! proof lanes are; QEMU's serialised emulation hid it. The UART mutex cannot be the answer:
//! the trap, fault and panic printers are lock-free on purpose (the mutex may be held on the
//! faulting hart), and they are exactly the writers that tore the ladder.
//!
//! The rule: a hart GATHERS its line (`LineBuf`) and sends it at `\n` as one unit through the
//! GATE (`LineOwner`), which a hart holds only while pure console bytes go out — never while it
//! does anything else, so no kernel lock can couple with the console (the first cut, a hart
//! owning the line from its first byte, did exactly that: a waiter could hold a lock the owner
//! needed; QEMU showed IPC round trips of 275 µs against a 64 µs budget). The gate's escapes
//! keep it deadlock- and stall-free by construction: a hart that already holds it (re-entry)
//! writes through; a gate held longer than the bound is ABANDONED (a hart that died
//! mid-unit) and the waiter takes it over; a waiter whose total wait passes the bound writes
//! through, torn rather than silent. A write-through can never release a gate it does not hold.
//! The word packs the holder's start time with its tag, so "abandoned" is one atomic read.
//! A hart's line is not yet a TASK's line: `TaskLines` (TASK-0333 6f-3) keeps userspace's half
//! lines per task until their `\n`, so a task switched out mid-line keeps its line whole.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the tests below (host); every SMP lane's ladder greps (a torn marker fails
//!   the lane); the board lane (`just board-test`), where the tearing was found

use core::sync::atomic::{AtomicU64, Ordering};

/// The result of asking for the line at a line's first byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    /// The line is ours until `\n`.
    Owned,
    /// The line was held longer than the bound — abandoned mid-line — and is ours now.
    Stolen,
    /// This hart already owned the line (a trap or a task switch mid-line): write through; the
    /// outer line's `\n` releases.
    Reentered,
    /// The line changed hands under us for longer than the bound: write through, torn rather
    /// than silent, and never a wait without end.
    TimedOut,
}

/// The owner word: the low byte is `0` = free or the owning hart's index plus one; the rest is
/// the tick the owner took the line at (its low eight bits dropped — 256 ticks are microseconds
/// on any timebase the tree names). `hart` is the caller's index, `now` its clock in ticks,
/// `bound` the wait AND the abandonment age in the same ticks (`0` = neither: the state
/// before the platform's clock is known).
pub struct LineOwner {
    word: AtomicU64,
}

const TAG_BITS: u32 = 8;
const TAG_MASK: u64 = (1 << TAG_BITS) - 1;

impl LineOwner {
    pub const fn new() -> Self {
        Self { word: AtomicU64::new(0) }
    }

    #[inline]
    fn tag(hart: usize) -> u64 {
        (hart as u64).wrapping_add(1) & TAG_MASK
    }

    #[inline]
    fn pack(now: u64, tag: u64) -> u64 {
        (now & !TAG_MASK) | tag
    }

    #[inline]
    fn since(word: u64) -> u64 {
        word & !TAG_MASK
    }

    /// Whether `hart` owns the line now.
    #[inline]
    pub fn owned_by(&self, hart: usize) -> bool {
        self.word.load(Ordering::Acquire) & TAG_MASK == Self::tag(hart)
    }

    /// Asks for the line at a line's first byte. `now()` is polled while waiting.
    pub fn begin(&self, hart: usize, bound: u64, now: impl Fn() -> u64) -> Entry {
        let me = Self::tag(hart);
        let mut word = self.word.load(Ordering::Acquire);
        if word & TAG_MASK == me {
            return Entry::Reentered;
        }
        let t0 = now();
        loop {
            let t = now();
            let free = word & TAG_MASK == 0;
            let abandoned = !free && t.wrapping_sub(Self::since(word)) > bound;
            if free || abandoned {
                match self.word.compare_exchange_weak(
                    word,
                    Self::pack(t, me),
                    Ordering::AcqRel,
                    Ordering::Acquire,
                ) {
                    Ok(_) => return if free { Entry::Owned } else { Entry::Stolen },
                    Err(seen) => {
                        word = seen;
                        continue;
                    }
                }
            }
            if t.wrapping_sub(t0) > bound {
                return Entry::TimedOut;
            }
            core::hint::spin_loop();
            word = self.word.load(Ordering::Acquire);
        }
    }

    /// Gives the line back at `\n` — only the owner can; anyone else's `\n` is a no-op, so a
    /// write-through never releases a line it does not hold, and a hart whose line was taken
    /// over cannot release the new owner's.
    pub fn end(&self, hart: usize) {
        let me = Self::tag(hart);
        let word = self.word.load(Ordering::Acquire);
        if word & TAG_MASK == me {
            let _ = self.word.compare_exchange(word, 0, Ordering::AcqRel, Ordering::Relaxed);
        }
    }
}

impl Default for LineOwner {
    fn default() -> Self {
        Self::new()
    }
}

/// A hart's line in the making. Bytes gather here and leave as ONE unit at `\n` (or when the
/// buffer is full — a longer line goes out in whole 256-byte pieces), so the gate above is
/// held only while pure UART bytes go out and never while a hart is doing anything else: a
/// waiter can never be waiting on a hart that is itself waiting on a kernel lock. The price is
/// one partial line per hart between its last `\n` and a silent death — the panic path flushes
/// its own hart's line, so a trapped death still ends at its last byte.
pub struct LineBuf {
    buf: [u8; LineBuf::CAP],
    len: usize,
}

impl LineBuf {
    /// The longest unit that leaves in one piece.
    pub const CAP: usize = 256;

    pub const fn new() -> Self {
        Self { buf: [0; Self::CAP], len: 0 }
    }

    /// Appends `byte`; `true` when the unit is complete (`\n`, or the buffer is full) and
    /// `take` must be called before the next push.
    #[inline]
    pub fn push(&mut self, byte: u8) -> bool {
        self.buf[self.len] = byte;
        self.len += 1;
        byte == b'\n' || self.len == Self::CAP
    }

    /// The gathered bytes, and the buffer is empty again.
    #[inline]
    pub fn take(&mut self) -> &[u8] {
        let n = self.len;
        self.len = 0;
        &self.buf[..n]
    }

    /// Whether anything is gathered.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl Default for LineBuf {
    fn default() -> Self {
        Self::new()
    }
}

/// Partial lines of userspace tasks kept apart at one moment (TASK-0333 6f-3): the tasks that
/// are mid-line now, not the tasks that exist.
pub const TASK_LINES: usize = 16;

/// Userspace's console bytes, gathered per TASK before they reach a hart's [`LineBuf`]
/// (TASK-0333 6f-3). A hart gathers whatever runs on it, and a hart runs many tasks: a task that
/// wrote its line in pieces (byte by byte, or several writes) and was switched out mid-line had
/// the next task's whole line appended to its first half — on the lowmem lane `abilitymgr: `
/// then `execd: exit pid=117 …` as one line, `kill (inst=3 …)` as the next, and a marker the
/// ladder never found. So a task's bytes wait here until its `\n` and enter the hart funnel as
/// ONE line, written in one go from one syscall. A line that outgrows [`LineBuf::CAP`] leaves
/// in whole pieces; a task that finds no free slot writes through — torn rather than silent,
/// never a wait; a task that dies mid-line has its piece ended at the death ([`Self::exited`]).
/// Pure and host-tested; `hal::console_line` holds the one table behind an IRQ-safe lock and
/// sends each unit with the lock released.
pub struct TaskLines {
    slots: [TaskLine; TASK_LINES],
}

#[derive(Clone, Copy)]
struct TaskLine {
    /// The task's id plus one; `0` = free.
    owner: u64,
    len: usize,
    buf: [u8; LineBuf::CAP],
}

impl TaskLine {
    const FREE: Self = Self { owner: 0, len: 0, buf: [0; LineBuf::CAP] };
}

impl TaskLines {
    pub const fn new() -> Self {
        Self { slots: [TaskLine::FREE; TASK_LINES] }
    }

    /// Feeds `bytes` that `task` wrote. Answers `(consumed, unit)`: how many of `bytes` were
    /// taken, and how many bytes of `out` to send NOW — a whole line ending in `\n`, a full
    /// piece of a longer one, or (no slot free) the bytes themselves; `0` = nothing complete
    /// yet. The caller loops until `bytes` is consumed, sending each unit outside its lock.
    /// Every call with bytes consumes at least one.
    pub fn feed(
        &mut self,
        task: u32,
        bytes: &[u8],
        out: &mut [u8; LineBuf::CAP],
    ) -> (usize, usize) {
        if bytes.is_empty() {
            return (0, 0);
        }
        let key = u64::from(task) + 1;
        let slot = match self.slots.iter().position(|s| s.owner == key) {
            Some(i) => i,
            None => {
                // Nothing pending: a line that ends within the buffer goes out at once.
                if let Some(nl) = bytes.iter().take(LineBuf::CAP).position(|b| *b == b'\n') {
                    out[..=nl].copy_from_slice(&bytes[..=nl]);
                    return (nl + 1, nl + 1);
                }
                match self.slots.iter().position(|s| s.owner == 0) {
                    Some(free) => {
                        self.slots[free].owner = key;
                        self.slots[free].len = 0;
                        free
                    }
                    None => {
                        // Every slot is some task's half line: through, never a wait.
                        let n = bytes.len().min(LineBuf::CAP);
                        out[..n].copy_from_slice(&bytes[..n]);
                        return (n, n);
                    }
                }
            }
        };
        let line = &mut self.slots[slot];
        for (i, &b) in bytes.iter().enumerate() {
            line.buf[line.len] = b;
            line.len += 1;
            if b == b'\n' || line.len == LineBuf::CAP {
                let n = line.len;
                out[..n].copy_from_slice(&line.buf[..n]);
                line.len = 0;
                if b == b'\n' {
                    line.owner = 0;
                }
                return (i + 1, n);
            }
        }
        (bytes.len(), 0)
    }

    /// `task` is gone: its half line (if any), ended with `\n`, as the unit to send; the slot
    /// is free again.
    pub fn exited(&mut self, task: u32, out: &mut [u8; LineBuf::CAP]) -> usize {
        let key = u64::from(task) + 1;
        let Some(line) = self.slots.iter_mut().find(|s| s.owner == key) else {
            return 0;
        };
        // A piece leaves at `CAP`, so a half line holds at most `CAP - 1`: its `\n` fits.
        let n = line.len;
        out[..n].copy_from_slice(&line.buf[..n]);
        line.owner = 0;
        line.len = 0;
        if n == 0 {
            return 0;
        }
        out[n] = b'\n';
        n + 1
    }

    /// Tasks mid-line now.
    pub fn pending(&self) -> usize {
        self.slots.iter().filter(|s| s.owner != 0).count()
    }
}

impl Default for TaskLines {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod task_line_tests {
    use super::*;

    /// Feeds `bytes` as `task` until consumed; the units sent, in order.
    fn feed_all(lines: &mut TaskLines, task: u32, mut bytes: &[u8], sent: &mut Vec<Vec<u8>>) {
        let mut out = [0u8; LineBuf::CAP];
        while !bytes.is_empty() {
            let (used, n) = lines.feed(task, bytes, &mut out);
            assert!(used > 0, "every call takes a byte");
            if n > 0 {
                sent.push(out[..n].to_vec());
            }
            bytes = &bytes[used..];
        }
    }

    /// The lowmem lane's tear, replayed: abilitymgr writes its line byte by byte and is
    /// switched out after `abilitymgr: `; execd writes its whole line; abilitymgr resumes.
    /// Each line leaves whole, in the order its `\n` came.
    #[test]
    fn a_task_switched_out_mid_line_keeps_its_line() {
        let mut lines = TaskLines::new();
        let mut sent = Vec::new();
        for &b in b"abilitymgr: " {
            feed_all(&mut lines, 7, &[b], &mut sent);
        }
        assert!(sent.is_empty(), "half a line waits");
        feed_all(&mut lines, 5, b"execd: exit pid=117 reason=killed code=-9\n", &mut sent);
        for &b in b"kill (inst=3 reason=memory)\n" {
            feed_all(&mut lines, 7, &[b], &mut sent);
        }
        assert_eq!(
            sent,
            [
                b"execd: exit pid=117 reason=killed code=-9\n".to_vec(),
                b"abilitymgr: kill (inst=3 reason=memory)\n".to_vec(),
            ]
        );
        assert_eq!(lines.pending(), 0, "both slots free again");
    }

    /// One write holding several lines leaves as several units; the tail waits for its `\n`.
    #[test]
    fn several_lines_in_one_write_leave_one_by_one() {
        let mut lines = TaskLines::new();
        let mut sent = Vec::new();
        feed_all(&mut lines, 1, b"a: one\nb: two\nc: thr", &mut sent);
        assert_eq!(sent, [b"a: one\n".to_vec(), b"b: two\n".to_vec()]);
        feed_all(&mut lines, 1, b"ee\n", &mut sent);
        assert_eq!(sent.last().map(Vec::as_slice), Some(&b"c: three\n"[..]));
    }

    /// A line longer than the buffer leaves in whole pieces — never a byte lost — and the
    /// task keeps its slot until the `\n`.
    #[test]
    fn test_reject_overflowing_a_task_line() {
        let mut lines = TaskLines::new();
        let mut sent = Vec::new();
        let long: Vec<u8> = (0..(LineBuf::CAP * 2 + 9)).map(|i| b'a' + (i % 26) as u8).collect();
        feed_all(&mut lines, 3, &long, &mut sent);
        assert_eq!(sent.len(), 2, "two full pieces out");
        assert!(sent.iter().all(|u| u.len() == LineBuf::CAP));
        assert_eq!(lines.pending(), 1);
        feed_all(&mut lines, 3, b"\n", &mut sent);
        let all: Vec<u8> = sent.concat();
        assert_eq!(&all[..long.len()], &long[..]);
        assert_eq!(all.last(), Some(&b'\n'));
        assert_eq!(lines.pending(), 0);
    }

    /// With every slot holding some task's half line, one more task writes through — torn
    /// rather than silent, and never a wait.
    #[test]
    fn test_reject_waiting_when_every_slot_is_taken() {
        let mut lines = TaskLines::new();
        let mut sent = Vec::new();
        for task in 0..TASK_LINES as u32 {
            feed_all(&mut lines, task, b"half", &mut sent);
        }
        assert_eq!(lines.pending(), TASK_LINES);
        assert!(sent.is_empty());
        feed_all(&mut lines, 99, b"late", &mut sent);
        assert_eq!(sent, [b"late".to_vec()]);
        // A whole line needs no slot at all.
        feed_all(&mut lines, 100, b"whole\n", &mut sent);
        assert_eq!(sent.last().map(Vec::as_slice), Some(&b"whole\n"[..]));
    }

    /// A task that dies mid-line has its piece ended at the death; a task with nothing
    /// pending sends nothing; the slot serves the next task.
    #[test]
    fn a_task_that_dies_mid_line_has_its_piece_ended() {
        let mut lines = TaskLines::new();
        let mut sent = Vec::new();
        feed_all(&mut lines, 4, b"memd: half", &mut sent);
        let mut out = [0u8; LineBuf::CAP];
        let n = lines.exited(4, &mut out);
        assert_eq!(&out[..n], b"memd: half\n");
        assert_eq!(lines.exited(4, &mut out), 0, "nothing twice");
        assert_eq!(lines.exited(9, &mut out), 0, "nothing for a task that wrote nothing");
        assert_eq!(lines.pending(), 0);
        // The longest half line still fits its `\n`.
        feed_all(&mut lines, 4, &[b'x'; LineBuf::CAP - 1], &mut sent);
        assert_eq!(lines.exited(4, &mut out), LineBuf::CAP);
        assert_eq!(out[LineBuf::CAP - 1], b'\n');
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn a_line_leaves_whole_at_newline() {
        let mut line = LineBuf::new();
        for &b in b"init: up blkd" {
            assert!(!line.push(b));
        }
        assert!(line.push(b'\n'));
        assert_eq!(line.take(), b"init: up blkd\n");
        assert!(line.is_empty());
        assert!(!line.push(b'x'));
        assert_eq!(line.take(), b"x");
    }

    /// A line longer than the buffer leaves in whole pieces — never a byte lost, never an
    /// index past the end.
    #[test]
    fn test_reject_overflowing_the_buffer() {
        let mut line = LineBuf::new();
        let mut out = std::vec::Vec::new();
        for i in 0..(LineBuf::CAP * 2 + 7) {
            let b = b'a' + (i % 26) as u8;
            if line.push(b) {
                out.extend_from_slice(line.take());
            }
        }
        assert!(line.push(b'\n'));
        out.extend_from_slice(line.take());
        assert_eq!(out.len(), LineBuf::CAP * 2 + 8);
        assert_eq!(out[LineBuf::CAP - 1], b'a' + ((LineBuf::CAP - 1) % 26) as u8);
        assert_eq!(*out.last().unwrap_or(&0), b'\n');
    }

    /// One tick of the tests' clock: the word drops the low eight bits of a tick, so the
    /// tests count in units the word keeps whole.
    const T: u64 = 256;

    /// A clock that advances `step` per poll, starting at `start`.
    fn clock(start: u64, step: u64) -> impl Fn() -> u64 {
        let t = Cell::new(start);
        move || {
            let v = t.get();
            t.set(v + step);
            v
        }
    }

    #[test]
    fn a_free_line_is_owned_until_newline() {
        let owner = LineOwner::new();
        assert_eq!(owner.begin(0, 10 * T, clock(1 << 20, T)), Entry::Owned);
        assert!(owner.owned_by(0));
        assert!(!owner.owned_by(1));
        owner.end(0);
        assert!(!owner.owned_by(0));
        assert_eq!(owner.begin(1, 10 * T, clock(1 << 20, T)), Entry::Owned);
    }

    /// The same hart mid-line (a trap, a task switch) writes through and never waits.
    #[test]
    fn the_owning_hart_reenters_without_waiting() {
        let owner = LineOwner::new();
        assert_eq!(owner.begin(2, 10 * T, clock(1 << 20, T)), Entry::Owned);
        let polls = Cell::new(0u64);
        let entry = owner.begin(2, 10 * T, || {
            polls.set(polls.get() + 1);
            (1 << 20) + polls.get() * T
        });
        assert_eq!(entry, Entry::Reentered);
        assert_eq!(polls.get(), 0, "a re-entry must not poll the clock at all");
    }

    /// A line held longer than the bound is abandoned: the waiter takes it over after waiting
    /// out the bound, and the old owner can no longer release it.
    #[test]
    fn an_abandoned_line_is_taken_over_after_the_bound() {
        let owner = LineOwner::new();
        assert_eq!(owner.begin(0, 10 * T, clock(1 << 20, T)), Entry::Owned);
        let polls = Cell::new(0u64);
        let entry = owner.begin(1, 10 * T, || {
            polls.set(polls.get() + 1);
            (1 << 20) + polls.get() * T
        });
        assert_eq!(entry, Entry::Stolen);
        assert!((10..=14).contains(&polls.get()), "polls={}", polls.get());
        assert!(owner.owned_by(1));
        assert!(!owner.owned_by(0));
        owner.end(0);
        assert!(owner.owned_by(1), "the old owner must not release the new owner's line");
        owner.end(1);
        assert!(!owner.owned_by(1));
    }

    /// Within the bound the waiter waits and the owner keeps the line (nothing is stolen
    /// early); the moment the owner's `\n` frees it, the waiter has it.
    #[test]
    fn test_reject_stealing_a_live_line() {
        let owner = LineOwner::new();
        assert_eq!(owner.begin(0, 100 * T, clock(1 << 20, T)), Entry::Owned);
        let polls = Cell::new(0u64);
        let entry = owner.begin(1, 100 * T, || {
            let n = polls.get() + 1;
            polls.set(n);
            assert!(owner.owned_by(0) || n > 5, "stolen before the owner let go");
            if n == 5 {
                owner.end(0);
            }
            (1 << 20) + n * T
        });
        assert_eq!(entry, Entry::Owned);
        assert!(owner.owned_by(1));
        assert!(polls.get() < 20, "polls={}", polls.get());
    }

    /// Before the platform's clock is known the bound is 0: a free line is taken, a held line
    /// counts as abandoned at once — nothing ever waits there (the single-hart early boot).
    #[test]
    fn a_zero_bound_never_waits() {
        let owner = LineOwner::new();
        assert_eq!(owner.begin(0, 0, clock(1 << 20, T)), Entry::Owned);
        let polls = Cell::new(0u64);
        let entry = owner.begin(1, 0, || {
            polls.set(polls.get() + 1);
            (1 << 20) + polls.get() * T
        });
        assert_eq!(entry, Entry::Stolen);
        assert!(polls.get() <= 2, "polls={}", polls.get());
    }

    /// A hart that wrote through must not release the line it never held.
    #[test]
    fn test_reject_release_by_a_non_owner() {
        let owner = LineOwner::new();
        assert_eq!(owner.begin(0, 10 * T, clock(1 << 20, T)), Entry::Owned);
        owner.end(1);
        assert!(owner.owned_by(0));
        owner.end(0);
        assert!(!owner.owned_by(0));
        // A release on a free line is a no-op too.
        owner.end(0);
        assert_eq!(owner.begin(3, 10 * T, clock(1 << 20, T)), Entry::Owned);
    }

    /// The hart index lives in the low byte, the start tick above it: neither corrupts the
    /// other for any hart the kernel can have.
    #[test]
    fn the_word_keeps_hart_and_time_apart() {
        let owner = LineOwner::new();
        for hart in [0usize, 1, 3, 7, 254] {
            assert_eq!(owner.begin(hart, 10, clock(u64::MAX - 4096 * T, T)), Entry::Owned);
            assert!(owner.owned_by(hart));
            assert!(!owner.owned_by(hart + 1));
            owner.end(hart);
        }
    }
}

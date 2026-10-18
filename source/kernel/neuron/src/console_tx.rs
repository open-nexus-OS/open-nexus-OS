// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The serial line's place in the console ring (TASK-0330 S2b): the UART reads
//! RFC-0107's ring itself, from a cursor of its own, so a console write costs a write into the
//! ring and never the serial time. Measured on the board (`board--2026-10-17T21-48-46`): the
//! 8 KiB queue this replaces ran full in every boot's bring-up (`full=1139` pushes there), and a
//! write into a full queue waited for the line with its hart's interrupts off — the console
//! syscalls under the kernel lock (the bring-up's 20 ms holds, `nr=44`), a crash report's 1540
//! bytes about 134 ms on its hart. The ring holds a whole boot's console (about 121 KB on the
//! board), so the line falls behind and catches up without losing a byte. Pure and bounded, NOT
//! target-gated, so the order and the lap rule run on the host; `hal::console_serial` owns the
//! UART and the lock.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: host (`tests` below)
//! INVARIANTS: the cursor never passes the head; bytes leave in the ring's order; a cursor more
//!   than the ring less [`LAP_MARGIN`] behind skips to the oldest byte still kept and counts what
//!   it skipped; a burst the writers reached while it was read is not sent.

/// Bytes a burst carries at most: the 16550's FIFO, the smallest in the family.
pub const FIFO_BURST: usize = 16;

/// How near the writers' lap the line reads at most: a byte closer to being overwritten is
/// skipped, not read. Each writer has at most one byte between its write and the head's store,
/// so a burst read this far behind the lap is the writers' bytes whole.
pub const LAP_MARGIN: u64 = 4096;

/// Room for the line the serial line sends where it skipped.
pub const NOTE_MAX: usize = 80;

const NOTE_HEAD: &[u8] = b"\n[serial: ";
const NOTE_TAIL: &[u8] = b" bytes skipped, the boot trace keeps them]\n";
// The longest count, `u64::MAX`, has twenty digits.
const _: () = assert!(NOTE_HEAD.len() + 20 + NOTE_TAIL.len() <= NOTE_MAX);

/// The serial line's cursor: the ring count of the next byte it sends.
pub struct Cursor {
    next: u64,
}

/// What the serial line sends next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Burst {
    /// Bytes the writers overwrote before the line reached them, skipped.
    pub lapped: u64,
    /// The ring count of the burst's first byte.
    pub from: u64,
    /// Bytes in the burst.
    pub len: usize,
}

impl Cursor {
    pub const fn new() -> Self {
        Self { next: 0 }
    }

    /// Bytes the line is behind the writers.
    pub const fn behind(&self, head: u64) -> u64 {
        head.saturating_sub(self.next)
    }

    /// Everything written up to `head` counts as sent: until the line reads the ring, the
    /// console writes each byte to the transmitter itself.
    pub fn follow(&mut self, head: u64) {
        self.next = self.next.max(head);
    }

    /// The next burst of at most `max` bytes up to `head`, in a ring of `capacity` bytes — past
    /// what the writers lapped, first.
    pub fn plan(&mut self, head: u64, capacity: u64, max: usize) -> Burst {
        let keep = capacity.saturating_sub(LAP_MARGIN);
        let mut lapped = 0;
        if head.saturating_sub(self.next) > keep {
            let oldest = head - keep;
            lapped = oldest - self.next;
            self.next = oldest;
        }
        let len = head.saturating_sub(self.next).min(max as u64) as usize;
        Burst { lapped, from: self.next, len }
    }

    /// The burst was read: the cursor moves past it. `true` when its bytes are the writers' —
    /// `false` when the head, read again after the copy, shows that the writers came within half
    /// the margin of its first byte meanwhile (the caller sends none of it and counts it).
    pub fn commit(&mut self, burst: Burst, head_after: u64, capacity: u64) -> bool {
        self.next = burst.from + burst.len as u64;
        head_after.saturating_sub(burst.from) <= capacity.saturating_sub(LAP_MARGIN / 2)
    }
}

impl Default for Cursor {
    fn default() -> Self {
        Self::new()
    }
}

/// The line the serial line sends where it skipped `lapped` bytes; returns its length.
pub fn lap_note(lapped: u64, out: &mut [u8; NOTE_MAX]) -> usize {
    let mut digits = [0u8; 20];
    let mut n = lapped;
    let mut count = 0;
    loop {
        digits[count] = b'0' + (n % 10) as u8;
        count += 1;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    let mut at = 0;
    for &b in NOTE_HEAD.iter().chain(digits[..count].iter().rev()).chain(NOTE_TAIL) {
        out[at] = b;
        at += 1;
    }
    at
}

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    use super::*;

    /// A ring as the kernel keeps it: every byte at its count modulo the capacity.
    struct Ring {
        data: Vec<u8>,
        head: u64,
    }

    impl Ring {
        fn new(capacity: usize) -> Self {
            Self { data: alloc::vec![0; capacity], head: 0 }
        }

        fn capacity(&self) -> u64 {
            self.data.len() as u64
        }

        fn write(&mut self, bytes: &[u8]) {
            for &b in bytes {
                let at = (self.head % self.capacity()) as usize;
                self.data[at] = b;
                self.head += 1;
            }
        }

        fn at(&self, count: u64) -> u8 {
            self.data[(count % self.capacity()) as usize]
        }
    }

    /// One burst as the line sends it: its bytes, or none when the writers reached them.
    fn send(cursor: &mut Cursor, ring: &Ring, out: &mut Vec<u8>) -> Burst {
        let burst = cursor.plan(ring.head, ring.capacity(), FIFO_BURST);
        let bytes: Vec<u8> = (0..burst.len as u64).map(|i| ring.at(burst.from + i)).collect();
        if cursor.commit(burst, ring.head, ring.capacity()) {
            out.extend_from_slice(&bytes);
        }
        burst
    }

    const CAPACITY: usize = 32 * 1024;

    #[test]
    fn bytes_leave_in_the_ring_order_a_burst_at_a_time() {
        let mut ring = Ring::new(CAPACITY);
        let mut cursor = Cursor::new();
        let (mut out, mut written) = (Vec::new(), Vec::new());
        // The writers run thousands of bytes ahead of the line; the line catches up.
        for line in 0..400u32 {
            let text = alloc::format!("SELFTEST: line {line} of the bring-up burst\n");
            ring.write(text.as_bytes());
            written.extend_from_slice(text.as_bytes());
            if line % 3 == 0 {
                let _ = send(&mut cursor, &ring, &mut out);
            }
        }
        assert!(written.len() > 8192, "more than the old queue held");
        while cursor.behind(ring.head) > 0 {
            assert_eq!(send(&mut cursor, &ring, &mut out).lapped, 0);
        }
        assert_eq!(out, written);
    }

    #[test]
    fn test_reject_a_burst_past_the_head() {
        let mut ring = Ring::new(CAPACITY);
        let mut cursor = Cursor::new();
        ring.write(b"abc");
        let burst = cursor.plan(ring.head, ring.capacity(), FIFO_BURST);
        assert_eq!(burst, Burst { lapped: 0, from: 0, len: 3 });
        assert!(cursor.commit(burst, ring.head, ring.capacity()));
        assert_eq!(cursor.plan(ring.head, ring.capacity(), FIFO_BURST).len, 0);
        assert_eq!(cursor.behind(ring.head), 0);
    }

    #[test]
    fn a_line_the_writers_lapped_skips_to_the_oldest_byte_kept() {
        let mut ring = Ring::new(CAPACITY);
        let mut cursor = Cursor::new();
        let total = CAPACITY as u64 + 1000;
        for i in 0..total {
            ring.write(&[(i % 251) as u8]);
        }
        let keep = ring.capacity() - LAP_MARGIN;
        let burst = cursor.plan(ring.head, ring.capacity(), FIFO_BURST);
        assert_eq!(burst, Burst { lapped: total - keep, from: total - keep, len: FIFO_BURST });
        assert!(cursor.commit(burst, ring.head, ring.capacity()));
        for i in 0..burst.len as u64 {
            assert_eq!(ring.at(burst.from + i), ((burst.from + i) % 251) as u8);
        }
    }

    #[test]
    fn test_reject_a_burst_the_writers_reached_while_it_was_read() {
        let capacity = CAPACITY as u64;
        let head = capacity - LAP_MARGIN;
        let mut cursor = Cursor::new();
        let burst = cursor.plan(head, capacity, FIFO_BURST);
        assert_eq!(burst, Burst { lapped: 0, from: 0, len: FIFO_BURST }, "at the margin: read");
        let reached = capacity - LAP_MARGIN / 2 + 1;
        assert!(!cursor.commit(burst, reached, capacity), "the writers came too near: not sent");
        assert_eq!(cursor.behind(reached), reached - FIFO_BURST as u64, "past it all the same");

        let mut cursor = Cursor::new();
        let burst = cursor.plan(head, capacity, FIFO_BURST);
        assert!(cursor.commit(burst, capacity - LAP_MARGIN / 2, capacity), "at the limit: sent");
    }

    #[test]
    fn follow_counts_the_synchronous_bytes_as_sent() {
        let mut ring = Ring::new(CAPACITY);
        let mut cursor = Cursor::new();
        ring.write(b"early bytes, written to the transmitter as they came\n");
        cursor.follow(ring.head);
        assert_eq!(cursor.behind(ring.head), 0);
        ring.write(b"KINIT: console serial\n");
        let mut out = Vec::new();
        while cursor.behind(ring.head) > 0 {
            let _ = send(&mut cursor, &ring, &mut out);
        }
        assert_eq!(out, b"KINIT: console serial\n");
        cursor.follow(3);
        assert_eq!(cursor.behind(ring.head), 0, "an older head never moves the cursor back");
    }

    #[test]
    fn the_lap_note_names_the_bytes_skipped() {
        let mut out = [0u8; NOTE_MAX];
        let n = lap_note(12345, &mut out);
        assert_eq!(&out[..n], b"\n[serial: 12345 bytes skipped, the boot trace keeps them]\n");
        let n = lap_note(0, &mut out);
        assert_eq!(&out[..n], b"\n[serial: 0 bytes skipped, the boot trace keeps them]\n");
        let n = lap_note(u64::MAX, &mut out);
        assert!(out[..n].starts_with(b"\n[serial: 18446744073709551615 bytes skipped"));
    }
}

// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: recv-wake-probe — the #102-family regression gate child
//! (TASK-0080D / closure-plan P0.2). execd spawns it once after `ready`,
//! grants two one-way endpoint halves BEFORE resume, and drives a strict
//! handshake: the child arms, PARKS in a plain BLOCKING ipc recv, and must
//! be woken by execd's send. The proof boot manual--2026-07-07T12-12-27
//! showed exec'd children parking in blocking recv forever while messages
//! sat in their queue — this probe reproduces (or proves fixed) that class
//! deterministically at every boot, headless, with no user interaction.
//! TASK-0333 6f-1 (ADR-0087): before its third park the child writes a pattern
//! into its heap — text, zeroes, noise — and execd squeezes it while it is
//! frozen; after the thaw the child checks every byte and answers `woke` only
//! when the pattern came back whole (`squeezed wrong` otherwise).
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: proven via QEMU markers (`execd: recv-wake probe …` +
//! `SELFTEST: exec child blocking recv wake ok`); no host surface (the
//! whole point is the OS scheduler/wake path).

#![forbid(unsafe_code)]
#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std, no_main)]

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn os_entry() -> Result<(), &'static str> {
    probe::run()
}

#[cfg(nexus_env = "host")]
fn main() {
    println!("recv-wake-probe: host mode - the probe runs on the OS (QEMU markers)");
}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
extern crate alloc;

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
mod probe {
    use nexus_abi::{ipc_recv_v1, ipc_send_v1, MsgHeader, IPC_SYS_TRUNCATE};

    /// The child's capability slots, from the ONE declaration execd grants against
    /// (TASK-0324 P4e-2 — they used to be copied here with a comment naming execd's
    /// copy, and execd's comment naming this one). execd `cap_transfer_to_slot`s
    /// them BEFORE `task_resume` (grants-before-resume discipline), so they are
    /// valid from the first instruction: the RECV half of the ping endpoint (execd
    /// holds SEND) and the SEND half of the reply endpoint (execd holds RECV). Two
    /// one-way endpoints — a single shared queue would let execd's reply-wait steal
    /// the ping it just sent.
    const PING_RECV_SLOT: u32 = nexus_service_topology::slots::recv_wake_probe::PING_RECV;
    const REPLY_SEND_SLOT: u32 = nexus_service_topology::slots::recv_wake_probe::REPLY_SEND;

    /// Wire bytes (one-byte protocol, values arbitrary but pinned):
    /// armed → child is about to park; woke → the blocking recv returned.
    const MSG_ARMED: u8 = 0xA1;
    const MSG_WOKE: u8 = 0xA2;
    /// After the thaw: the pattern did not come back as written (TASK-0333 6f-1).
    const MSG_SQUEEZED_WRONG: u8 = 0xA3;

    /// The pattern the squeeze must give back (ADR-0087): pages of text (they compress), of
    /// zeroes written once (dropped, not recorded), of noise (kept as they are).
    const PAGE: usize = 4096;
    const TEXT_PAGES: usize = 8;
    const ZERO_PAGES: usize = 2;
    const NOISE_PAGES: usize = 2;
    const PATTERN_BYTES: usize = (TEXT_PAGES + ZERO_PAGES + NOISE_PAGES) * PAGE;

    /// The byte at `at` of the pattern.
    fn expected(at: usize) -> u8 {
        const PHRASE: &[u8] = b"a frozen child is squeezed and every page comes back as it was. ";
        let page = at / PAGE;
        if page < TEXT_PAGES {
            PHRASE[(at + page * 7) % PHRASE.len()]
        } else if page < TEXT_PAGES + ZERO_PAGES {
            0
        } else {
            // A hash per byte: no four bytes repeat, the page does not compress.
            let mut x = (at as u64) | (1 << 40);
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 56) as u8
        }
    }

    /// The pattern, in the heap (lazy `.bss`): the zero pages written once so they are committed.
    fn write_pattern() -> alloc::vec::Vec<u8> {
        let mut memory = alloc::vec![0u8; PATTERN_BYTES];
        let zeroes = TEXT_PAGES * PAGE..(TEXT_PAGES + ZERO_PAGES) * PAGE;
        for (at, byte) in memory.iter_mut().enumerate() {
            *byte = if zeroes.contains(&at) { 0x5A } else { expected(at) };
        }
        memory[zeroes].fill(0);
        memory
    }

    /// Probe markers must NOT fold: `nexus-service-entry` arms verdict
    /// folding for every process it bootstraps, so `debug_println` swallows
    /// non-FAIL lines in interactive boots. Raw write, like app-host.
    fn raw_marker(line: &str) {
        let mut buf = [0u8; 96];
        let bytes = line.as_bytes();
        let n = bytes.len().min(buf.len() - 1);
        buf[..n].copy_from_slice(&bytes[..n]);
        buf[n] = b'\n';
        let _ = nexus_abi::debug_write(&buf[..n + 1]);
    }

    /// Bounded non-blocking send (2s wall budget): the probe must never park
    /// in SEND — the class under test is the RECV park. QueueFull only ever
    /// means execd hasn't drained yet; anything else is a wiring failure.
    /// One waited send (TASK-0324 P7-d): queue space, or the spawner's death.
    fn send_byte(slot: u32, byte: u8) -> Result<(), &'static str> {
        let payload = [byte];
        let hdr = MsgHeader::new(0, 0, 0, 0, payload.len() as u32);
        ipc_send_v1(slot, &hdr, &payload, 0, 0).map(|_| ()).map_err(|_| "send failed")
    }

    pub fn run() -> Result<(), &'static str> {
        // 1. Tell execd we are armed. After this send the ONLY thing between
        //    us and the park is the recv syscall itself — execd waits a beat
        //    before pinging so the park is (near-)certain to have happened.
        if let Err(e) = send_byte(REPLY_SEND_SLOT, MSG_ARMED) {
            raw_marker("RECVWAKE: FAIL armed send");
            return Err(e);
        }

        // 2. THE TEST — THREE full park/wake cycles. Cycle 1 alone passed while
        //    the real app still hung on its SECOND park (counter repro
        //    2026-07-07: tap 1 processed, tap 2 delivered but the child never
        //    ran again, not blocked, no failed wake — the wake-then-lost
        //    class). A regression gate that only proves the first wake is
        //    half a gate. The THIRD park is where execd freezes this process
        //    (ADR-0082): the ping sent meanwhile waits in the queue, and the
        //    reply after the thaw is the proof the thaw ran it again.
        let mut hdr = MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 16];
        let memory = write_pattern();
        for cycle in 0..3u32 {
            match ipc_recv_v1(PING_RECV_SLOT, &mut hdr, &mut buf, IPC_SYS_TRUNCATE, 0) {
                Ok(_) => {}
                Err(_) => {
                    raw_marker(match cycle {
                        0 => "RECVWAKE: FAIL blocking recv errored (cycle 1)",
                        1 => "RECVWAKE: FAIL blocking recv errored (cycle 2)",
                        _ => "RECVWAKE: FAIL blocking recv errored (cycle 3)",
                    });
                    return Err("recv failed");
                }
            }
            // Round-trip proof per cycle: the reply is measured evidence the
            // child actually RAN again after the wake. After the third (the thaw),
            // it also says whether the squeezed pattern came back whole.
            let intact =
                cycle < 2 || memory.iter().enumerate().all(|(at, &byte)| byte == expected(at));
            if !intact {
                raw_marker("RECVWAKE: FAIL the squeezed pattern came back wrong");
            }
            let answer = if intact { MSG_WOKE } else { MSG_SQUEEZED_WRONG };
            if let Err(e) = send_byte(REPLY_SEND_SLOT, answer) {
                raw_marker("RECVWAKE: FAIL woke send");
                return Err(e);
            }
            raw_marker(match cycle {
                0 => "RECVWAKE: woke from blocking recv",
                1 => "RECVWAKE: woke twice (park/wake cycle repeats)",
                _ => "RECVWAKE: woke after thaw (frozen ping delivered)",
            });
        }
        // 3. THE END IS AN ORDER (ADR-0082): park in a receive nobody will ever
        //    serve; execd's `task_kill` ends this process. A return here would be a
        //    voluntary exit and the kill proof would have nothing to kill.
        let _ = ipc_recv_v1(PING_RECV_SLOT, &mut hdr, &mut buf, IPC_SYS_TRUNCATE, 0);
        raw_marker("RECVWAKE: FAIL survived the kill");
        Err("survived the kill")
    }
}

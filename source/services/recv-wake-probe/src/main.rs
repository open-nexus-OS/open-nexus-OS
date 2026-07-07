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
mod probe {
    use nexus_abi::{ipc_recv_v1, ipc_send_v1, nsec, yield_, MsgHeader, IPC_SYS_NONBLOCK,
        IPC_SYS_TRUNCATE};

    /// Fixed child capability slots — execd `cap_transfer_to_slot`s these
    /// BEFORE `task_resume` (grants-before-resume discipline), so they are
    /// valid from the first instruction. Slot 5 = RECV half of the ping
    /// endpoint (execd holds SEND); slot 6 = SEND half of the reply endpoint
    /// (execd holds RECV). Two one-way endpoints — a single shared queue
    /// would let execd's reply-wait steal the ping it just sent.
    const PING_RECV_SLOT: u32 = 5;
    const REPLY_SEND_SLOT: u32 = 6;

    /// Wire bytes (one-byte protocol, values arbitrary but pinned):
    /// armed → child is about to park; woke → the blocking recv returned.
    const MSG_ARMED: u8 = 0xA1;
    const MSG_WOKE: u8 = 0xA2;

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
    fn send_byte(slot: u32, byte: u8) -> Result<(), &'static str> {
        let payload = [byte];
        let hdr = MsgHeader::new(0, 0, 0, 0, payload.len() as u32);
        let deadline = nsec().unwrap_or(0).saturating_add(2_000_000_000);
        loop {
            match ipc_send_v1(slot, &hdr, &payload, IPC_SYS_NONBLOCK, 0) {
                Ok(_) => return Ok(()),
                Err(nexus_abi::IpcError::QueueFull) => {
                    if nsec().unwrap_or(u64::MAX) >= deadline {
                        return Err("send timeout");
                    }
                    let _ = yield_();
                }
                Err(_) => return Err("send failed"),
            }
        }
    }

    pub fn run() -> Result<(), &'static str> {
        // 1. Tell execd we are armed. After this send the ONLY thing between
        //    us and the park is the recv syscall itself — execd waits a beat
        //    before pinging so the park is (near-)certain to have happened.
        if let Err(e) = send_byte(REPLY_SEND_SLOT, MSG_ARMED) {
            raw_marker("RECVWAKE: FAIL armed send");
            return Err(e);
        }

        // 2. THE TEST — TWO full park/wake cycles. Cycle 1 alone passed while
        //    the real app still hung on its SECOND park (counter repro
        //    2026-07-07: tap 1 processed, tap 2 delivered but the child never
        //    ran again, not blocked, no failed wake — the wake-then-lost
        //    class). A regression gate that only proves the first wake is
        //    half a gate.
        let mut hdr = MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 16];
        for cycle in 0..2u32 {
            match ipc_recv_v1(PING_RECV_SLOT, &mut hdr, &mut buf, IPC_SYS_TRUNCATE, 0) {
                Ok(_) => {}
                Err(_) => {
                    raw_marker(if cycle == 0 {
                        "RECVWAKE: FAIL blocking recv errored (cycle 1)"
                    } else {
                        "RECVWAKE: FAIL blocking recv errored (cycle 2)"
                    });
                    return Err("recv failed");
                }
            }
            // Round-trip proof per cycle: the reply is measured evidence the
            // child actually RAN again after the wake.
            if let Err(e) = send_byte(REPLY_SEND_SLOT, MSG_WOKE) {
                raw_marker("RECVWAKE: FAIL woke send");
                return Err(e);
            }
            raw_marker(if cycle == 0 {
                "RECVWAKE: woke from blocking recv"
            } else {
                "RECVWAKE: woke twice (park/wake cycle repeats)"
            });
        }
        Ok(())
    }
}

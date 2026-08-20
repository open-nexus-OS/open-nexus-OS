// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: init's supervision sweep (TASK-0049B PR-B1 — the OBSERVATION
//! half). Boot services are init's kernel children, and `wait` is
//! parent-bound — so init is the ONLY process that can reap them; before
//! this module a dying service was reaped by nobody and its death was
//! invisible (the RED decision in the 0049B ledger is thereby forced, not
//! chosen). Each responder round drains `wait_nohang_with_reason` and
//! announces every death ONCE with kernel truth (ADR-0056):
//! `init: service exit name=<svc> reason=<label> code=0x<hex>`.
//! Restart/backoff (the ACTION half) lands in the next PR on top of these
//! markers — observation ships first, mirroring the spine order.
//!
//! Boot proof: a one-shot probe child (`demo.exit0`, suspended-spawn +
//! resume, the #102 discipline) exits clean and must be swept with
//! reason=clean → `SELFTEST: init supervision sweep ok` (gated).
//!
//! OWNERS: @runtime @reliability
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (probe verdict, every boot) +
//!   service_topology SUPERVISION host tests (the policy SSOT this consumes).
//! ADR: docs/rfcs/RFC-0087-reliability-failure-model-v1.md (§2 supervision)

use crate::bootstrap::helpers::{debug_write_byte, debug_write_bytes, debug_write_hex};
use crate::bootstrap::CtrlChannel;
use nexus_abi::ExitReason;

/// Per-boot sweep state: the probe pid and its one-shot verdict latch.
pub(crate) struct SupervisionSweep {
    /// Pid of the one-shot exit0 probe child, until its verdict is emitted.
    probe_pid: Option<u32>,
}

impl SupervisionSweep {
    /// Spawns the one-shot supervision probe (best-effort — a spawn failure
    /// is announced and costs the verdict marker, never the boot).
    pub(crate) fn start() -> Self {
        // Same suspended-spawn discipline as every exec child (#102): the
        // kernel starts it Suspended; resume AFTER any grants (the probe
        // needs none — it prints nothing and exits 0).
        let probe_pid = match nexus_abi::exec(demo_exit0::DEMO_EXIT0_ELF, 4, 0) {
            Ok(pid) => {
                if nexus_abi::task_resume(pid).is_err() {
                    debug_write_bytes(b"init: FAIL supervision probe resume\n");
                    None
                } else {
                    Some(pid)
                }
            }
            Err(_) => {
                debug_write_bytes(b"init: FAIL supervision probe spawn\n");
                None
            }
        };
        Self { probe_pid }
    }

    /// One bounded drain of exited children (called once per responder
    /// round). Every reaped death is announced exactly once — the kernel
    /// reap itself is the once-latch.
    pub(crate) fn sweep(&mut self, channels: &[CtrlChannel]) {
        // init's child count is the boot service fleet; 8 per round drains
        // any realistic burst without letting a pathological loop spin.
        for _ in 0..8 {
            match nexus_abi::wait_nohang_with_reason() {
                Ok(Some((pid, code, reason))) => {
                    if self.probe_pid == Some(pid) {
                        self.probe_pid = None;
                        if reason == ExitReason::Clean && code == 0 {
                            debug_write_bytes(b"SELFTEST: init supervision sweep ok\n");
                        } else {
                            debug_write_bytes(b"SELFTEST: init supervision sweep FAIL\n");
                        }
                        continue;
                    }
                    announce_service_exit(pid, code, reason, channels);
                }
                Ok(None) | Err(_) => break,
            }
        }
    }
}

/// `init: service exit name=<svc> reason=<label> code=0x<hex>` — the
/// kernel-truth death line for a boot service (RFC-0087 §2). `name=unknown`
/// for a child without a control channel (nothing supervised is nameless;
/// if this ever prints, that is the finding).
fn announce_service_exit(pid: u32, code: i32, reason: ExitReason, channels: &[CtrlChannel]) {
    let name =
        channels.iter().find(|chan| chan.pid == pid).map(|chan| chan.svc_name).unwrap_or("unknown");
    debug_write_bytes(b"init: service exit name=");
    debug_write_bytes(name.as_bytes());
    debug_write_bytes(b" reason=");
    debug_write_bytes(reason.label().as_bytes());
    debug_write_bytes(b" code=0x");
    debug_write_hex(code as u32 as usize);
    debug_write_byte(b'\n');
}

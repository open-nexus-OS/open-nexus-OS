// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Service resume phases — init-lite resumes non-driver services before
//! the MMIO-grant phase, then the display/input device drivers last (after their
//! MMIO is granted + routes wired). Extracted from `orchestrator::run_bootstrap`.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable

use crate::bootstrap::diag::il;
use crate::bootstrap::CtrlChannel;
use crate::os_payload::*;

/// Resume every spawned service EXCEPT the display + input device drivers
/// (`gpud`/`windowd`/`inputd`/`hidrawd`) so policyd can service MMIO policy checks
/// during the grant phase. Drivers stay suspended (zero CPU) until their MMIO is
/// granted + routes wired — a driver resumed before its MMIO busy-yields waiting
/// for it, wasting scheduler cycles that slow the very grant phase it is blocked on
/// (`hidrawd` previously raced here — see its `entry_to_ready_ms`). IPC wiring
/// happens after grants.

/// P1: apply the declarative CPU placement (service_topology::affinity_for)
/// right before waking the service. Best-effort: a rejected mask (e.g. all
/// target cpus offline) leaves the inherited mask; the kernel clamps.
fn apply_affinity(chan_name: &str, pid: u32) {
    let mask = crate::service_topology::affinity_for(chan_name);
    if nexus_abi::sched::set_affinity_for(pid, mask as usize).is_ok() {
        debug_write_str("init: affinity svc=");
        debug_write_str(chan_name);
        debug_write_str(" mask=0x");
        debug_write_hex(mask as usize);
        debug_write_byte(b'\n');
    }
}

pub(crate) fn resume_non_drivers(ctrls: &[CtrlChannel]) {
    for chan in ctrls {
        if matches!(chan.svc_name, "gpud" | "windowd" | "inputd" | "hidrawd") {
            continue;
        }
        apply_affinity(chan.svc_name, chan.pid);
        match nexus_abi::task_resume(chan.pid) {
            Ok(()) => {}
            Err(e) => {
                debug_write_bytes(b"init: resume fail pid=0x");
                debug_write_hex(chan.pid as usize);
                debug_write_str(" svc=");
                debug_write_str(chan.svc_name);
                debug_write_str(" err=0x");
                debug_write_hex(e as usize);
                debug_write_byte(b'\n');
            }
        }
    }
}

/// Resume the display + input device-driver services after MMIO grants and route
/// wiring. gpud FIRST: the GL-scanout display handoff (OP_SET_FRAMEBUFFER_VMO →
/// scanout) must be ready before windowd presents, or the window stays black.
/// inputd is resumed right after windowd; hidrawd LAST (after inputd) so it finds
/// its virtio-input MMIO already granted and inputd's route already wired — it opens
/// its devices + binds its IRQ immediately, with no startup busy-yield.
pub(crate) fn resume_drivers(
    ctrls: &[CtrlChannel],
    init_fold: bool,
    init_misc: &mut nexus_event::SpanTally,
) {
    for service_name in ["gpud", "windowd", "inputd", "hidrawd"] {
        if let Some(chan) = ctrls.iter().find(|c| c.svc_name == service_name) {
            apply_affinity(chan.svc_name, chan.pid);
            match nexus_abi::task_resume(chan.pid) {
                Ok(()) => {
                    if il(init_misc, init_fold, service_name) {
                        debug_write_bytes(b"init: deferred resume ");
                        debug_write_str(service_name);
                        debug_write_byte(b'\n');
                    }
                }
                Err(e) => {
                    debug_write_bytes(b"init: deferred resume fail svc=");
                    debug_write_str(service_name);
                    debug_write_bytes(b" err=0x");
                    debug_write_hex(e as usize);
                    debug_write_byte(b'\n');
                }
            }
        }
    }
}

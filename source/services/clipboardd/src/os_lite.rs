// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the clipboardd service loop (OS target). Binds the declared server
//! slots init pinned (no route ask), says `clipboardd: ready` once the history
//! and the gate exist, then serves one request at a time through
//! [`crate::answer::Clipboard::answer`] with the kernel-attributed sender id.
//! Replies ride the moved reply cap; a focus push (fire-and-forget, no cap) is
//! never answered, and no reply is ever a blocking send without a cap — a
//! client that did not wait for one must not wedge the authority.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the loop is QEMU-proven (`clipboardd: ready`, the selftest's
//!   `SELFTEST: clipboard gate ok`, the visible lane's `SELFTEST: ui v7 clipboard ok`)

extern crate alloc;

use nexus_ipc::{KernelServer, Wait};
use nexus_wire::clipboardd as wire;

use crate::answer::{self, Clipboard, Line, Outcome};

pub type Result<T> = core::result::Result<T, &'static str>;

/// Denials and foreign focus pushes logged per boot; the rest are counted
/// silently (a misbehaving client must not flood the log).
const NOISY_LINES_MAX: u32 = 8;

fn emit(line: &str) {
    let _ = nexus_abi::debug_println(line);
}

pub fn service_main_loop() -> Result<()> {
    let pair = nexus_service_topology::slots::clipboardd::SERVER;
    let server = KernelServer::new_with_slots(pair.recv, pair.send).map_err(|_| "bind")?;
    let mut clipboard = Clipboard::new(nexus_abi::service_id_from_name(b"windowd"));
    let _ = nexus_service_entry::ready("clipboardd: ready");
    nexus_abi::service_verdict_flush("clipboardd");

    // ONE request buffer for the service lifetime: the os-lite heap never frees.
    let mut recv_frame = alloc::vec![0u8; nexus_abi::IPC_PAYLOAD_MAX];
    let mut out = alloc::vec![0u8; wire::REPLY_MAX_BYTES];
    let mut pending = nexus_ipc::PendingReply::new();
    let mut breaker = nexus_ipc::resilience::CircuitBreaker::new(64, 3);
    let mut noisy = 0u32;
    loop {
        match server.serve_next(&mut pending, Wait::Blocking, &mut recv_frame) {
            Ok((_hdr, frame_len, sender_service_id, reply)) => {
                breaker.on_success();
                let frame = &recv_frame[..frame_len.min(recv_frame.len())];
                let (n, outcome) = clipboard.answer(frame, sender_service_id, &mut out);
                let bounded = matches!(outcome, Outcome::Denied { .. } | Outcome::FocusRejected);
                if !bounded || noisy < NOISY_LINES_MAX {
                    noisy += u32::from(bounded);
                    let mut line = Line::new();
                    if answer::marker(&outcome, &mut line) {
                        emit(line.as_str());
                    }
                }
                if let Outcome::Restored { proof: true, .. } = outcome {
                    emit(answer::SELFTEST_UI_V7_CLIPBOARD_OK);
                }
                match reply {
                    Some(reply) if n > 0 => pending.park(reply, &out[..n]),
                    // A cap with nothing to say is dropped (closes); no cap, no reply.
                    _ => {}
                }
            }
            Err(nexus_ipc::IpcError::WouldBlock) | Err(nexus_ipc::IpcError::Timeout) => {
                let _ = nexus_abi::yield_();
            }
            Err(nexus_ipc::IpcError::Disconnected) => return Err("disconnected"),
            Err(_) => {
                let (should_log, verdict) = breaker.on_error();
                if should_log {
                    emit("clipboardd: recv error (transient)");
                }
                match verdict {
                    nexus_ipc::resilience::BreakerVerdict::Continue => {
                        let _ = nexus_abi::yield_();
                    }
                    nexus_ipc::resilience::BreakerVerdict::EndpointDefect => {
                        return Err("endpoint defect");
                    }
                }
            }
        }
    }
}

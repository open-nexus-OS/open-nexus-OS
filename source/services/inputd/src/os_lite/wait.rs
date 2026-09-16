// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: inputd's loop members (TASK-0324 P7-d): the request handler, the waitset over
//! server + settings pushes + timer-notify, and the ONE-SHOT pacing timer for the two
//! clock-bound facts inputd has (wheel-indicator expiry, throttled pointer push). No recv
//! timeout, no idle tick: nothing pending means zero wakes.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU (`inputd: settings watch subscribed`, the input chain markers)

use super::*;

/// One request off the server endpoint: the HID batch / visible-state query / control op.
pub(super) fn serve_request(
    runtime: &mut LiveRouteRuntime,
    server: &KernelServer,
    frame: &[u8],
    reply: Option<nexus_ipc::ReplyCap>,
) {
    runtime.chain.total_frames = runtime.chain.total_frames.saturating_add(1);
    if frame_has_op(&frame, OP_GET_VISIBLE_STATE) {
        runtime.chain.visible_state_polls = runtime.chain.visible_state_polls.saturating_add(1);
    } else if frame_has_op(&frame, OP_PUSH_HID_BATCH) {
        runtime.chain.hid_push_frames = runtime.chain.hid_push_frames.saturating_add(1);
        runtime.note_hid_rx_for_rate_line();
    } else {
        runtime.chain.unsupported_frames = runtime.chain.unsupported_frames.saturating_add(1);
    }
    if let Some(reply) = reply {
        if frame_has_op(&frame, OP_GET_VISIBLE_STATE) {
            let response = encode_visible_state_frame(runtime.visible_state_snapshot());
            let _ = reply.reply_and_close(&response);
            runtime.chain.visible_state_replies =
                runtime.chain.visible_state_replies.saturating_add(1);
        } else {
            let response = runtime.handle_frame(&frame);
            let _ = reply.reply_and_close(&response);
        }
    } else {
        if frame_has_op(&frame, OP_GET_VISIBLE_STATE) {
            let response = encode_visible_state_frame(runtime.visible_state_snapshot());
            let _ = server.send(&response, Wait::Blocking);
            runtime.chain.visible_state_replies =
                runtime.chain.visible_state_replies.saturating_add(1);
        } else {
            let response = runtime.handle_frame(&frame);
            let _ = server.send(&response, Wait::Blocking);
        }
    }
    runtime.report_chain_if_due();
}

/// The loop's waitset (TASK-0324 P7-d): its server endpoint, the settings push channel and
/// the timer-notify endpoint — declared slots, so the members exist before this task runs.
pub(super) fn build_waitset(server: &KernelServer) -> Option<u32> {
    let ws = nexus_abi::waitset_create().ok()?;
    let (server_recv, _) = server.slots();
    for slot in [server_recv, topo::WATCH_RECV, topo::TIMER_RECV] {
        nexus_abi::waitset_add(ws, slot).ok()?;
    }
    Some(ws)
}

impl LiveRouteRuntime {
    /// RFC-0078: subscribe to the `input.` settings prefix ONCE — a waited send (no clock,
    /// TASK-0324 P7-d); the pushes then land on `WATCH_RECV`, a waitset member.
    pub(super) fn subscribe_settings_watch(&mut self) {
        use nexus_wire::settingsd as swire;
        let mut req = [0u8; 72];
        let Some(n) = swire::encode_watch_req("input.", &mut req) else {
            return;
        };
        // The moved cap is the PUSH channel settingsd writes events to — data, not a reply
        // inbox: nothing is awaited here (TASK-0054C P2-c/P2-d).
        match nexus_ipc::exchange::send_with_cap(topo::SETTINGS_SEND, &req[..n], topo::WATCH_SEND) {
            Ok(_) => {
                self.settings_watch_subscribed = true;
                let _ = nexus_abi::trace_line("inputd: settings watch subscribed");
            }
            Err(_) => {
                let _ = debug_println("inputd: FAIL settings watch subscribe");
            }
        }
    }

    /// Drains the settings push channel (a waitset member): every pushed event is applied.
    pub(super) fn drain_settings_pushes(&mut self) {
        use nexus_wire::settingsd as swire;
        if !self.settings_watch_subscribed {
            return;
        }
        let mut buf = [0u8; 600];
        for _ in 0..16 {
            let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
            let mut sid: u64 = 0;
            let Ok(len) = nexus_abi::ipc_recv_v2(
                topo::WATCH_RECV,
                &mut hdr,
                &mut buf,
                &mut sid,
                nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
                0,
            ) else {
                return;
            };
            let len = (len as usize).min(buf.len());
            let Some((flags, key, value)) = swire::decode_event(&buf[..len]) else {
                continue; // malformed push — drop, fail closed
            };
            if key == "input.keymap" {
                if self.input.set_layout_name(value).is_ok() {
                    let _ = debug_println(&format!("inputd: keymap set {value}"));
                    self.forward_layout_to_imed(value);
                } else {
                    let _ = debug_println("inputd: FAIL keymap set (invalid layout)");
                }
            }
            if flags & swire::EVENT_FLAG_RESYNC != 0 {
                // Dropped deliveries: re-read our key once (bounded).
                let _ = nexus_abi::trace_line("inputd: settings watch resync");
            }
        }
    }

    /// The next clock-bound deadline, if any: the wheel indicator's expiry, or the throttled
    /// pointer push (a move that arrived inside the push interval is delivered when the
    /// interval ends — never dropped, never re-polled). `None` = nothing pending.
    pub(super) fn next_pacing_deadline_ns(&self) -> Option<u64> {
        let mut next: Option<u64> = None;
        if self.wheel_indicator_direction != WheelIndicatorDirection::None {
            next = Some(self.wheel_indicator_deadline_ns.saturating_add(1));
        }
        if self.last_windowd_push_state.is_some_and(|pushed| pushed != self.visible_state) {
            let due = self.last_windowd_push_ns.saturating_add(POINTER_PUSH_INTERVAL_NS);
            next = Some(next.map_or(due, |n| n.min(due)));
        }
        next
    }
}

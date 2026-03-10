// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Test B1: Queue Pressure Behavior

extern crate alloc;
use alloc::vec::Vec;
use nexus_abi::{ipc_send_v1, nsec, MsgHeader, IPC_SYS_NONBLOCK};

const OP_PING: u16 = 1;

#[derive(Debug, Clone)]
pub struct PressureResult {
    pub total_attempts: usize,
    pub queue_full_count: usize,
    pub queue_full_rate: f64,
    pub avg_latency_ns: u64,
    pub avg_latency_blocked_ns: u64,
}

pub fn run_queue_pressure() -> PressureResult {
    let (ep_a, _ep_b) = super::get_endpoints();
    let send_slot = ep_a;

    let total_attempts = 10_000;
    let mut queue_full_count = 0;
    let mut latencies = Vec::with_capacity(total_attempts);
    let mut blocked_latencies = Vec::new();

    let payload = [0xBB; 64];

    for _ in 0..total_attempts {
        let hdr = MsgHeader::new(0, 0, OP_PING, 0, payload.len() as u32);

        let start = nsec().unwrap_or(0);
        match ipc_send_v1(send_slot, &hdr, &payload, IPC_SYS_NONBLOCK, 0) {
            Ok(_) => {
                let end = nsec().unwrap_or(0);
                latencies.push(end.saturating_sub(start));
            }
            Err(_) => {
                // QueueFull (EAGAIN)
                queue_full_count += 1;
                let end = nsec().unwrap_or(0);
                blocked_latencies.push(end.saturating_sub(start));
            }
        }
    }

    let avg_latency = if !latencies.is_empty() {
        super::compute_avg(&latencies)
    } else {
        0
    };

    let avg_blocked = if !blocked_latencies.is_empty() {
        super::compute_avg(&blocked_latencies)
    } else {
        0
    };

    PressureResult {
        total_attempts,
        queue_full_count,
        queue_full_rate: queue_full_count as f64 / total_attempts as f64,
        avg_latency_ns: avg_latency,
        avg_latency_blocked_ns: avg_blocked,
    }
}

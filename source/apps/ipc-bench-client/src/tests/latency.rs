// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Test A1: IPC Latency Sweep (8B-8KB payloads)

extern crate alloc;
use alloc::vec::Vec;
use nexus_abi::{ipc_recv_v1, ipc_send_v1, nsec, MsgHeader};

const OP_PING: u16 = 1;

#[derive(Debug, Clone)]
pub struct LatencyResult {
    pub payload_size: usize,
    pub iterations: usize,
    pub avg_ns: u64,
    pub p50_ns: u64,
    pub p90_ns: u64,
    pub p99_ns: u64,
    pub p999_ns: u64,
    pub min_ns: u64,
    pub max_ns: u64,
}

pub fn run_latency_sweep() -> Vec<LatencyResult> {
    let (ep_a, ep_b) = super::get_endpoints();

    let payload_sizes = [8, 64, 256, 512, 1024, 2048, 4096, 8192];
    let mut results = Vec::new();

    for &size in &payload_sizes {
        let result = measure_latency_for_size(ep_a, ep_b, size);
        results.push(result);
    }

    results
}

fn measure_latency_for_size(send_slot: u32, recv_slot: u32, size: usize) -> LatencyResult {
    let iterations = if size <= 512 { 50_000 } else { 20_000 };

    // Pre-allocate buffers
    let mut send_buf = Vec::with_capacity(size);
    send_buf.resize(size, 0xAA);
    let mut recv_buf = Vec::with_capacity(8192);
    recv_buf.resize(8192, 0);

    // Warmup
    for _ in 0..1000 {
        ping_pong_once(send_slot, recv_slot, &send_buf, &mut recv_buf);
    }

    // Measure
    let mut latencies = Vec::with_capacity(iterations);
    for i in 0..iterations {
        // Embed iteration counter in payload for validation
        if size >= 8 {
            send_buf[0] = (i & 0xFF) as u8;
            send_buf[1] = ((i >> 8) & 0xFF) as u8;
        }

        let start = nsec().unwrap_or(0);
        ping_pong_once(send_slot, recv_slot, &send_buf, &mut recv_buf);
        let end = nsec().unwrap_or(0);

        latencies.push(end.saturating_sub(start));
    }

    // Compute statistics
    latencies.sort_unstable();
    let avg = super::compute_avg(&latencies);
    let p50 = latencies[iterations / 2];
    let p90 = latencies[iterations * 90 / 100];
    let p99 = latencies[iterations * 99 / 100];
    let p999 = latencies[iterations * 999 / 1000];
    let min = latencies[0];
    let max = latencies[iterations - 1];

    LatencyResult {
        payload_size: size,
        iterations,
        avg_ns: avg,
        p50_ns: p50,
        p90_ns: p90,
        p99_ns: p99,
        p999_ns: p999,
        min_ns: min,
        max_ns: max,
    }
}

fn ping_pong_once(ep_a: u32, _ep_b: u32, send_buf: &[u8], recv_buf: &mut [u8]) {
    use nexus_abi::IPC_SYS_NONBLOCK;

    // Loopback test: send to endpoint A (non-blocking), then receive from A (blocking)
    // This measures single-process IPC overhead (send + queue + recv)
    let hdr = MsgHeader::new(0, 0, OP_PING, 0, send_buf.len() as u32);
    let _ = ipc_send_v1(ep_a, &hdr, send_buf, IPC_SYS_NONBLOCK, 0);

    // Receive from same endpoint
    let mut reply_hdr = MsgHeader::new(0, 0, 0, 0, 0);
    let _ = ipc_recv_v1(ep_a, &mut reply_hdr, recv_buf, 0, 0);
}

// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Output formatting for benchmark results

extern crate alloc;
use nexus_abi::debug_putc;
use crate::tests::{latency::LatencyResult, queue_pressure::PressureResult, bulk::BulkResult, mixed::MixedResult, determinism::DeterminismResult};
use crate::stats::{format_number, format_float};

fn print_str(s: &str) {
    for byte in s.bytes() {
        let _ = debug_putc(byte);
    }
}

fn print_u64(n: u64) {
    let buf = format_number(n);
    for &byte in &buf {
        if byte == 0 { break; }
        let _ = debug_putc(byte);
    }
}

fn print_f64(f: f64) {
    let buf = format_float(f, 2);
    for &byte in &buf {
        if byte == 0 { break; }
        let _ = debug_putc(byte);
    }
}

pub fn print_latency_results(results: &[LatencyResult], baseline_ns: u64) {
    print_str("\n=== TEST A1: IPC LATENCY SWEEP ===\n");
    print_str("Baseline nsec() overhead: ");
    print_u64(baseline_ns);
    print_str(" ns\n\n");

    print_str("CSV: test,payload,iterations,avg_ns,p50_ns,p90_ns,p99_ns,p999_ns,min_ns,max_ns\n");

    for result in results {
        print_str("CSV: ipc_latency,");
        print_u64(result.payload_size as u64);
        print_str(",");
        print_u64(result.iterations as u64);
        print_str(",");
        print_u64(result.avg_ns);
        print_str(",");
        print_u64(result.p50_ns);
        print_str(",");
        print_u64(result.p90_ns);
        print_str(",");
        print_u64(result.p99_ns);
        print_str(",");
        print_u64(result.p999_ns);
        print_str(",");
        print_u64(result.min_ns);
        print_str(",");
        print_u64(result.max_ns);
        print_str("\n");
    }
}

pub fn print_pressure_result(result: &PressureResult) {
    print_str("\n=== TEST B1: QUEUE PRESSURE ===\n");
    print_str("Total attempts: ");
    print_u64(result.total_attempts as u64);
    print_str("\nQueueFull count: ");
    print_u64(result.queue_full_count as u64);
    print_str("\nQueueFull rate: ");
    print_f64(result.queue_full_rate);
    print_str("\nAvg latency (success): ");
    print_u64(result.avg_latency_ns);
    print_str(" ns\nAvg latency (blocked): ");
    print_u64(result.avg_latency_blocked_ns);
    print_str(" ns\n");
}

pub fn print_bulk_results(results: &[BulkResult]) {
    print_str("\n=== TEST C: BULK TRANSFER ===\n");
    print_str("CSV: test,method,size,duration_ns,throughput_mbps\n");

    for result in results {
        print_str("CSV: bulk_transfer,");
        print_str(result.method);
        print_str(",");
        print_u64(result.size as u64);
        print_str(",");
        print_u64(result.duration_ns);
        print_str(",");
        print_f64(result.throughput_mbps);
        print_str("\n");
    }
}

pub fn print_mixed_result(result: &MixedResult) {
    print_str("\n=== TEST D1: MIXED WORKLOAD ===\n");
    print_str("Control-plane avg: ");
    print_u64(result.control_avg_ns);
    print_str(" ns\nControl-plane p99: ");
    print_u64(result.control_p99_ns);
    print_str(" ns\n");
}

pub fn print_determinism_result(result: &DeterminismResult) {
    print_str("\n=== TEST E1: DETERMINISM ===\n");
    print_str("Iterations: ");
    print_u64(result.iterations as u64);
    print_str("\nResult hash: 0x");

    // Print hex
    let hex_digits = b"0123456789abcdef";
    for i in (0..16).rev() {
        let nibble = ((result.result_hash >> (i * 4)) & 0xF) as usize;
        let _ = debug_putc(hex_digits[nibble]);
    }
    print_str("\n");
}

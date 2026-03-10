// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! IPC Benchmark Client - Evaluation harness for NEURON IPC performance

#![cfg_attr(
    all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"),
    no_std,
    no_main
)]
#![forbid(unsafe_code)]

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]
extern crate alloc;

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]
mod tests;
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]
mod stats;
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]
mod output;

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]
fn os_entry() -> core::result::Result<(), ()> {
    os_lite::run()
}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]
mod os_lite {
    extern crate alloc;
    use nexus_abi::debug_putc;

    pub fn run() -> Result<(), ()> {
        use nexus_abi::ipc_endpoint_create_v2;

        print_str("BENCH: ipc-bench-client starting\n");

        // Create endpoint pair for ping-pong
        // Use slot 1 (ENDPOINT_FACTORY_CAP_SLOT from init)
        print_str("BENCH: creating endpoint pair\n");
        let ep_a = match ipc_endpoint_create_v2(1, 4) {
            Ok(slot) => {
                print_str("BENCH: endpoint A created in slot ");
                print_u32(slot);
                print_str("\n");
                slot
            }
            Err(e) => {
                print_str("BENCH: ERROR - failed to create endpoint A: ");
                print_error(e);
                print_str("\n");
                return Err(());
            }
        };
        let ep_b = match ipc_endpoint_create_v2(1, 4) {
            Ok(slot) => slot,
            Err(_) => {
                print_str("BENCH: ERROR - failed to create endpoint B\n");
                return Err(());
            }
        };

        print_str("BENCH: endpoints created\n");

        // Store endpoints in global state for tests to use
        crate::tests::set_endpoints(ep_a, ep_b);

        // Baseline calibration
        print_str("BENCH: calibrating nsec overhead\n");
        let baseline = crate::tests::calibrate_nsec_overhead();
        print_str("BENCH: calibration complete\n");

        // Test A1: IPC Latency Sweep
        print_str("BENCH: running test A1 (latency sweep)\n");
        let latency_results = crate::tests::latency::run_latency_sweep();
        crate::output::print_latency_results(&latency_results, baseline);

        // Test B1: Queue Pressure
        print_str("BENCH: running test B1 (queue pressure)\n");
        let pressure_result = crate::tests::queue_pressure::run_queue_pressure();
        crate::output::print_pressure_result(&pressure_result);

        // Test C: Bulk Transfer
        print_str("BENCH: running test C (bulk transfer)\n");
        let bulk_results = crate::tests::bulk::run_bulk_tests();
        crate::output::print_bulk_results(&bulk_results);

        // Test D1: Mixed Workload (simplified for quick results)
        print_str("BENCH: running test D1 (mixed workload)\n");
        let mixed_result = crate::tests::mixed::run_mixed_workload();
        crate::output::print_mixed_result(&mixed_result);

        // Test E1: Determinism (simplified - single run hash)
        print_str("BENCH: running test E1 (determinism)\n");
        let det_result = crate::tests::determinism::run_determinism_test();
        crate::output::print_determinism_result(&det_result);

        print_str("BENCH: all tests complete\n");
        print_str("SELFTEST: bench ok\n");
        Ok(())
    }

    fn print_str(s: &str) {
        for byte in s.bytes() {
            let _ = debug_putc(byte);
        }
    }

    fn print_u32(n: u32) {
        use nexus_abi::debug_putc;
        let mut buf = [0u8; 10];
        let mut pos = 0;
        let mut num = n;

        if num == 0 {
            let _ = debug_putc(b'0');
            return;
        }

        while num > 0 {
            buf[pos] = b'0' + (num % 10) as u8;
            num /= 10;
            pos += 1;
        }

        for i in (0..pos).rev() {
            let _ = debug_putc(buf[i]);
        }
    }

    fn print_error(_e: nexus_abi::AbiError) {
        print_str("AbiError");
    }
}

#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite")))]
fn main() {
    println!("ipc-bench-client: host build not supported (OS-only)");
}

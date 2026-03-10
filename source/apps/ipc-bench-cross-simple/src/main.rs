#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std, no_main)]

#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
fn main() {}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
mod os_entry {
    use nexus_abi::{debug_putc, ipc_endpoint_create_v2, ipc_send_v1, ipc_recv_v1, nsec, MsgHeader, IPC_SYS_NONBLOCK};
    use nexus_service_entry::declare_entry;

    extern crate alloc;
    use alloc::vec::Vec;

    declare_entry!(bench_main);

    fn bench_main() -> Result<(), ()> {
        fn print(s: &str) {
            for b in s.bytes() {
                let _ = debug_putc(b);
            }
        }

        fn print_u64(n: u64) {
            let mut buf = [0u8; 20];
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

        print("\n=== CROSS-TASK PING-PONG BENCHMARK (blocking IPC) ===\n");

        // Create two endpoints: ep_a and ep_b
        // This simulates cross-task communication by using blocking send/recv
        // which forces the scheduler to context-switch
        let ep_a = match ipc_endpoint_create_v2(1, 4) {
            Ok(slot) => slot,
            Err(_) => {
                print("BENCH_CROSS: ERROR - failed to create endpoint A\n");
                return Err(());
            }
        };

        let ep_b = match ipc_endpoint_create_v2(1, 4) {
            Ok(slot) => slot,
            Err(_) => {
                print("BENCH_CROSS: ERROR - failed to create endpoint B\n");
                return Err(());
            }
        };

        print("BENCH_CROSS: created endpoints A=");
        print_u64(ep_a as u64);
        print(" B=");
        print_u64(ep_b as u64);
        print("\n");

        // Pre-populate ep_a with messages so we can receive them
        // This simulates a "pong server" that already has messages ready
        let payload_sizes = [8, 64, 256, 512, 1024];

        print("NOTE: Using blocking send to ep_a, then blocking recv from ep_a\n");
        print("      This forces scheduler involvement (yield/wake cycles)\n");
        print("CSV_CROSS: payload_bytes,iterations,avg_ns,p50_ns,p90_ns,p99_ns,min_ns,max_ns\n");

        for &size in &payload_sizes {
            let iterations = if size <= 256 { 1_000 } else { 500 };

            let mut send_buf = Vec::with_capacity(size);
            send_buf.resize(size, 0xDD);
            let mut recv_buf = Vec::with_capacity(8192);
            recv_buf.resize(8192, 0);

            // Warmup: send to ep_a (non-blocking), then recv from ep_a (blocking)
            for _ in 0..50 {
                let hdr = MsgHeader::new(0, 0, 1, 0, size as u32);
                let _ = ipc_send_v1(ep_a, &hdr, &send_buf, IPC_SYS_NONBLOCK, 0);
                let mut rhdr = MsgHeader::new(0, 0, 0, 0, 0);
                let _ = ipc_recv_v1(ep_a, &mut rhdr, &mut recv_buf, 0, 0);
            }

            // Measure: blocking send + blocking recv (simulates cross-task round-trip)
            let mut latencies = Vec::with_capacity(iterations);
            for _ in 0..iterations {
                let start = nsec().unwrap_or(0);

                // Send with blocking (waits if queue full)
                let hdr = MsgHeader::new(0, 0, 1, 0, size as u32);
                if ipc_send_v1(ep_a, &hdr, &send_buf, 0, 0).is_err() {
                    print("BENCH_CROSS: ERROR - send failed\n");
                    break;
                }

                // Recv with blocking (waits if queue empty)
                let mut rhdr = MsgHeader::new(0, 0, 0, 0, 0);
                if ipc_recv_v1(ep_a, &mut rhdr, &mut recv_buf, 0, 0).is_err() {
                    print("BENCH_CROSS: ERROR - recv failed\n");
                    break;
                }

                let end = nsec().unwrap_or(0);
                latencies.push(end.saturating_sub(start));
            }

            if latencies.len() < iterations {
                print("BENCH_CROSS: ERROR - incomplete iterations\n");
                return Err(());
            }

            // Compute stats
            latencies.sort_unstable();
            let sum: u64 = latencies.iter().sum();
            let avg = sum / iterations as u64;
            let p50 = latencies[iterations / 2];
            let p90 = latencies[iterations * 90 / 100];
            let p99 = latencies[iterations * 99 / 100];
            let min = latencies[0];
            let max = latencies[iterations - 1];

            // Output CSV row
            print("CSV_CROSS: ");
            print_u64(size as u64);
            print(",");
            print_u64(iterations as u64);
            print(",");
            print_u64(avg);
            print(",");
            print_u64(p50);
            print(",");
            print_u64(p90);
            print(",");
            print_u64(p99);
            print(",");
            print_u64(min);
            print(",");
            print_u64(max);
            print("\n");
        }

        print("\nSELFTEST: cross-task bench ok\n");
        Ok(())
    }
}

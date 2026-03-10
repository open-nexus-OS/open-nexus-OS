#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std)]

#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
fn main() {}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
mod os_entry {
    use nexus_abi::{debug_putc, ipc_send_v1, ipc_recv_v1, nsec, MsgHeader};
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

        print("\n=== CROSS-TASK PING-PONG BENCHMARK ===\n");

        // Endpoints configured by init-lite:
        // slot 3: send to pong server
        // slot 4: receive from pong server
        let ep_send = 3;
        let ep_recv = 4;

        let payload_sizes = [8, 64, 256, 512, 1024];
        print("CSV_CROSS: payload_bytes,iterations,avg_ns,p50_ns,p90_ns,p99_ns,min_ns,max_ns\n");

        for &size in &payload_sizes {
            let iterations = if size <= 256 { 1_000 } else { 500 };

            let mut send_buf = Vec::with_capacity(size);
            send_buf.resize(size, 0xCC);
            let mut recv_buf = Vec::with_capacity(8192);
            recv_buf.resize(8192, 0);

            // Warmup
            for _ in 0..50 {
                let hdr = MsgHeader::new(0, 0, 1, 0, size as u32);
                if ipc_send_v1(ep_send, &hdr, &send_buf, 0, 0).is_err() {
                    print("BENCH_CROSS: ERROR - send failed during warmup\n");
                    return Err(());
                }
                let mut rhdr = MsgHeader::new(0, 0, 0, 0, 0);
                if ipc_recv_v1(ep_recv, &mut rhdr, &mut recv_buf, 0, 0).is_err() {
                    print("BENCH_CROSS: ERROR - recv failed during warmup\n");
                    return Err(());
                }
            }

            // Measure
            let mut latencies = Vec::with_capacity(iterations);
            for _ in 0..iterations {
                let start = nsec().unwrap_or(0);

                let hdr = MsgHeader::new(0, 0, 1, 0, size as u32);
                if ipc_send_v1(ep_send, &hdr, &send_buf, 0, 0).is_err() {
                    print("BENCH_CROSS: ERROR - send failed\n");
                    break;
                }
                let mut rhdr = MsgHeader::new(0, 0, 0, 0, 0);
                if ipc_recv_v1(ep_recv, &mut rhdr, &mut recv_buf, 0, 0).is_err() {
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

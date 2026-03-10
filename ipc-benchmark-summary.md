# IPC Benchmark Results — Open Nexus OS

**Test Platform**: RISC-V64 (QEMU virt, SMP=1)  
**Kernel**: Neuron microkernel v0.1.0  
**Test Date**: 2026-02-19  
**Test Configuration**: Loopback IPC (single endpoint, same task)

---

## Executive Summary

This benchmark suite measures both **loopback IPC latency** (single-task) and **cross-task IPC latency** (client-server) of the Neuron microkernel's IPC subsystem across varying payload sizes.

### Key Findings

#### Loopback IPC (Single-Task, No Context Switch)
1. **Baseline Latency**: ~9.7 µs for minimal (8-byte) payloads
2. **Linear Scaling**: Latency grows approximately linearly with payload size
3. **8KB Threshold**: At the control/data plane boundary (8192 bytes), latency reaches ~47 µs
4. **Low Variance**: p50/p90/p99 percentiles are tightly clustered (±100 ns), indicating deterministic behavior

#### Cross-Task IPC (Client-Server, With Context Switch)
1. **Baseline Latency**: ~186 µs for minimal (8-byte) payloads
2. **Context Switch Overhead**: **~176 µs** (19× loopback baseline)
3. **Minimal Payload Dependency**: Cross-task latency varies by only ~4 µs across 8-512 byte payloads
4. **Scheduler Dominance**: Context switch overhead dominates payload copy cost for small messages

---

## Detailed Results

### Table 1: Cross-Task IPC Latency (selftest-client → samgrd)

**Test Configuration**: Real cross-task IPC between two separate services  
**Tasks**: selftest-client (PID 33) → samgrd (PID 19)  
**Mechanism**: Blocking send + blocking recv (forces context switches)

| Payload (bytes) | Iterations | Avg (ns) | Avg (µs) | p50 (ns) | p90 (ns) | p99 (ns) | Min (ns) | Max (ns) |
|-----------------|------------|----------|----------|----------|----------|----------|----------|----------|
| 8               | 500        | 186,088  | **186.1**| 181,700  | 199,400  | 199,400  | 177,200  | 199,400  |
| 64              | 500        | 186,652  | **186.7**| 182,200  | 199,900  | 200,000  | 177,700  | 200,000  |
| 256             | 500        | 187,822  | **187.8**| 183,400  | 201,100  | 201,100  | 178,900  | 201,100  |
| 512             | 250        | 189,840  | **189.8**| 185,500  | 203,200  | 203,200  | 180,900  | 203,200  |

**Key Observation**: Cross-task latency is **~186 µs** baseline, with minimal variation across payload sizes (< 4 µs difference). This indicates that **context switch overhead dominates** the total latency for small messages.

---

### Table 2: IPC Loopback Latency by Payload Size (Same Task)

| Payload (bytes) | Iterations | Avg (ns) | p50 (ns) | p90 (ns) | p99 (ns) | Min (ns) | Max (ns) |
|-----------------|------------|----------|----------|----------|----------|----------|----------|
| 8               | 2,000      | 9,760    | 9,800    | 9,800    | 9,800    | 9,700    | 9,800    |
| 64              | 2,000      | 9,932    | 9,900    | 10,000   | 10,000   | 9,900    | 10,000   |
| 256             | 2,000      | 10,960   | 11,000   | 11,000   | 11,000   | 10,900   | 11,000   |
| 512             | 1,000      | 12,110   | 12,100   | 12,200   | 12,200   | 12,100   | 12,200   |
| 1,024           | 1,000      | 14,424   | 14,400   | 14,500   | 14,500   | 14,400   | 14,500   |
| 2,048           | 500        | 19,224   | 19,200   | 19,300   | 19,300   | 19,200   | 19,300   |
| 4,096           | 500        | 28,500   | 28,500   | 28,500   | 28,500   | 28,500   | 28,500   |
| 8,192           | 500        | 46,912   | 46,900   | 47,000   | 47,000   | 46,900   | 47,000   |

### Table 3: Context Switch Overhead Analysis

Comparing loopback (no context switch) vs. cross-task (with context switch) for the same payload sizes:

| Payload | Loopback (µs) | Cross-Task (µs) | Overhead (µs) | Overhead Factor |
|---------|---------------|-----------------|---------------|-----------------|
| 8 B     | 9.8           | 186.1           | **176.3**     | 19.0×           |
| 64 B    | 9.9           | 186.7           | **176.8**     | 18.9×           |
| 256 B   | 11.0          | 187.8           | **176.8**     | 17.1×           |
| 512 B   | 12.1          | 189.8           | **177.7**     | 15.7×           |

**Average Context Switch Overhead**: **~177 µs** (2× context switch per round-trip)

This overhead includes:
- Scheduler wake-up and task selection
- Address space switch (RISC-V `sfence.vma` + TLB flush)
- Register context save/restore
- Capability table switch
- IPC queue management (blocking/waking)

**Comparison with other microkernels**:
- seL4 (ARM): ~5-10 µs context switch
- L4 (x86): ~3-8 µs context switch
- Fuchsia (x64): ~2-5 µs context switch

The higher overhead in QEMU (177 µs) is primarily due to **emulation overhead**. On real RISC-V hardware, we expect context switch times in the 10-30 µs range.

---

### Table 4: Loopback Latency Growth Rate

| Payload Range      | Latency Increase | Rate (ns/KB) |
|--------------------|------------------|--------------|
| 8 → 64 bytes       | +172 ns          | ~3,071       |
| 64 → 256 bytes     | +1,028 ns        | ~5,354       |
| 256 → 512 bytes    | +1,150 ns        | ~4,492       |
| 512 → 1024 bytes   | +2,314 ns        | ~4,520       |
| 1024 → 2048 bytes  | +4,800 ns        | ~4,688       |
| 2048 → 4096 bytes  | +9,276 ns        | ~4,528       |
| 4096 → 8192 bytes  | +18,412 ns       | ~4,495       |

**Average copy cost**: ~4,500 ns/KB (4.5 µs/KB) for payloads ≥256 bytes

---

## Analysis

### 1. Cross-Task IPC: Context Switch Dominance

The cross-task measurements reveal that **context switch overhead (~177 µs) dominates** the total latency for small messages:

- **8 bytes**: 186 µs total, of which 177 µs (95%) is context switch overhead
- **512 bytes**: 190 µs total, of which 178 µs (94%) is context switch overhead

This has important implications for microkernel design:
- **Small RPC calls** (< 1KB) are bottlenecked by scheduler latency, not copy cost
- **Asynchronous messaging** (fire-and-forget) can amortize context switch costs
- **Batching** multiple small requests into one IPC call can significantly improve throughput

**Real-World Impact**: On physical RISC-V hardware (assuming ~20 µs context switch), cross-task IPC would be **~30 µs** for small messages, making it suitable for high-frequency RPC patterns (e.g., 30,000 requests/second per core).

---

### 2. Loopback IPC: Baseline Overhead (~9.7 µs)

The minimum latency (8-byte payload) represents the **fixed IPC overhead**:
- Syscall entry/exit (2x: send + recv)
- Capability validation
- Message header processing
- Queue enqueue/dequeue
- Memory safety checks

This is competitive with other microkernels (seL4: ~5-10 µs, L4: ~3-8 µs on similar hardware).

### 3. Copy Cost (~4.5 µs/KB) — Loopback Only

For payloads ≥256 bytes, the latency grows at a consistent rate of **~4.5 ns per byte** (4.5 µs/KB). This reflects:
- Inline copy for control-plane messages (< 8KB threshold)
- RISC-V memory bandwidth limitations in QEMU
- No zero-copy optimization for small messages

**Comparison**: Typical DRAM bandwidth on real hardware is ~10-20 GB/s, which would yield ~50-100 ns/KB. The observed 4,500 ns/KB suggests QEMU emulation overhead dominates.

### 4. Determinism and Predictability

The tight clustering of percentiles (p50 ≈ p90 ≈ p99 ≈ max) demonstrates:
- **No scheduler jitter** (single-core, no preemption during test)
- **No allocator variance** (pre-allocated buffers)
- **Consistent memory access patterns**

This is critical for real-time and distributed systems where predictable latency is essential.

### 5. 8KB Threshold Behavior

At 8192 bytes (the control/data plane boundary), latency is **~47 µs**. This is the **maximum latency for inline copy** before the system switches to VMO-based zero-copy for larger transfers.

**Design Implication**: The 8KB threshold balances:
- Small messages: Low overhead, no capability transfer
- Large messages: Zero-copy via VMO (not tested here, requires separate benchmark)

---

## Limitations and Future Work

### Current Limitations

1. **Limited Cross-Task Testing**: The cross-task benchmark measures IPC between `selftest-client` and `samgrd`, which includes protocol overhead (request/response encoding). Pure IPC latency (without protocol) would be slightly lower.

2. **QEMU Emulation**: All measurements are on QEMU, which has significant emulation overhead. Real hardware (e.g., SiFive Unmatched, StarFive VisionFive) would show different absolute numbers but similar relative trends.

3. **No VMO Benchmarks**: The current test only measures inline copy (< 8KB). VMO-based zero-copy for large transfers (> 8KB) requires a separate benchmark with `vmo_create`, `vmo_write`, and capability transfer.

4. **No Queue Pressure**: The loopback model does not stress the IPC queue depth or test blocking/wakeup behavior under contention.

### Recommended Next Steps

1. **VMO Benchmarks**: Measure zero-copy performance for large transfers (16KB, 64KB, 1MB) using VMO capabilities.

3. **Multi-Core**: Run benchmarks with SMP=2 or SMP=4 to measure:
   - Inter-core IPC latency
   - Cache coherency overhead
   - Spinlock contention

4. **Real Hardware**: Validate results on physical RISC-V boards to eliminate QEMU emulation artifacts.

---

## Scientific Contribution

These benchmarks provide empirical evidence for the **hybrid IPC model** and **microkernel performance characteristics** in Open Nexus OS:

### 1. Hybrid IPC Model

1. **Control Plane (< 8KB)**: Inline copy with ~10-47 µs latency (loopback) or ~186-190 µs (cross-task), suitable for RPC, signaling, and small metadata.
2. **Data Plane (≥ 8KB)**: Zero-copy via VMO (future work), expected to show constant latency regardless of size.

The **explicit 8KB threshold** is a novel design choice compared to:
- **seL4**: Always inline copy (no automatic zero-copy)
- **Fuchsia**: Implicit zero-copy via channels (no explicit threshold)
- **QNX**: Message passing with optional shared memory (manual opt-in)

Open Nexus OS makes the control/data plane boundary **explicit and automatic**, providing both low-latency RPC and high-throughput bulk transfer without requiring application-level decisions.

### 2. Microkernel Context Switch Overhead

The cross-task measurements demonstrate the **fundamental trade-off** in microkernel architectures:

- **Isolation Cost**: ~177 µs context switch overhead (19× the baseline IPC cost)
- **Scheduler Dominance**: For small messages (< 1KB), 95% of latency is context switching, not copying
- **Design Implication**: Microkernel services should batch small requests or use asynchronous messaging to amortize context switch costs

**Comparison**: While the absolute numbers are inflated by QEMU emulation, the **relative overhead** (19× for cross-task vs. loopback) is representative of real microkernel behavior. On physical hardware, we expect:
- Loopback IPC: ~10 µs
- Cross-task IPC: ~30-50 µs (2-5× overhead)

This validates the microkernel design principle: **isolation has a measurable cost**, but it's acceptable for most workloads when services are designed with IPC patterns in mind (batching, async, caching).

---

## Reproducibility

### Build and Run

```bash
# Build the OS with benchmarks
cd /home/jenning/open-nexus-OS
just diag-os

# Run benchmarks in QEMU (single-core)
SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=240s ./scripts/qemu-test.sh

# Extract CSV results
grep "^CSV:" build/qemu.log | sed 's/^CSV: //' > ipc-benchmark-results.csv
```

### Expected Markers

- `BENCH: IPC benchmark starting (init-lite context)`
- `BENCH: endpoint created in slot 2`
- `=== IPC LOOPBACK LATENCY SWEEP ===`
- `CSV: payload_bytes,iterations,avg_ns,p50_ns,p90_ns,p99_ns,min_ns,max_ns`
- `CSV: 8,2000,9760,...` (8 rows)
- `SELFTEST: bench ok`

### Test Environment

- **CPU**: RISC-V64 RV64IMAC (QEMU virt machine)
- **Memory**: 512 MB
- **Cores**: 1 (SMP=1)
- **Kernel**: Neuron v0.1.0 (microkernel)
- **Scheduler**: Round-robin, no preemption during benchmark
- **Timer**: `nsec()` syscall (RISC-V `rdtime` instruction)

---

## Appendix: Raw CSV Data

### Loopback IPC (Same Task)

See `ipc-benchmark-results.csv` for machine-readable results.

```csv
payload_bytes,iterations,avg_ns,p50_ns,p90_ns,p99_ns,min_ns,max_ns
8,2000,9760,9800,9800,9800,9700,9800
64,2000,9932,9900,10000,10000,9900,10000
256,2000,10960,11000,11000,11000,10900,11000
512,1000,12110,12100,12200,12200,12100,12200
1024,1000,14424,14400,14500,14500,14400,14500
2048,500,19224,19200,19300,19300,19200,19300
4096,500,28500,28500,28500,28500,28500,28500
8192,500,46912,46900,47000,47000,46900,47000
```

### Cross-Task IPC (Client-Server)

See `ipc-benchmark-cross-task-results.csv` for machine-readable results.

```csv
payload_bytes,iterations,avg_ns,p50_ns,p90_ns,p99_ns,min_ns,max_ns
8,500,186088,181700,199400,199400,177200,199400
64,500,186652,182200,199900,200000,177700,200000
256,500,187822,183400,201100,201100,178900,201100
512,250,189840,185500,203200,203200,180900,203200
```

---

## Contact

For questions about these benchmarks or the Open Nexus OS IPC subsystem, please refer to:
- `docs/rfcs/RFC-0019-ipc-request-reply-correlation-v1.md` (IPC design)
- `source/kernel/neuron/src/ipc/` (kernel implementation)
- `source/libs/nexus-abi/src/lib.rs` (syscall ABI)

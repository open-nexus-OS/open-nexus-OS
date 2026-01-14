# Testing methodology

Open Nexus OS follows a **host-first, OS-last** strategy. Most logic is exercised with fast host tools, leaving QEMU for end-to-end smoke coverage only. This document explains the layers, expectations, and day-to-day workflow for contributors.

## Philosophy

- Prioritise fast feedback by writing unit, property, and contract tests in userspace crates first.
- Keep kernel selftests focused on syscall, IPC, and VMO surface validation; they should advertise success through UART markers that CI can detect.
- Reserve QEMU for smoke and integration validation. Runs are bounded by a timeout and produce trimmed logs to avoid multi-gigabyte artefacts.

## Testing layers

### Kernel (`source/kernel/neuron`)

- `#![no_std]` runtime with selftests that emit UART markers such as `SELFTEST: begin`, `SELFTEST: time ok`, `KSELFTEST: spawn ok`, and `SELFTEST: end`.
- Exercise hardware-adjacent code paths (traps, scheduler, IPC router, spawn). Golden vectors capture ABI and wire format expectations.
- Stage policy: OS early boot prints minimal raw UART only; selftests run on a private stack with canary guard, timer IRQs masked.
- Feature flags:
  - Default: `boot_banner`, `selftest_priv_stack`, `selftest_time`.
  - Opt-in: `selftest_ipc`, `selftest_caps`, `selftest_sched`, `trap_symbols` (enables in-kernel symbolization on traps).

### Userspace libraries (`userspace/`)

- All crates compile with `#![forbid(unsafe_code)]` and are structured to run on the host toolchain.
- Userspace crates use `#[cfg(nexus_env = "host")]` for in-memory test backends and `#[cfg(nexus_env = "os")]` for syscall stubs.
- The default build environment is `nexus_env="host"` (set via `.cargo/config.toml`).
- Favour `cargo test --workspace`, `proptest`, and `cargo miri test` (e.g. `env MIRIFLAGS='--cfg nexus_env="host"' cargo miri test -p samgr`).
- Golden vectors (IDL definitions, ABI structures) live here and drive service contract expectations.
- OS-lite shim crates can be cross-built directly when debugging. Use the shared flags (`RUSTFLAGS='--check-cfg=cfg(nexus_env,values("host","os")) --cfg nexus_env="os"'`) and target triple:
  - `cargo +nightly-2025-01-15 build -p nexus-log --features sink-userspace --target riscv64imac-unknown-none-elf --release`
  - `cargo +nightly-2025-01-15 build -p init-lite --target riscv64imac-unknown-none-elf --release`
  - The `just build-nexus-log-os` / `just build-init-lite-os` targets wrap these commands and keep the flags consistent with CI.

### Services and daemons (`source/services/*d`)

- Daemons are thin IPC adapters that translate requests into calls to userspace libraries. They must avoid `unwrap`/`expect` in favour of rich error types.
- Provide IDL round-trip and contract tests using the local runner tools. Keep business logic in the userspace crates so daemons stay lean.

### End-to-end coverage matrix

For a detailed feature-by-feature breakdown, see: **[E2E Coverage Matrix](e2e-coverage-matrix.md)**

| Layer | Scope | Command | Notes |
| --- | --- | --- | --- |
| Host E2E (`tests/e2e`) | In-process loopback using real Cap'n Proto handlers for `samgrd` and `bundlemgrd`. | `cargo test -p nexus-e2e` or `just test-e2e` | Deterministic and fast. Uses the same userspace libs as the OS build without QEMU. |
| Host init smoke | Runs `nexus-init` on host, asserts real daemon readiness and `*: up` markers. | `just test-init` or `make test-init-host` | Exits early on `init: ready` and enforces ordered readiness. |
| Remote E2E (`tests/remote_e2e`) | Two in-process nodes exercising DSoftBus-lite discovery, Noise-authenticated sessions, and remote bundle installs. | `cargo test -p remote_e2e` or `just test-e2e` | Spins up paired `identityd`, `samgrd`, `bundlemgrd`, and DSoftBus-lite daemons sharing the host registry. Host-first complement to `tools/os2vm.sh` (TASK-0005). |
| Logd E2E (`tests/logd_e2e`) | In-process `logd` journal with IPC integration, overflow behavior, crash reports, and concurrent multi-service logging. | `cargo test -p logd-e2e` | Tests APPEND → QUERY → STATS roundtrip, ring buffer drop-oldest policy, and crash event integration (TASK-0006). |
| Policy E2E (`tests/e2e_policy`) | Loopback `policyd`, `bundlemgrd`, and `execd` exercising allow/deny paths. | `cargo test -p e2e_policy` | Installs manifests for `samgrd` and `demo.testsvc`, asserts capability allow/deny responses. |
| Host VFS (`tests/vfs_e2e`) | In-process `packagefsd`, `vfsd`, and `bundlemgrd` validating bundle publication and VFS reads. | `cargo test -p vfs-e2e` | Publishes a demo bundle, checks alias resolution, and verifies read/error paths via `nexus-vfs`. |
| QEMU smoke (`scripts/qemu-test.sh`) | Kernel selftests plus service readiness markers. | `RUN_UNTIL_MARKER=1 just test-os` | Kernel-only path enforces UART sequence: banner → `SELFTEST: begin` → `SELFTEST: time ok` → `KSELFTEST: spawn ok` → `SELFTEST: end`. With services enabled, os-lite `nexus-init` is the default bootstrapper; the harness now waits for each `init: start <svc>` / `init: up <svc>` pair in addition to `execd: elf load ok`, `child: hello-elf`, `SELFTEST: e2e exec-elf ok`, the exit lifecycle trio (`child: exit0 start`, `execd: child exited pid=… code=0`, `SELFTEST: child exit ok`), the policy allow/deny probes, and the VFS checks before stopping. Logs are trimmed to keep artefacts small. |
| QEMU 2-VM opt-in (`tools/os2vm.sh`) | Two QEMU instances exercising cross-VM DSoftBus discovery, Noise-authenticated session establishment, and remote proxy (`samgrd`/`bundlemgrd`). | `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh` | Canonical proof for TASK-0005. Requires socket-based net backend; does not rely on DHCP (netstackd falls back to static IP under the 2-VM harness). |

## Workflow checklist

1. Extend userspace tests first and run `cargo test --workspace` until green.
2. Execute Miri for host-compatible crates.
3. Refresh Golden Vectors (IDL frames, ABI structs) and bump SemVer when contracts change.
4. Rebuild the Podman development container (`podman build -t open-nexus-os-dev -f podman/Containerfile`) so host tooling matches CI.
5. **Run OS build hygiene checks**: `just diag-os` and `just dep-gate` (catches forbidden dependencies).
6. Run OS smoke coverage via QEMU: `just test-os` (bounded by `RUN_TIMEOUT`, exits on readiness markers).

## Scaffold sanity

Run the QEMU smoke test to confirm the UART marker sequence reaches
`SELFTEST: e2e exec-elf ok`. Keep `RUN_UNTIL_MARKER=1` to exit early once markers are
seen and ensure log caps are in effect. `just test-os` wraps
`scripts/qemu-test.sh`, so the same command exercises the minimal exec path.

### Just targets

- Host unit/property: `just test-host`
- Host E2E: `just test-e2e` (runs `nexus-e2e`, `remote_e2e`, `logd-e2e`, `vfs-e2e`, `e2e_policy`)
- QEMU smoke: `RUN_UNTIL_MARKER=1 just test-os`

### Miri tiers

- Strict (no IO): run on crates without filesystem/network access.
  - Example: `just miri-strict` (uses `MIRIFLAGS='--cfg nexus_env="host"'`).
- FS-enabled: for crates that legitimately touch the filesystem or env.
  - Example: `just miri-fs` (uses `MIRIFLAGS='-Zmiri-disable-isolation --cfg nexus_env="host"'`).
- Under `#[cfg(miri)]`, keep property tests lightweight (lower case count, disable persistence) to avoid long runtimes.

## OS-E2E marker sequence and VMO split

The os-lite `nexus-init` backend is responsible for announcing service
bring-up. The OS smoke path emits a deterministic sequence of UART markers that
the runner validates in order:

1. `neuron vers.` – kernel banner
2. `init: start` – init process begins bootstrapping services
3. `init: start keystored` – os-lite init launching the keystore daemon
4. `init: up keystored` – os-lite init observed the keystore daemon reach readiness
5. `init: start policyd` – os-lite init launching the policy daemon
6. `init: up policyd` – os-lite init observed the policy daemon reach readiness
7. `init: start samgrd` – os-lite init launching the service manager
8. `init: up samgrd` – os-lite init observed the service manager reach readiness
9. `init: start bundlemgrd` – os-lite init launching the bundle manager
10. `init: up bundlemgrd` – os-lite init observed the bundle manager reach readiness
11. `init: start packagefsd` – os-lite init launching the package FS daemon
12. `init: up packagefsd` – os-lite init observed the package FS daemon reach readiness
13. `init: start vfsd` – os-lite init launching the VFS dispatcher
14. `init: up vfsd` – os-lite init observed the VFS dispatcher reach readiness
15. `init: start execd` – os-lite init launching the exec daemon
16. `init: up execd` – os-lite init observed the exec daemon reach readiness
17. `init: ready` – init completed baseline bring-up
18. `keystored: ready` – keystore daemon ready (emitted by service process)
19. `policyd: ready` – policy daemon ready
20. `samgrd: ready` – service manager daemon ready
21. `bundlemgrd: ready` – bundle manager daemon ready
22. `packagefsd: ready` – package file system daemon registered
23. `vfsd: ready` – VFS dispatcher ready to serve requests
24. `execd: ready` – exec daemon ready (may be stubbed during bring-up)
25. `execd: elf load ok` – exec syscall mapped the embedded ELF into the child address space
26. `child: hello-elf` – spawned task from the ELF payload started and yielded control back to the kernel
27. `SELFTEST: e2e exec-elf ok` – selftest client observed the ELF loader path end-to-end
28. `child: exit0 start` – demo payload exercising `exit(0)` began execution
29. `execd: child exited pid=… code=0` – selftest client observed the exit status (marker name kept for compatibility)
30. `SELFTEST: child exit ok` – selftest client observed the lifecycle markers
31. `SELFTEST: policy allow ok` – simulated allow path succeeded via policy check
32. `SELFTEST: policy deny ok` – simulated denial path emitted for `demo.testsvc`
33. `SELFTEST: ipc payload roundtrip ok` – kernel IPC v1 payload copy-in/out roundtrip succeeded
34. `SELFTEST: ipc deadline timeout ok` – kernel IPC v1 deadline semantics: past deadline returns `TimedOut` deterministically
35. `SELFTEST: nexus-ipc kernel loopback ok` – `nexus-ipc` OS backend exercised kernel IPC v1 syscalls (loopback on bootstrap endpoint)
36. `SELFTEST: ipc routing ok` – `nexus-ipc` resolved a named service target via the init-lite routing responder
37. `SELFTEST: vfs stat ok` – VFS over IPC: stat succeeded
38. `SELFTEST: vfs read ok` – VFS over IPC: open/read succeeded
39. `SELFTEST: vfs ebadf ok` – VFS over IPC: EBADF behavior verified
40. `logd: ready` – logd RAM journal ready for APPEND/QUERY/STATS
41. `SELFTEST: log query ok` – selftest client queried logd and verified records
42. `SELFTEST: core services log ok` – core services (samgrd/bundlemgrd/policyd/dsoftbusd) emit structured logs to logd (verified via bounded QUERY + STATS delta)
43. `execd: crash report pid=... code=42 name=demo.exit42` – execd observed non-zero exit and emitted crash report
44. `SELFTEST: crash report ok` – selftest client verified crash report via logd query
45. `SELFTEST: end` – concluding marker from the selftest client

### OS E2E: exec-elf (service path)

- Run `RUN_UNTIL_MARKER=1 just test-os` to exercise the service-integrated loader flow on QEMU.
- Confirm the UART log contains `execd: elf load ok`, `child: hello-elf`, the lifecycle markers (`child: exit0 start`, `execd: child exited pid=… code=0`, `SELFTEST: child exit ok`), and `SELFTEST: e2e exec-elf ok` before ending the run.
- Host coverage mirrors the flow via `cargo test -p nexus-loader` for loader unit tests and the bundle manager IPC round-trips in
  `cargo test -p tests/e2e`.

## Policy E2E notes

- Host coverage lives in `tests/e2e_policy/`. The crate spins up loopback
  instances of `policyd`, `bundlemgrd`, and `execd`, installs two manifests, and
  asserts both the allow and deny responses along with the returned missing
  capabilities.
- The OS run mirrors this by loading policies at boot and printing
  `SELFTEST: policy allow ok` / `SELFTEST: policy deny ok` markers from
  `selftest-client` once the simulated policy checks succeed.
- Policies are stored under `recipes/policy/`. Merge order is lexical; later
  files override earlier definitions. For development overrides drop a
  `local-*.toml` file so it sorts after `base.toml`.

Cap'n Proto remains a userland concern. Large payloads (e.g. bundle artifacts) are transferred via VMO handles on the OS; on the host these handles are emulated by staging bytes in the bundle manager's artifact store before issuing control-plane requests.

## Remote E2E harness

The remote harness in `tests/remote_e2e` proves that two host nodes can discover
each other, authenticate sessions using Noise XK, and forward Cap'n Proto
traffic over the encrypted stream. Each node hosts real `samgrd` and
`bundlemgrd` loops via in-process IPC, while the identity keys are derived using
the shared `userspace/identity` crate. Artifact transfers are staged over a
dedicated DSoftBus channel before issuing the install request, mirroring the VMO
hand-off the OS build will use later. Execute the tests with
`cargo test -p remote_e2e`—they finish in a few seconds and require no QEMU. (Note: the DSoftBus OS backend is stubbed until kernel networking is available.)

## Environment parity & prerequisites

- Toolchain pinned via `rust-toolchain.toml`; install the listed version before building.
- Targets required: `rustup target add riscv64imac-unknown-none-elf`.
- System dependencies: `qemu-system-misc`, `capnproto`, and supporting build packages. The Podman container image installs the same dependencies for CI parity.
- Do not rely on host-only tools—update `recipes/` or container definitions when new packages are needed.

## Security testing

Security-relevant code requires additional testing beyond functional tests. See `docs/standards/SECURITY_STANDARDS.md` for full guidelines.

### Negative case tests (required for security code)

Security code MUST include tests that verify rejection of invalid/malicious inputs:

```rust
// Pattern: test_reject_* functions
#[test]
fn test_reject_identity_mismatch() {
    // Attempt auth with wrong key → verify rejection
    let result = handshake_with_wrong_key();
    assert!(result.is_err());
}

#[test]
fn test_reject_oversized_input() {
    let oversized = vec![0u8; MAX_SIZE + 1];
    let result = parse(&oversized);
    assert!(result.is_err());
}
```

Run security-specific tests:

```bash
# All reject tests across workspace
cargo test -- reject --nocapture

# Specific crate security tests
cargo test -p dsoftbus -- reject
cargo test -p keystored -- reject
cargo test -p nexus-sel -- reject
```

### Hardening markers (QEMU)

Security behavior must be verifiable via QEMU markers that prove enforcement:

| Marker | Meaning |
| --- | --- |
| `dsoftbusd: auth ok` | Handshake + identity binding succeeded |
| `dsoftbusd: identity mismatch peer=<id>` | Identity binding enforcement works |
| `dsoftbusd: announce ignored (malformed)` | Input validation works |
| `policyd: deny (subject=<svc> action=<op>)` | Policy deny-by-default works |
| `policyd: allow (subject=<svc> action=<op>)` | Explicit allow logged |
| `keystored: sign denied (subject=<svc>)` | Policy-gated signing works |

### Fuzz testing (recommended for parsers)

For parsing and protocol code:

```bash
# Install cargo-fuzz if needed
cargo install cargo-fuzz

# Run fuzz targets (if available)
cargo +nightly fuzz run fuzz_discovery_packet
cargo +nightly fuzz run fuzz_noise_handshake
cargo +nightly fuzz run fuzz_policy_parser
```

### Security review checklist

Before merging security-relevant PRs, verify:

- [ ] No secrets in logs, markers, or error messages
- [ ] Test keys labeled `// SECURITY: bring-up test keys`
- [ ] `sender_service_id` used (not payload strings) for identity
- [ ] Inputs bounded (max sizes enforced)
- [ ] No `unwrap`/`expect` on untrusted data
- [ ] Audit records produced for security decisions
- [ ] Negative case tests (`test_reject_*`) included

## Build hygiene (OS targets)

Before committing OS-related changes, run these validation gates:

| Command | Purpose |
| --- | --- |
| `just diag-os` | Check all OS services compile for `riscv64imac-unknown-none-elf` |
| `just dep-gate` | **Critical**: Fail if forbidden crates (`parking_lot`, `getrandom`) appear in OS graph |
| `just diag-host` | Check host builds compile cleanly |

### The `dep-gate` rule

OS services **must** be built with `--no-default-features --features os-lite`. Without these flags, `std`-only dependencies leak into the bare-metal build and cause cryptic errors like:

```text
error: can't find crate for `std`
  --> parking_lot_core/src/lib.rs
```

The `just dep-gate` command checks the dependency graph and **fails the build** if forbidden crates appear:

```bash
# Run before any OS commit
just dep-gate

# If it fails, check which crate pulled in the forbidden dependency:
cargo tree --target riscv64imac-unknown-none-elf -p dsoftbusd -i parking_lot
```

**See also**: `docs/standards/BUILD_STANDARDS.md` for the full feature gate convention.

## House rules

- No `unwrap`/`expect` in daemons; propagate errors with context.
- Userspace crates must keep `#![forbid(unsafe_code)]` enabled and pass Clippy's denied lints.
- No blanket `#[allow(dead_code)]` or `#[allow(unused)]`. Use the `tools/deadcode-scan.sh` guard, gate WIP APIs behind features, or add time-boxed entries to `config/deadcode.allow`.
- CI enforces architecture guards, UART markers, and formatting; keep commits green locally before pushing.
- **OS builds must pass `just dep-gate`** to ensure no `std`-only crates leak into bare-metal targets.

## Troubleshooting tips

- QEMU runs are bounded by the `RUN_TIMEOUT` environment variable (default `45s`). Increase it only when debugging: `RUN_TIMEOUT=120s just qemu`. During kernel bring-up we rely on marker-driven early exit and strict triage: the kernel prints `map kernel segments ok` once linker-derived mappings are active and `AS: post-satp OK` after each SATP switch; illegal-instruction traps print `sepc/scause/stval` and instruction bytes; optional symbolization (`trap_symbols`) resolves `sepc` to `name+offset`; RX-sanity failures panic with the offending PC before the switch.
- CI log filters fail the run on `PANIC`, `EXC:`, `ILLEGAL`, `rx guard:`, or missing marker sequences and retain deterministic seeds plus the trimmed UART/QEMU artefacts for post-mortem analysis.
- Logs are trimmed post-run. Override caps with `QEMU_LOG_MAX` or `UART_LOG_MAX` if you need to preserve more context. Each run drops three auto-generated artefacts (all ignored by Git) in the workspace root: `uart.log`, `uart_ecall_chars.log`, and `neuron-boot.map`. Keep the map around while triaging traps—agents rely on it to resolve `sepc` offsets without rerunning the build. Use `tools/uart-filter.py --strip-escape uart.log` to decode the escape-prefixed probe lines or `--grep init:` to focus on readiness markers.
- Enable marker-driven early exit for faster loops by setting `RUN_UNTIL_MARKER=1` (already defaulted in `just test-os`). Logs appear as `qemu.log` (diagnostics) and `uart.log` (console output) in the working directory. Set `QEMU_TRACE=1` and optionally `QEMU_TRACE_FLAGS=in_asm,int,mmu,unimp` to capture detailed traces while debugging. Enable the `trap_ring` feature to retain the last 64 trap frames in debug builds or `trap_symbols` to annotate `sepc` with `name+offset` when triaging crashes. `scripts/run-qemu-rv64.sh` accepts `NEURON_BOOT_FEATURES=trap_ring,trap_symbols` so you can flip those flags per-run without touching manifests; the helper passes them into the kernel build before QEMU launches. Kernel panic/guard prints remain unconditional (mirroring seL4/Fuchsia panic channels) until we finish the current KPGF work; once the loader is stable we plan to gate them behind a `panic_uart` feature and rely on the trap ring for routine post-mortems.
- Init-lite tags its verbose traces with RFC‑0003-style topics. Export `INIT_LITE_LOG_TOPICS=svc-meta` (comma-separated) before invoking `just qemu`/`just test-os` to opt into additional channels such as the service metadata probes. The build script records the value via `cargo:rustc-env`, so changing it automatically triggers the rebuild already performed by `scripts/run-qemu-rv64.sh`.
- Init-lite emits two unconditional bring-up markers for triage: `!init-lite entry` before any service work and `!init-lite ready` after all configured services spawn. Fatal bootstrap errors log a `fatal err=…` line and then panic the init task so the failure is visible in UART.
- To catch silent spins during bring-up, you can enable a watchdog that panics after N cooperative yields: `INIT_LITE_WATCHDOG_TICKS=500 RUN_TIMEOUT=10s just qemu`. Leave it unset for normal runs.
- Escape-coded UART (E-prefixed) is currently forced off to keep logs clean. Old logs can still be decoded with `tools/uart-filter.py --strip-escape uart.log`. If you need probe framing, add it locally in code, but keep it disabled for shared runs.
- For stubborn host/container mismatches, rebuild the Podman image and ensure the same targets are installed inside and outside the container.

### Capturing OS networking traffic (PCAP / Wireshark)

When debugging cross-VM networking issues (ARP/UDP/TCP handshakes), it is often fastest to capture the QEMU network traffic into PCAP files and inspect them with Wireshark/tshark.

- **2-VM harness with PCAP**:

```bash
RUN_OS2VM=1 RUN_TIMEOUT=180s OS2VM_PCAP=1 tools/os2vm.sh
```

- **Outputs**: `os2vm-A.pcap` and `os2vm-B.pcap` (in the current directory unless `LOG_DIR` is set).
- **Inspect**: open the PCAPs in Wireshark and filter on `arp`, `icmp`, `udp.port==37020`, or `tcp.port==34567`.

## Networking testing & debugging strategy (host-first, OS-last)

Networking issues are notoriously easy to “fake-green” (e.g. a connect syscall returns OK but no packets are ever emitted). For Open Nexus OS we treat networking as a **layered proof problem** with deterministic markers and opt-in deep capture.

### Goals

- **Fast feedback**: most protocol logic is proven on host first.
- **Realism only where needed**: QEMU is used to validate end-to-end wiring and on-wire behavior.
- **No fake success**: markers must correspond to *observable* behavior (packets, state transitions), not “we called a function”.
- **Determinism**: bounded loops, stable markers, stable seeds.

### Layered proof ladder (recommended)

- **Host (contract tests)**:
  - Parse/encode of wire formats (golden vectors).
  - State machine stepping (Noise handshake, discovery cache, session framing) using host backends / fakes.
  - Negative cases (`test_reject_*`): malformed frames, oversized input, identity mismatch.
- **OS (unit smoke)**:
  - `netstackd` facade health: interface up, socket bind/listen works.
  - **L2**: ARP request/response observed (PCAP or bounded in-OS marker derived from real traffic).
  - **L3**: ICMP echo where applicable (DHCP/usernet path); otherwise skip deterministically.
  - **L4/UDP**: discovery announce/recv with real datagrams under 2-VM harness (no loopback shortcuts).
  - **L4/TCP**: SYN/SYN-ACK observed (PCAP) and accept/connect completes; only then run higher layers.
- **OS (integration)**:
  - DSoftBus sessions + Noise auth + identity binding.
  - Remote proxy allowlist (`samgrd` / `bundlemgrd`) with bounded frames and deny-by-default behavior.

### Debugging toolbox (recommended order)

1) **PCAP capture (best ROI)** via the 2-VM harness (`OS2VM_PCAP=1`). This proves whether ARP/UDP/TCP packets are actually emitted and received.
2) **Bounded OS diagnostics** (markers or one-shot logs) to narrow failures without spamming UART:
   - “entered SYN-SENT” vs. “TX emitted SYN”
   - “accept returned WOULD_BLOCK” vs. “accept OK”
3) **Host reproduction**: reduce the failing OS scenario into a host test or a small deterministic harness.

### How to avoid fake-green markers

- Prefer markers that reflect **external observables**:
  - “pcap contains TCP SYN” is stronger than “connect() returned OK”.
  - “received announce from `<peer>`” is stronger than “announce sent”.
- If a marker is necessarily internal (e.g. “state entered SYN-SENT”), label it as such and pair it with an on-wire proof marker.

### Future log hygiene

Once the outstanding kernel guard fault is root-caused we’ll align the UART policy with other production kernels:

- **Compile-time silencing (seL4-style):** panic/guard prints move behind a `panic_uart` feature, with CI builds using the trap ring + symbolization instead of raw UART spam.
- **Runtime `klog` topics (Fuchsia-style):** the kernel will emit severity/target-tagged records into a ring buffer, and only mirror them to UART when `kernel.serial`-style boot args (or our `INIT_LITE_LOG_TOPICS`) request it.
- **Strict single-path boot (OHOS-style):** redundant “init-lite vs. shared loader” shims are already gone; the next phase deletes any legacy probes that stay quiet for a full regression run so unexpected noise can’t creep back in.

Keep these knobs in mind when adding diagnostics—new prints should either use `nexus_log` topics or the kernel trap ring so we can flip the switch once the bug hunt ends.

### Init-lite logging topics

`init-lite` currently recognises the following topics:

| Topic | Description |
| --- | --- |
| `general` | Default logs (always enabled). |
| `svc-meta` | Service metadata probes (`svc meta …` traces, guard diagnostics). |
| `probe` | Loader/allocator instrumentation and raw guard telemetry. Disabled by default to keep UART noise down. |

Example:

```bash
INIT_LITE_LOG_TOPICS=general,svc-meta,probe RUN_UNTIL_MARKER=1 just test-os
```

Because the knob is evaluated at build time, you only need to export it before running the Just/QEMU helper; the script’s rebuild step will pick it up automatically.

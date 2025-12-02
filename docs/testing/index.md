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

| Layer | Scope | Command | Notes |
| --- | --- | --- | --- |
| Host E2E (`tests/e2e`) | In-process loopback using real Cap'n Proto handlers for `samgrd` and `bundlemgrd`. | `cargo test -p nexus-e2e` or `just test-e2e` | Deterministic and fast. Uses the same userspace libs as the OS build without QEMU. |
| Host init smoke | Runs `nexus-init` on host, asserts real daemon readiness and `*: up` markers. | `just test-init` or `make test-init-host` | Exits early on `init: ready` and enforces ordered readiness. |
| Remote E2E (`tests/remote_e2e`) | Two in-process nodes exercising DSoftBus-lite discovery, Noise-authenticated sessions, and remote bundle installs. | `cargo test -p remote_e2e` or `just test-e2e` | Spins up paired `identityd`, `samgrd`, `bundlemgrd`, and DSoftBus-lite daemons sharing the host registry. |
| Policy E2E (`tests/e2e_policy`) | Loopback `policyd`, `bundlemgrd`, and `execd` exercising allow/deny paths. | `cargo test -p e2e_policy` | Installs manifests for `samgrd` and `demo.testsvc`, asserts capability allow/deny responses. |
| Host VFS (`tests/vfs_e2e`) | In-process `packagefsd`, `vfsd`, and `bundlemgrd` validating bundle publication and VFS reads. | `cargo test -p vfs-e2e` | Publishes a demo bundle, checks alias resolution, and verifies read/error paths via `nexus-vfs`. |
| QEMU smoke (`scripts/qemu-test.sh`) | Kernel selftests plus service readiness markers. | `RUN_UNTIL_MARKER=1 just test-os` | Kernel-only path enforces UART sequence: banner → `SELFTEST: begin` → `SELFTEST: time ok` → `KSELFTEST: spawn ok` → `SELFTEST: end`. With services enabled, os-lite `nexus-init` is the default bootstrapper; the harness now waits for each `init: start <svc>` / `init: up <svc>` pair in addition to `execd: elf load ok`, `child: hello-elf`, `SELFTEST: e2e exec-elf ok`, the exit lifecycle trio (`child: exit0 start`, `execd: child exited pid=… code=0`, `SELFTEST: child exit ok`), the policy allow/deny probes, and the VFS checks before stopping. Logs are trimmed to keep artefacts small. |

## Workflow checklist
1. Extend userspace tests first and run `cargo test --workspace` until green.
2. Execute Miri for host-compatible crates.
3. Refresh Golden Vectors (IDL frames, ABI structs) and bump SemVer when contracts change.
4. Rebuild the Podman development container (`podman build -t open-nexus-os-dev -f podman/Containerfile`) so host tooling matches CI.
5. Run OS smoke coverage via QEMU: `just test-os` (bounded by `RUN_TIMEOUT`, exits on readiness markers).

## Scaffold sanity

Run the QEMU smoke test to confirm the UART marker sequence reaches
`SELFTEST: e2e exec-elf ok`. Keep `RUN_UNTIL_MARKER=1` to exit early once markers are
seen and ensure log caps are in effect. `just test-os` wraps
`scripts/qemu-test.sh`, so the same command exercises the minimal exec path.

### Just targets

- Host unit/property: `just test-host`
- Host E2E: `just test-e2e` (runs `nexus-e2e` and `remote_e2e`)
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
5. `keystored: ready` – key store stub ready
6. `init: start policyd` – os-lite init launching the policy daemon
7. `init: up policyd` – os-lite init observed the policy daemon reach readiness
8. `policyd: ready` – policy stub ready
9. `init: start samgrd` – os-lite init launching the service manager
10. `init: up samgrd` – os-lite init observed the service manager reach readiness
11. `samgrd: ready` – service manager daemon ready
12. `init: start bundlemgrd` – os-lite init launching the bundle manager
13. `init: up bundlemgrd` – os-lite init observed the bundle manager reach readiness
14. `bundlemgrd: ready` – bundle manager daemon ready
15. `init: start packagefsd` – os-lite init launching the package FS daemon
16. `init: up packagefsd` – os-lite init observed the package FS daemon reach readiness
17. `packagefsd: ready` – package file system daemon registered
18. `init: start vfsd` – os-lite init launching the VFS dispatcher
19. `init: up vfsd` – os-lite init observed the VFS dispatcher reach readiness
20. `vfsd: ready` – VFS dispatcher ready to serve requests
21. `init: start execd` – os-lite init launching the exec daemon
22. `init: up execd` – os-lite init observed the exec daemon reach readiness
23. `init: ready` – init completed baseline bring-up
24. `execd: elf load ok` – loader mapped the embedded ELF into the child address space
25. `child: hello-elf` – spawned task from the ELF payload started and yielded control back to the kernel
26. `SELFTEST: e2e exec-elf ok` – selftest client observed the ELF loader path end-to-end
27. `child: exit0 start` – demo payload exercising `exit(0)` began execution
28. `execd: child exited pid=… code=0` – supervisor reaped the exit0 child and logged its status
29. `SELFTEST: child exit ok` – selftest client observed the lifecycle markers from execd
30. `SELFTEST: policy allow ok` – simulated allow path succeeded via policy check
31. `SELFTEST: policy deny ok` – simulated denial path emitted for `demo.testsvc`
32. `SELFTEST: vfs stat ok` – selftest exercised read-only stat via the userspace VFS
33. `SELFTEST: vfs read ok` – selftest read bundle payload bytes via the VFS client
34. `SELFTEST: vfs ebadf ok` – selftest confirmed closed handles are rejected by the VFS
35. `SELFTEST: end` – concluding marker from the host-side selftest client

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

## House rules
- No `unwrap`/`expect` in daemons; propagate errors with context.
- Userspace crates must keep `#![forbid(unsafe_code)]` enabled and pass Clippy’s denied lints.
- No blanket `#[allow(dead_code)]` or `#[allow(unused)]`. Use the `tools/deadcode-scan.sh` guard, gate WIP APIs behind features, or add time-boxed entries to `config/deadcode.allow`.
- CI enforces architecture guards, UART markers, and formatting; keep commits green locally before pushing.

## Troubleshooting tips
- QEMU runs are bounded by the `RUN_TIMEOUT` environment variable (default `45s`). Increase it only when debugging: `RUN_TIMEOUT=120s just qemu`. During kernel bring-up we rely on marker-driven early exit and strict triage: the kernel prints `map kernel segments ok` once linker-derived mappings are active and `AS: post-satp OK` after each SATP switch; illegal-instruction traps print `sepc/scause/stval` and instruction bytes; optional symbolization (`trap_symbols`) resolves `sepc` to `name+offset`; RX-sanity failures panic with the offending PC before the switch.
- CI log filters fail the run on `PANIC`, `EXC:`, `ILLEGAL`, `rx guard:`, or missing marker sequences and retain deterministic seeds plus the trimmed UART/QEMU artefacts for post-mortem analysis.
- Logs are trimmed post-run. Override caps with `QEMU_LOG_MAX` or `UART_LOG_MAX` if you need to preserve more context. Each run drops three auto-generated artefacts (all ignored by Git) in the workspace root: `uart.log`, `uart_ecall_chars.log`, and `neuron-boot.map`. Keep the map around while triaging traps—agents rely on it to resolve `sepc` offsets without rerunning the build. Use `tools/uart-filter.py --strip-escape uart.log` to decode the escape-prefixed probe lines or `--grep init:` to focus on readiness markers.
- Enable marker-driven early exit for faster loops by setting `RUN_UNTIL_MARKER=1` (already defaulted in `just test-os`). Logs appear as `qemu.log` (diagnostics) and `uart.log` (console output) in the working directory. Set `QEMU_TRACE=1` and optionally `QEMU_TRACE_FLAGS=in_asm,int,mmu,unimp` to capture detailed traces while debugging. Enable the `trap_ring` feature to retain the last 64 trap frames in debug builds or `trap_symbols` to annotate `sepc` with `name+offset` when triaging crashes.
- Init-lite tags its verbose traces with RFC‑0003-style topics. Export `INIT_LITE_LOG_TOPICS=svc-meta` (comma-separated) before invoking `just qemu`/`just test-os` to opt into additional channels such as the service metadata probes. The build script records the value via `cargo:rustc-env`, so changing it automatically triggers the rebuild already performed by `scripts/run-qemu-rv64.sh`.
- For stubborn host/container mismatches, rebuild the Podman image and ensure the same targets are installed inside and outside the container.

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

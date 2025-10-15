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
| QEMU smoke (`scripts/qemu-test.sh`) | Kernel selftests plus service readiness markers. | `RUN_UNTIL_MARKER=1 just test-os` | Kernel-only path enforces UART sequence: banner → `SELFTEST: begin` → `SELFTEST: time ok` → `KSELFTEST: spawn ok` → `SELFTEST: end`. When services run, the harness waits for `execd: spawn ok`, `child: hello`, and `SELFTEST: e2e exec ok` before stopping. Logs are trimmed to keep artefacts small. |

## Workflow checklist
1. Extend userspace tests first and run `cargo test --workspace` until green.
2. Execute Miri for host-compatible crates.
3. Refresh Golden Vectors (IDL frames, ABI structs) and bump SemVer when contracts change.
4. Rebuild the Podman development container (`podman build -t open-nexus-os-dev -f podman/Containerfile`) so host tooling matches CI.
5. Run OS smoke coverage via QEMU: `just test-os` (bounded by `RUN_TIMEOUT`, exits on readiness markers).

## Scaffold sanity

Run the QEMU smoke test to confirm the UART marker sequence reaches
`SELFTEST: e2e exec ok`. Keep `RUN_UNTIL_MARKER=1` to exit early once markers are
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

The OS smoke path emits a deterministic sequence of UART markers that the runner validates in order:

1. `neuron vers.` – kernel banner
2. `init: start` – init process begins bootstrapping services
3. `keystored: ready` – key store stub ready
4. `policyd: ready` – policy stub ready
5. `samgrd: ready` – service manager daemon ready
6. `bundlemgrd: ready` – bundle manager daemon ready
7. `init: ready` – init completed baseline bring-up
8. `execd: spawn ok` – minimal spawn syscall succeeded for the hello payload
9. `child: hello` – spawned task started and yielded control back to the kernel
10. `SELFTEST: e2e exec ok` – selftest client observed the minimal exec path end-to-end
11. `SELFTEST: policy allow ok` – simulated allow path succeeded via policy check
12. `SELFTEST: policy deny ok` – simulated denial path emitted for `demo.testsvc`
13. `SELFTEST: end` – concluding marker from the host-side selftest client

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
- Logs are trimmed post-run. Override caps with `QEMU_LOG_MAX` or `UART_LOG_MAX` if you need to preserve more context.
- Enable marker-driven early exit for faster loops by setting `RUN_UNTIL_MARKER=1` (already defaulted in `just test-os`). Logs appear as `qemu.log` (diagnostics) and `uart.log` (console output) in the working directory. Set `QEMU_TRACE=1` and optionally `QEMU_TRACE_FLAGS=in_asm,int,mmu,unimp` to capture detailed traces while debugging. Enable the `trap_ring` feature to retain the last 64 trap frames in debug builds or `trap_symbols` to annotate `sepc` with `name+offset` when triaging crashes.
- For stubborn host/container mismatches, rebuild the Podman image and ensure the same targets are installed inside and outside the container.

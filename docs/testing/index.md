<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Testing methodology

Open Nexus OS follows a **host-first, OS-last** strategy. Most logic is exercised with fast host tools, leaving QEMU for end-to-end smoke coverage only. This document explains the layers, expectations, and day-to-day workflow for contributors.

**Related RFCs:**
- **RFC-0013**: Boot gates v1 — readiness contract + spawn failure reasons (Complete)
- **RFC-0014**: Testing contracts v1 — host-first service contract tests + phased QEMU smoke (Complete)
- **RFC-0015**: Policy Authority & Audit Baseline v1 — policy engine + audit trail (Complete)
- **RFC-0017**: Device MMIO access model v1 — capability-gated MMIO mapping + init/policy distribution (Done)
- **RFC-0019**: IPC request/reply correlation v1 — nonce correlation + deterministic QEMU virtio-mmio policy (Complete)
- **RFC-0031**: Crashdumps v1 — deterministic in-process minidumps + host symbolization (Complete)
- **RFC-0038**: Selftest-client production-grade deterministic test architecture refactor + manifest/evidence/replay v1 — proof-manifest SSOT, signed evidence bundles, replay/bisect tooling (Done; one environmental closure step remaining — external CI-runner replay artifact for P6-05, see `docs/testing/replay-and-bisect.md` §7-§11)
- **RFC-0046**: UI v1a host CPU renderer + deterministic snapshots — BGRA8888 host renderer, deterministic goldens, reject tests, and fake-marker prohibition (Done; `TASK-0054` Done)
- **RFC-0047**: UI v1b windowd surface/layer/present — headless `windowd` compositor contract, bounded surface/VMO/layer rejects, generated Cap'n Proto roundtrips, and honest QEMU markers (Done; `TASK-0055` Done)
- **RFC-0048**: UI v1c visible QEMU scanout bootstrap — fixed-mode QEMU `ramfb`, capability-gated `fw_cfg` setup, visible marker ladder, and harness-profile proof (Done; `TASK-0055B` Done)

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
| QEMU smoke (`scripts/qemu-test.sh`) | Kernel selftests plus service readiness and crashdump v1 markers. | `RUN_UNTIL_MARKER=1 just test-os` | Kernel-only path enforces UART sequence: banner → `SELFTEST: begin` → `SELFTEST: time ok` → `KSELFTEST: spawn ok` → `SELFTEST: end`. With services enabled, os-lite `nexus-init` is the default bootstrapper; the harness waits for each `init: start <svc>` / `init: up <svc>` pair in addition to `execd: elf load ok`, `child: hello-elf`, `SELFTEST: e2e exec-elf ok`, the exit lifecycle trio (`child: exit0 start`, `execd: child exited pid=… code=0`, `SELFTEST: child exit ok`), crashdump v1 markers (`execd: minidump written`, `SELFTEST: minidump ok`, negative reject markers), policy probes, and VFS checks before stopping. Logs are trimmed to keep artefacts small. |
| QEMU 2-VM opt-in (`tools/os2vm.sh`) | Two QEMU instances exercising cross-VM DSoftBus discovery, Noise-authenticated session establishment, and remote proxy (`samgrd`/`bundlemgrd` + TASK-0016 remote packagefs RO marker ladder). | `RUN_OS2VM=1 RUN_TIMEOUT=180s OS2VM_PROFILE=ci RUN_PHASE=end tools/os2vm.sh` | Canonical proof for TASK-0005/TASK-0016. Uses run-scoped artifacts under `artifacts/os2vm/runs/<runId>/` with retention/GC and typed failure classification. |

### TASK-0020 mux v2 requirement matrix

`TASK-0020` uses requirement-based host tests (not phase-named files). Host suites are authoritative and OS mux-marker closure is now enforced via `REQUIRE_DSOFTBUS=1`.

| Requirement surface | Test file | Canonical command |
| --- | --- | --- |
| Reject taxonomy, bounded flow-control/backpressure, mixed-priority starvation bounds, naming rejects | `userspace/dsoftbus/tests/mux_contract_rejects_and_bounds.rs` | `cargo test -p dsoftbus --test mux_contract_rejects_and_bounds -- --nocapture` |
| Frame/state transitions, keepalive semantics, seeded state-machine accounting invariants, idempotent RST contract | `userspace/dsoftbus/tests/mux_frame_state_keepalive_contract.rs` | `cargo test -p dsoftbus --test mux_frame_state_keepalive_contract -- --nocapture` |
| Open/accept/data/rst endpoint integration, duplicate-name reject, unauthenticated fail-closed, teardown rejects | `userspace/dsoftbus/tests/mux_open_accept_data_rst_integration.rs` | `cargo test -p dsoftbus --test mux_open_accept_data_rst_integration -- --nocapture` |
| Requirement-suite aggregate | n/a | `just test-dsoftbus-mux` |
| Full package regression | n/a | `just test-dsoftbus-host` (or `cargo test -p dsoftbus -- --nocapture`) |
| Mandatory slice regressions | n/a | `just test-e2e` and `just test-os-dhcp` |

OS and distributed evidence:
- `REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s ./scripts/qemu-test.sh` (includes `dsoftbus:mux ...` and `SELFTEST: mux ...` ladder)
- `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh` with reviewed summaries under `artifacts/os2vm/runs/<runId>/summary.{json,txt}` plus `release-evidence.json` (includes explicit `phase: mux` cross-VM ladder, `phase: perf` deterministic budget gate, and `phase: soak` hardening gate)
- For `RFC-0034` legacy-closure distributed claims (`TASK-0001..0020`), review `release-evidence.json` as a mandatory artifact alongside summaries.

### TASK-0021 QUIC scaffold behavior-first matrix

`TASK-0021` remains the host-first QUIC contract baseline.

| Requirement surface | Proof type | Canonical command |
| --- | --- | --- |
| Host real QUIC connect + bidirectional stream exchange + TASK-0020 mux smoke payload + reject mapping | host transport assertions | `cargo test -p dsoftbus --test quic_host_transport_contract -- --nocapture` |
| Host QUIC selection + reject/fallback contract (`test_reject_*` + positive path) | host selection assertions | `cargo test -p dsoftbus --test quic_selection_contract -- --nocapture` |
| OS QUIC marker wiring (`dsoftbusd: transport selected quic`, `dsoftbusd: auth ok`, `SELFTEST: quic session ok`) plus fallback-marker rejection | single-VM boundary marker proof | `REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=220s ./scripts/qemu-test.sh` |
| Deterministic selection perf envelope (bounded attempts/marker budget) | host perf-budget assertions | `cargo test -p dsoftbus --test quic_selection_contract perf_budget -- --nocapture` |

Scope note:
- 2-VM proof is only required when `TASK-0021` claims new distributed behavior; do not run `tools/os2vm.sh` for completeness-only reasons.
- Convenience aggregate for host QUIC scope: `just test-dsoftbus-quic`.

### TASK-0022 no_std core abstraction matrix

`TASK-0022` closure uses host-first deterministic reject/perf proofs plus explicit no_std compile evidence.

| Requirement surface | Proof type | Canonical command |
| --- | --- | --- |
| Core crate no_std compatibility (`dsoftbus-core`) | target compile proof | `cargo +nightly-2025-01-15 check -p dsoftbus-core --target riscv64imac-unknown-none-elf` |
| Required security reject paths (`test_reject_*`) | host reject assertions | `cargo test -p dsoftbus -- reject --nocapture` |
| Core boundary + determinism extras (`Send`/`Sync`, backpressure budget, borrow-view no-copy) | host deterministic contract assertions | `cargo test -p dsoftbus --test core_contract_rejects -- --nocapture` |
| TASK-0021 regression floor preserved | host QUIC regression assertions | `just test-dsoftbus-quic` |
| OS integration hygiene + marker ladder when hooks are touched | OS compile + single-VM proof | `just dep-gate && just diag-os && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os` |

Scope note:
- `tools/os2vm.sh` is required only when the slice asserts new distributed behavior claims.

### TASK-0023 QUIC v2 OS-enabled matrix

`TASK-0023` closes OS QUIC-v2 session enablement in the no_std-friendly UDP framing profile.

| Requirement surface | Proof type | Canonical command |
| --- | --- | --- |
| Host fail-closed selection + real-transport reject mapping (`test_reject_*`) | host gate-integrity assertions | `cargo test -p dsoftbus --test quic_selection_contract -- --nocapture` and `cargo test -p dsoftbus --test quic_host_transport_contract -- --nocapture` |
| Phase-D feasibility reject/boundedness contract (`test_reject_quic_feasibility_*`) | host feasibility assertions | `cargo test -p dsoftbus --test quic_feasibility_contract -- --nocapture` |
| Service-side QUIC frame reject paths (`test_reject_quic_frame_*`) | host service contract assertions | `cargo test -p dsoftbusd --test p0_unit -- --nocapture` |
| Host aggregate regression floor for QUIC semantics | host aggregate assertions | `just test-dsoftbus-quic` |
| OS QUIC marker contract (`dsoftbusd: transport selected quic`, `dsoftbusd: auth ok`, `dsoftbusd: os session ok`, `SELFTEST: quic session ok`) with fallback-marker rejection | single-VM boundary marker proof | `REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=220s ./scripts/qemu-test.sh` |

Scope note:
- `tools/os2vm.sh` remains required only when new distributed behavior is explicitly claimed.
- Advanced QUIC tuning/perf breadth remains follow-up scope (`TASK-0044`).

### TASK-0029 supply-chain v1 matrix

`TASK-0029` closes host-first supply-chain baseline for bundle SBOM/repro metadata and sign-policy enforcement.

| Requirement surface | Proof type | Canonical command |
| --- | --- | --- |
| Deterministic SBOM embedding (`meta/sbom.json`) | host determinism assertions | `cargo test -p sbom -- determinism` and `cargo test -p nxb-pack -- supply_chain` |
| CycloneDX contract compliance (schema + roundtrip) | host interoperability assertions | `cargo run -p nxb-pack -- --hello target/supplychain-proof` then `build/tools/cyclonedx-cli validate/convert/convert/validate` (v1.5) |
| Repro metadata capture + verify (`meta/repro.env.json`) | host schema/digest assertions | `cargo test -p repro -- verify` |
| Single-authority allowlist + install enforcement order | host authority-chain assertions | `cargo test -p keystored -- is_key_allowed` and `cargo test -p bundlemgrd -- supply_chain` |
| Mandatory deny-by-default reject paths (`test_reject_*`) | host fail-closed assertions | `cargo test -p bundlemgrd -- test_reject_unknown_publisher test_reject_unknown_key test_reject_unsupported_alg test_reject_payload_digest_mismatch test_reject_sbom_digest_mismatch test_reject_repro_digest_mismatch test_reject_sbom_secret_leak test_reject_repro_schema_invalid test_reject_audit_unreachable` |
| QEMU supply-chain marker ladder | single-VM gated marker proof | `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os supply-chain` |

### TASK-0031 zero-copy VMO plumbing matrix

`TASK-0031` is the host-first VMO plumbing and honesty floor. Production-grade kernel closure remains in `TASK-0290`.

| Requirement surface | Proof type | Canonical command |
| --- | --- | --- |
| Typed VMO API (+ `from_bytes`, `from_file_range`, `VmoSlice`) and deterministic accounting counters (copy fallback vs control/data plane bytes, map reuse hit/miss) | host contract assertions | `cargo test -p nexus-vmo -- --nocapture` |
| Deny-by-default reject paths (`test_reject_unauthorized_transfer`, `test_reject_oversized_mapping`, `test_ro_mapping_enforced`, short file-range, host slot-transfer reject) | host reject assertions | `cargo test -p nexus-vmo -- reject --nocapture` |
| Producer transfer -> spawned consumer task RO map/verify -> marker ladder (`vmo:*`, `SELFTEST: vmo share ok`) | single-VM OS-gated marker proof | `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os` |

### TASK-0032 packagefs v2 `pkgimg` matrix

`TASK-0032` proves deterministic read-only image/index mount-read behavior (host-first, OS-gated).

| Requirement surface | Proof type | Canonical command |
| --- | --- | --- |
| Deterministic `pkgimg` v2 builder/parser and required reject paths (`test_reject_pkgimg_*`) | host contract + reject assertions | `cargo test -p storage -- --nocapture` |
| `packagefsd` host integration floor stays green while wiring v2 mount/read path | host daemon assertions | `cargo test -p packagefsd -- --nocapture` |
| `pkgimg-build` host tooling compiles and remains executable in workspace | host tooling sanity | `cargo test -p pkgimg-build -- --nocapture` |
| OS marker ladder (`packagefsd: v2 mounted (pkgimg)`, `SELFTEST: pkgimg mount ok`, `SELFTEST: pkgimg stat/read ok`) | single-VM OS-gated marker proof | `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os` |

### TASK-0039 sandboxing v1 userspace confinement matrix

`TASK-0039` proves userspace-only sandboxing floor (kernel unchanged): namespace traversal rejects, CapFd fail-closed checks, and spawn-time no-direct-fs-cap discipline.

| Requirement surface | Proof type | Canonical command |
| --- | --- | --- |
| Namespace traversal and unauthorized path rejects (`test_reject_path_traversal`, `test_reject_unauthorized_namespace_path`) | host reject assertions | `cargo test -p vfsd -- --nocapture` and `cargo test -p nexus-vfs -- --nocapture` |
| CapFd authenticity/replay/rights fail-closed (`test_reject_forged_capfd`, `test_reject_replayed_capfd`, `test_reject_capfd_rights_mismatch`) | host reject assertions | `cargo test -p vfsd -- --nocapture` |
| Spawn boundary deny for direct fs-service cap bypass (`test_reject_direct_fs_cap_bypass_at_spawn_boundary`) | host authority-boundary assertion | `cargo test -p execd --lib test_reject_direct_fs_cap_bypass_at_spawn_boundary -- --nocapture` |
| OS-gated marker ladder (`vfsd: namespace ready`, `vfsd: capfd grant ok`, `vfsd: access denied`, `SELFTEST: sandbox deny ok`, `SELFTEST: capfd read ok`) | single-VM boundary marker proof | `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os` |

### Legacy TASK-0001..0020 Soll requirement test matrix (production closure)

Legacy tasks remain `Done`; production closure uses follow-on requirement suites to prove Soll behavior (not implementation internals).

| Requirement lineage (1..20) | Soll-oriented proof expectation | Canonical command(s) |
| --- | --- | --- |
| Authenticated session + identity binding (`TASK-0003B/0004/0005`) | reject unauthenticated or mismatched identity before payload/proxy actions | `cargo test -p dsoftbus -- reject --nocapture`; `RUN_UNTIL_MARKER=1 just test-os` |
| Discovery/admission boundedness (`TASK-0003C/0004`) | ACL/rate/TTL rejects and deterministic peer aging | task-owned requirement suites; `RUN_UNTIL_MARKER=1 just test-os` |
| Remote proxy authorization and deny-by-default (`TASK-0005/0016/0017`) | explicit deny markers and no unauthorized forwarding | `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh` |
| Mux/backpressure/keepalive correctness (`TASK-0020`) | deterministic reject taxonomy + bounded flow control + keepalive teardown | `just test-dsoftbus-mux`; `just test-dsoftbus-host` |
| Distributed correctness claims (`TASK-0005` lineage) | no single-VM substitution for distributed assertions | `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh` |
| Marker honesty (all 1..20 lineages) | markers validated against real protocol/state outcomes, never grep-only closure | `scripts/qemu-test.sh` expected markers + task-owned assertions |

Hard rule:
- No fake-success markers (`*: ready`, `SELFTEST: * ok`, transport/mux/media OK markers) may be used as proof unless the associated behavior is asserted by deterministic tests/harness checks.

### TASK-0054 UI host renderer snapshot matrix

`TASK-0054` is a host-only UI renderer proof floor for `RFC-0046`. It proves deterministic BGRA8888 pixels, checked renderer bounds, fixture-font text, bounded damage behavior, and golden update discipline. It does not prove OS present, `windowd`, compositor, GPU, IPC, VMO reuse, or kernel production-grade behavior.

| Requirement surface | Proof type | Canonical command |
| --- | --- | --- |
| Bounded BGRA8888 renderer core, newtypes, exact buffer length, 64-byte frame stride, deterministic damage coalescing | host renderer assertions | `cargo test -p ui_renderer -- --nocapture` |
| Pixel-exact clear/rect/rounded-rect/blit/text behavior, canonical BGRA golden comparison, PNG metadata independence | host snapshot assertions | `cargo test -p ui_host_snap -- --nocapture` |
| Oversized dimensions, invalid stride/buffer length, arithmetic/rect/image/font rejects, golden update gating, path traversal rejects, fake OS marker scan | host reject assertions | `cargo test -p ui_host_snap reject -- --nocapture` |

Golden updates are disabled unless `UPDATE_GOLDENS=1` is explicitly set. PNG files are deterministic artifacts only; equality is decided from decoded canonical BGRA pixels, not encoded PNG metadata.

### TASK-0055 UI headless windowd present matrix

`TASK-0055` proves the first bounded headless `windowd` surface/layer/present
path. It does not prove visible scanout, real input routing, GPU/display-driver
behavior, or kernel/MM/IPC/zero-copy production closure.

| Requirement surface | Proof type | Canonical command |
| --- | --- | --- |
| Surface/layer state machine, exact BGRA composition, no-damage present skip, deterministic layer ordering, minimal present acknowledgement, vsync/input stubs | host behavior assertions | `cargo test -p ui_windowd_host -- --nocapture` |
| Generated Cap'n Proto encode/decode for surface create, queue-buffer damage, scene commit, vsync subscribe, and input subscribe | host IDL codec assertions | `cargo test -p ui_windowd_host capnp -- --nocapture` |
| Invalid dimensions/stride/format, missing/forged/wrong-rights VMO handles, stale surface/commit IDs, unauthorized layer mutation, fake marker/postflight reject | host reject assertions | `cargo test -p ui_windowd_host reject -- --nocapture` |
| Headless desktop present marker ladder (`windowd: ready`, `windowd: systemui loaded`, `windowd: present ok`, launcher/selftest UI markers) with proof-manifest verification | single-VM OS-gated marker proof | `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os` |
| Repo closure gates | full build/test/run assertions | `scripts/fmt-clippy-deny.sh`, `make build` then `make test`, `make build` then `make run` |

The headless QEMU proof uses `64x48@60Hz` to stay inside the current selftest
heap. Rich display/profile presets remain `TASK-0055D`; visible output remains
`TASK-0055B`/`TASK-0055C`.

### TASK-0055B visible QEMU scanout bootstrap

`TASK-0055B` proves one deterministic visible first-frame path in QEMU. It uses
`NEXUS_DISPLAY_BOOTSTRAP=1` to boot a graphics-capable QEMU `ramfb` path and
keeps `visible-bootstrap` as a harness/marker profile, not a future
SystemUI/launcher start profile such as desktop, TV, mobile, or car.

| Requirement surface | Proof type | Canonical command |
| --- | --- | --- |
| Fixed 1280x800 ARGB8888 mode, stride validation, display capability handoff, and pre-scanout marker rejection | host behavior/reject assertions | `cargo test -p windowd -p ui_windowd_host -- --nocapture` |
| `selftest-client` visible bootstrap and `init-lite` `fw_cfg` capability path compile for the OS target | OS target compile assertions | `RUSTFLAGS='--check-cfg=cfg(nexus_env,values("host","os")) --cfg nexus_env="os"' NEXUS_DISPLAY_BOOTSTRAP=1 cargo check -p selftest-client --target riscv64imac-unknown-none-elf --release --no-default-features --features os-lite` and `RUSTFLAGS='--check-cfg=cfg(nexus_env,values("host","os")) --cfg nexus_env="os"' cargo check -p init-lite --target riscv64imac-unknown-none-elf --release` |
| Visible marker ladder (`display: bootstrap on`, `display: mode 1280x800 argb8888`, `windowd: present ok`, `display: first scanout ok`, `SELFTEST: display bootstrap guest ok`) with proof-manifest verification | single-VM QEMU visible scanout proof | `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap` |

This closes only the bootstrap scanout path. Visible SystemUI/launcher profile
selection, input, cursor, dirty-rect display services, perf budgets, virtio-gpu,
and kernel/core production-grade display closure remain follow-up scope.

### TASK-0055C visible windowd present + SystemUI first frame

`TASK-0055C` proves that visible QEMU output can be fed by the real
`windowd` present lifecycle with a deterministic SystemUI first frame. The
`visible-bootstrap` profile remains only a harness/marker profile.

| Requirement surface | Proof type | Canonical command |
| --- | --- | --- |
| TOML-backed `desktop` SystemUI profile/shell seed and deterministic BGRA first-frame pixels/checksum | host behavior assertions | `cargo test -p systemui -- --nocapture` |
| Visible present evidence uses `windowd` composition (full composed frame on host, composed rows in OS to fit selftest heap), not a raw SystemUI source-buffer write; invalid mode/capability/pre-marker paths reject | host behavior/reject assertions | `cargo test -p windowd -p ui_windowd_host -- --nocapture` and `cargo test -p ui_windowd_host reject -- --nocapture` |
| `selftest-client` visible SystemUI path compiles for the OS target | OS target compile assertion | `RUSTFLAGS='--check-cfg=cfg(nexus_env,values("host","os")) --cfg nexus_env="os"' NEXUS_DISPLAY_BOOTSTRAP=1 cargo check -p selftest-client --target riscv64imac-unknown-none-elf --release --no-default-features --features os-lite` |
| Visible marker ladder (`windowd: backend=visible`, `windowd: present visible ok`, `systemui: first frame visible`, `SELFTEST: ui visible present ok`) with proof-manifest verification | single-VM QEMU visible SystemUI proof | `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap` |

This slice still does not prove input, cursor/focus/click, display-service
integration, dirty-rect scanout, frame-budget smoothness, dev display/profile
preset matrices, or kernel/core production-grade display closure.

### TASK-0056 v2a present scheduler + input routing

`TASK-0056` proves the first double-buffered present scheduler and input routing
baseline in `windowd`. The host suite is the behavior authority; QEMU markers are
accepted only through the proof-manifest verified `visible-bootstrap` profile.

| Requirement surface | Proof type | Canonical command |
| --- | --- | --- |
| Frame-indexed back-buffer acquisition, deterministic rapid-submit coalescing, no-damage skip, and minimal post-present fence signaling | host behavior assertions | `cargo test -p ui_v2a_host -- --nocapture` |
| Stale/unauthorized/invalid frame index, scheduler queue/damage caps, no-focus keyboard, input backlog cap, and postflight log-only rejects | host reject assertions | `cargo test -p ui_v2a_host reject -- --nocapture` |
| v2a marker ladder (`windowd: present scheduler on`, `windowd: input on`, `windowd: focus -> 1`, `launcher: click ok`, `SELFTEST: ui v2 present ok`, `SELFTEST: ui v2 input ok`) with proof-manifest verification | single-VM QEMU v2a proof | `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap` |

This slice remains a functional baseline only. It does not prove cursor visuals,
real HID/touch input, latency budgets, WM-lite/compositor-v2 breadth,
screenshot/GTK refresh evidence, or kernel/core zero-copy production closure.

## Workflow checklist

1. Extend userspace tests first and run `cargo test --workspace` until green.
2. Execute Miri for host-compatible crates.
3. Refresh Golden Vectors (IDL frames, ABI structs) and bump SemVer when contracts change.
4. Rebuild the Podman development container (`podman build -t open-nexus-os-dev -f podman/Containerfile`) so host tooling matches CI.
5. **Run OS build hygiene checks**: `just diag-os` and `just dep-gate` (catches forbidden dependencies).
6. Run OS smoke coverage via QEMU: `just test-os` (bounded by `RUN_TIMEOUT`, exits on readiness markers).
7. For SMP changes, run dual-mode proof commands sequentially:
   - `SMP=2 REQUIRE_SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
   - `SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`

## Topic guides

- `docs/testing/device-mmio-access.md` — Device MMIO access tests today + extension plan.
- `docs/testing/network-distributed-debugging.md` — SSOT for network/distributed triage (`qemu-test` proof knobs, `os2vm` phases, packet capture, typed error matrix).
- `docs/testing/replay-and-bisect.md` — Phase-6 replay/bisect workflow, bounded budgets, determinism allowlist operations, **proof-floor evidence map (§9)**, **synthetic bad-bundle reproducer (§10)**, and **single remaining environmental closure step (external CI-runner replay artifact, §11)**.
- `docs/testing/trace-diff-format.md` — deterministic trace diff classes and machine-readable output contract.
- `docs/testing/bisect-good-drift-regress.json` — fixture for the Phase-6 3-commit `good→drift→regress` synthetic bisect smoke (`tools/bisect-evidence.sh ... --synthetic-map=docs/testing/bisect-good-drift-regress.json ...`).
- `docs/testing/trace-diff-fixtures.json` — fixture corpus for `tools/diff-traces.sh` (exact / extra / missing / reorder / phase-mismatch classes).

## Scaffold sanity

Run the QEMU smoke test to confirm the UART marker sequence reaches
`SELFTEST: e2e exec-elf ok`. Keep `RUN_UNTIL_MARKER=1` to exit early once markers are
seen and ensure log caps are in effect. `just test-os` wraps
`scripts/qemu-test.sh`, so the same command exercises the minimal exec path.

### Just targets

- Host unit/property: `just test-host`
- Host E2E: `just test-e2e` (runs `nexus-e2e`, `remote_e2e`, `logd-e2e`, `vfs-e2e`, `e2e_policy`)
- DSoftBus mux requirement suites (`TASK-0020`): `just test-dsoftbus-mux`
- DSoftBus QUIC host requirement suites (`TASK-0021`): `just test-dsoftbus-quic`
- DSoftBus full host regression: `just test-dsoftbus-host`
- nx CLI host proof suite (`TASK-0045`): `cargo test -p nx -- --nocapture`
- Config v1 host proof floor (`TASK-0046`):
  - `cargo test -p nexus-config -- --nocapture`
  - `cargo test -p configd -- --nocapture`
  - `cargo test -p nx -- --nocapture`
  - proves Soll-Verhalten, not implementation detail coupling:
    - stable reject classification for unknown/type/depth/size failures
    - deterministic layered merge order and JSON-only authoring contract
    - semantic parity between `configd` views and `nx config effective --json`
    - honest 2PC commit/abort/rollback state transitions with unchanged-version evidence on failure
    - deterministic CLI exit/JSON/file-effect contracts under `nx config`
- Policy as Code v1 host proof floor (`TASK-0047`):
  - `cargo test -p policy -- --nocapture`
  - `cargo test -p nexus-config -- --nocapture`
  - `cargo test -p configd -- --nocapture`
  - `cargo test -p policyd -- --nocapture`
  - `cargo test -p nx -- --nocapture`
  - proves Soll-Verhalten, not implementation detail coupling:
    - single live policy root under `policies/` with `recipes/policy/` non-authoritative
    - deterministic `PolicyVersion` for equivalent validated inputs
    - stable `test_reject_*` classes for invalid/oversize/ambiguous/traversal/trace-budget cases
    - deterministic `policies/manifest.json` validation and fail-closed missing/mismatch handling
    - bounded explain traces and dry-run/learn non-bypass semantics
    - authenticated/current-version mode changes and stale/unauthorized rejects
    - Config v1 `policy.root` effective-snapshot carriage into the `configd::ConfigConsumer` policy reload path
    - external `policyd` host frame operations for version/eval/mode get/mode set
    - first adapter parity plus service-facing `policyd` check-frame cutover through the unified authority
    - `nx policy` deterministic exit/JSON contracts under the existing `tools/nx` binary, including explicit `mode` preflight-only output
- UI host renderer snapshot proof floor (`TASK-0054` / `RFC-0046`):
  - `cargo test -p ui_renderer -- --nocapture`
  - `cargo test -p ui_host_snap -- --nocapture`
  - `cargo test -p ui_host_snap reject -- --nocapture`
  - proves Soll-Verhalten, not implementation detail coupling:
    - BGRA8888 byte order and 64-byte frame stride
    - deterministic primitive pixels and bounded damage semantics
    - fixture-font text without host font discovery or locale fallback
    - canonical BGRA golden comparison with PNG metadata ignored
    - fail-closed update/path traversal/input reject classes
    - no host-only fake OS/QEMU success markers
- QEMU smoke: `RUN_UNTIL_MARKER=1 just test-os` (defaults to `PROFILE=full`)
- QEMU smoke (DHCP requested): `just ci-os-dhcp` (PROFILE-driven; replaces the deleted `test-os-dhcp`)
- QEMU smoke (Strict DHCP gate): `just ci-os-dhcp-strict` (PROFILE-driven; replaces the deleted `test-os-dhcp-strict`)
- DSoftBus 2-VM harness (TASK-0005): `just ci-os-os2vm` (PROFILE-driven; or `just os2vm` for the raw 2-VM driver)
- Crashdump v1 host proofs (TASK-0018):
  - `cargo test -p crash -- --nocapture`
  - `cargo test -p execd -- --nocapture`
  - `cargo test -p minidump-host -- --nocapture`
  - `cargo test -p statefsd -- --nocapture`
- DSoftBus 2-VM harness + PCAP: `just test-dsoftbus-2vm-pcap` (or `just os2vm-pcap`)
  - Expected remote markers on success: `SELFTEST: remote resolve ok`, `SELFTEST: remote query ok`, `SELFTEST: remote pkgfs stat ok`, `SELFTEST: remote pkgfs open ok`, `SELFTEST: remote pkgfs read step ok`, `SELFTEST: remote pkgfs close ok`, `SELFTEST: remote pkgfs read ok`, and `dsoftbusd: remote packagefs served`
- Full gate (recommended before “everything is green”): `just test-all`
  - Includes `fmt-check`, `lint`, `deny-check`, host tests, Miri tiers, `arch-check`, and QEMU selftests.
- Make convenience wrapper: `make verify`
  - Delegates to `just` gates (`diag-host`, `test-host`, `test-e2e`, `dep-gate`, `diag-os`, `test-os`).
  - Optional SMP dual-mode extension: `REQUIRE_SMP_VERIFY=1 make verify`.

### Manifest-driven workflow (TASK-0023B Phase 4)

The QEMU marker ladder, harness profile catalog, and runtime selftest profile catalog live under **`source/apps/selftest-client/proof-manifest/manifest.toml`** and its included `markers/`, `profiles/`, and `phases.toml` files. This proof-manifest tree is the single source of truth (SSOT). `scripts/qemu-test.sh`, `tools/os2vm.sh`, the `selftest-client` build, and the `nexus-proof-manifest` host CLI all read from it; nothing else is allowed to declare markers, profile env, or phase order.

#### Profile catalog

| Kind     | Profile         | Driver                          | Purpose |
|---       |---              |---                              |---|
| Harness  | `full`          | `scripts/qemu-test.sh`          | Default 12-phase ladder (`just test-os` / `just ci-os-full`). |
| Harness  | `visible-bootstrap` | `scripts/qemu-test.sh`      | TASK-0055B/0055C/0056 visible `ramfb`, SystemUI, and v2a marker ladder (`just test-os visible-bootstrap`); not a SystemUI start profile or screenshot proof. |
| Harness  | `smp`           | `scripts/qemu-test.sh`          | SMP-only marker contract; consumed by `just ci-os-smp` (SMP=2 strict + SMP=1 parity). |
| Harness  | `dhcp`          | `scripts/qemu-test.sh`          | DHCP requested; deterministic fallback allowed (`just ci-os-dhcp`). |
| Harness  | `dhcp-strict`   | `scripts/qemu-test.sh`          | DHCP must bind (`just ci-os-dhcp-strict`); extends `dhcp`. |
| Harness  | `quic-required` | `scripts/qemu-test.sh`          | QUIC required; `transport selected tcp` is forbidden (`just ci-os-quic`). |
| Harness  | `supply-chain`  | `scripts/qemu-test.sh`          | Supply-chain marker ladder required (`just test-os supply-chain`). |
| Harness  | `os2vm`         | `tools/os2vm.sh`                | Cross-VM DSoftBus harness (`just ci-os-os2vm`). |
| Runtime  | `bringup`       | `os_lite::profile` (in-OS)      | Boots `bringup` + `end` phases only (`SELFTEST_PROFILE=bringup`). |
| Runtime  | `quick`         | `os_lite::profile` (in-OS)      | `bringup` → `ipc_kernel` → `mmio` → `end`. |
| Runtime  | `ota`           | `os_lite::profile` (in-OS)      | `bringup` → `ipc_kernel` → `ota` → `end`. |
| Runtime  | `net`           | `os_lite::profile` (in-OS)      | `bringup` → `ipc_kernel` → `mmio` → `routing` → `net` → `end`. |
| Runtime  | `none`          | `os_lite::profile` (in-OS)      | `bringup` + `end` only — empty middle. |

Harness profiles live under `[profile.<name>]` with `runner` + optional `extends` + `env` keys. Runtime profiles set `runtime_only = true` and declare `phases = [...]` (subset of the 12 declared `[phase.X]` entries); skipped phases emit a single `dbg: phase X skipped` breadcrumb instead of any `SELFTEST:` markers.

#### Adding a new marker

1. Append a `[marker."<literal>"]` entry to `proof-manifest.toml` with `phase = "<name>"` and any `emit_when = { profile = "..." }` / `forbidden_when = { profile = "..." }` gates.
2. Re-run `cargo build -p selftest-client` (the `build.rs` regenerates `markers_generated.rs` with a `pub(crate) const M_<KEY>: &str = "<literal>";` constant).
3. In the emitting site, reference the constant: `crate::markers::emit_line(crate::markers::M_<KEY>);`. Do not hand-write the literal — `arch-gate` Rule 3 fails the build if any `SELFTEST:` / `dsoftbusd:` / `dsoftbus:` literal appears outside `markers.rs` + `markers_generated.rs`.
4. Add a `cargo test -p nexus-proof-manifest` reject test if the marker carries new gating semantics.

#### Adding a new harness profile

1. Append `[profile.<name>]` with `runner = "scripts/qemu-test.sh"` (or your driver), optional `extends = "<parent>"`, and `env = { ... }`. Use `extends` for inheritance — child entries shadow parent keys; cycles are rejected at parse time.
2. Add a `just ci-<flavor>` recipe in the `# CI matrix` section of `justfile` that invokes `just test-os PROFILE=<name>` (or your alternate runner). Do **not** put `REQUIRE_*` env literals in the recipe body — `arch-gate` Rule 6 will fail. All env wiring belongs in the manifest.
3. Verify: `nexus-proof-manifest list-env --profile=<name>` prints the resolved env; `nexus-proof-manifest list-markers --profile=<name>` prints the expected marker ladder.

#### Adding a new runtime profile

1. Append `[profile.<name>]` with `runtime_only = true` and `phases = ["...", "..."]` (subset of declared phases). The parser rejects unknown phase names and rejects `runner` keys on runtime-only profiles.
2. Add a `Profile::<Name>` variant to `source/apps/selftest-client/src/os_lite/profile.rs` and wire it into `Profile::from_kernel_cmdline_or_default` + `Profile::includes`.
3. Add a `ci-os-runtime-<name>` recipe that sets `SELFTEST_PROFILE=<name>` at build time (the env is read via `option_env!`, so the value is baked into the binary; `build.rs` already advertises `cargo:rerun-if-env-changed=SELFTEST_PROFILE`).

#### Adding a new phase

1. Update [RFC-0014](../rfcs/RFC-0014-testing-contracts-and-qemu-phases-v1.md) and append a `[phase.<name>] order = N` entry to `proof-manifest.toml`.
2. Add `source/apps/selftest-client/src/os_lite/phases/<name>.rs` and register it in `os_lite/phases/mod.rs`.
3. Extend `PhaseId` in `os_lite/profile.rs` and the `run_or_skip!` chain in `os_lite/mod.rs` (declarative; no logic).
4. Add a `dbg: phase <name> skipped` marker entry to `proof-manifest.toml` so runtime profiles can declaratively skip the phase.

#### Deny-by-default analyzer (P4-09)

After every QEMU pass `scripts/qemu-test.sh` invokes `nexus-proof-manifest verify-uart --profile=<name> --uart=uart.log`. Any marker that is not in the profile's expected set, or that the profile lists as forbidden, fails the run with exit 1. Set `PM_VERIFY_UART=0` only as a temporary escape hatch (will be required by P4-10 closure / Phase-5).

### Phase-gated QEMU smoke (triage helper)

For faster triage (RFC‑0014 Phase 2), you can stop QEMU early after a named **phase** and only
validate markers up to that phase:

- Supported phases: `bring-up`, `routing`, `ota`, `policy`, `logd`, `vfs`, `end`

Examples:

```bash
# Stop after core service bring-up (init + core *: ready markers)
RUN_PHASE=bring-up RUN_TIMEOUT=90s just test-os

# Stop after OTA smoke (stage → switch → health → rollback)
RUN_PHASE=ota RUN_TIMEOUT=190s just test-os
```

On failure, the harness prints:

- `first_failed_phase=<name>`
- `missing_marker='<marker>'`
- a **bounded** UART excerpt scoped to the failed phase.

### Network/distributed debugging (SSOT)

Canonical networking proof semantics and knobs now live in:

- `docs/testing/network-distributed-debugging.md` (SSOT)

Keep this file focused on the global testing workflow. For DHCP/DSoftBus proof gates, packet evidence expectations, and `os2vm` typed failure triage, use the SSOT document.

### Miri tiers

- Strict (no IO): run on crates without filesystem/network access.
  - Example: `just miri-strict` (uses `MIRIFLAGS='--cfg nexus_env="host"'`).
- FS-enabled: for crates that legitimately touch the filesystem or env.
  - Example: `just miri-fs` (uses `MIRIFLAGS='-Zmiri-disable-isolation --cfg nexus_env="host"'`).
- Under `#[cfg(miri)]`, keep property tests lightweight (lower case count, disable persistence) to avoid long runtimes.

## OS-E2E marker sequence and VMO split

The os-lite `nexus-init` backend is responsible for announcing service
bring-up. The OS smoke path emits a deterministic sequence of UART markers that
the runner validates in order.

Readiness contract (RFC-0013):

- `init: up <svc>` means the control-plane handshake completed (spawn + bootstrap channel is live).
- `<svc>: ready` means the service is fully ready to serve its v1 contract.
- Tests MUST NOT treat `init: up` as readiness; missing `<svc>: ready` is a hard failure with an explicit error message.
- Low-effort readiness gates may query `logd` for `*: ready` markers when a service lacks a dedicated ready RPC (stopgap until readiness RPCs exist).

Additional kernel selftest markers may appear before `init: start`:

- `KSELFTEST: spawn reasons ok`
- `KSELFTEST: resource sentinel ok`

SMP-gated markers (enabled only with `REQUIRE_SMP=1` and `SMP>=2`):

- `KINIT: cpu1 online`
- `KSELFTEST: smp online ok`
- `KSELFTEST: ipi counterfactual ok`
- `KSELFTEST: ipi resched ok`
- `KSELFTEST: test_reject_invalid_ipi_target_cpu ok`
- `KSELFTEST: test_reject_offline_cpu_resched ok`
- `KSELFTEST: work stealing ok`
- `KSELFTEST: test_reject_steal_above_bound ok`
- `KSELFTEST: test_reject_steal_higher_qos ok`

Memory pressure note (until TASK-0228):

- Boot runs may still hit allocator exhaustion (`ALLOC-FAIL`) or late spawn failures due to memory pressure.
- This is expected to be fully addressed by the cooperative OOM watchdog in
  `tasks/TASK-0228-oomd-v1-deterministic-watchdog-cooperative-memstat-samgr-kill.md`.
- Until then, use the SpawnFailReason markers and readiness gates to diagnose failures quickly.

Marker order:

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
43. `execd: minidump written /state/crash/child.demo.minidump.nmd` – crash artifact metadata validated and accepted
44. `execd: crash report pid=... code=42 name=demo.minidump` – execd observed non-zero exit and emitted crash report
45. `SELFTEST: crash report ok` – selftest client verified crash report via logd query
46. `SELFTEST: minidump ok` – selftest client validated artifact + report path end-to-end
47. `SELFTEST: minidump forged metadata rejected` – forged metadata rejected fail-closed
48. `SELFTEST: minidump no-artifact metadata rejected` – report without artifact bytes rejected fail-closed
49. `SELFTEST: minidump mismatched build_id rejected` – inconsistent build metadata rejected fail-closed
50. `SELFTEST: end` – concluding marker from the selftest client

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
hand-off used by the OS build today. Execute the tests with
`cargo test -p remote_e2e`—they finish in a few seconds and require no QEMU.

The DSoftBus OS backend is implemented (`TASK-0003` through `TASK-0005`) and the
daemon orchestration is now modularized (`TASK-0015`): `source/services/dsoftbusd/src/main.rs`
is a thin entry/wiring layer, with host seam coverage in
`source/services/dsoftbusd/tests/p0_unit.rs`,
`source/services/dsoftbusd/tests/reject_transport_validation.rs`, and
`source/services/dsoftbusd/tests/session_steps.rs`.

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
cargo test -p dsoftbusd -- reject
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
| `policyd: audit emit ok` | Audit record successfully emitted to logd |
| `keystored: sign denied (subject=<svc>)` | Policy-gated signing works |
| `SELFTEST: policy deny audit ok` | Deny decision + audit record proven |
| `SELFTEST: policy allow audit ok` | Allow decision + audit record proven |
| `SELFTEST: keystored sign denied ok` | Policy-gated signing denied without required capability |

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

### QEMU smoke proof knobs (determinism)

Network/distributed debugging runbooks, packet capture workflow, `os2vm` phase controls, typed error matrix, and slot-mismatch triage are maintained in:

- `docs/testing/network-distributed-debugging.md` (SSOT)

Keep this index focused on the global testing framework; use the SSOT document for operational network/distributed triage details.

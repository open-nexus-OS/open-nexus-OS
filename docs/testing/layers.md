<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Testing layers reference

Full layer-by-layer testing reference, including the end-to-end coverage table and the per-TASK requirement matrices. Split out of the former `docs/testing/index.md`; the entry point and workflow checklist live in [README.md](README.md).

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

Current live-output hardening work uses the focused service matrix in
[`display-output-hardening-matrix.md`](display-output-hardening-matrix.md).
When `visible-bootstrap` fails after host tests pass, treat the failed marker as
a missing per-service proof first, not as a reason to add marker-only retries.

| Requirement surface | Proof type | Canonical command |
| --- | --- | --- |
| Fixed 1280x800 ARGB8888 mode, `fbdevd -> windowd` framebuffer registration, framebuffer-capability rejection, and pre-scanout marker rejection | host behavior/reject assertions | `cargo test -p windowd -p fbdevd -- --nocapture` |
| `selftest-client` visible bootstrap and `init-lite` `fw_cfg` capability path compile for the OS target | OS target compile assertions | `RUSTFLAGS='--check-cfg=cfg(nexus_env,values("host","os")) --cfg nexus_env="os"' NEXUS_DISPLAY_BOOTSTRAP=1 cargo check -p selftest-client --target riscv64imac-unknown-none-elf --release --no-default-features --features os-lite` and `RUSTFLAGS='--check-cfg=cfg(nexus_env,values("host","os")) --cfg nexus_env="os"' cargo check -p init-lite --target riscv64imac-unknown-none-elf --release` |
| Service-owned visible ladder (`fbdevd: ready`, `fbdevd: map ok`, `fbdevd: ramfb configured`, `fbdevd: flush ok`, `display: bootstrap on`, `display: mode 1280x800 argb8888`, `display: first scanout ok`) with proof-manifest verification | single-VM QEMU visible scanout proof | `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap` |

This closes the bootstrap scanout path. TASK-0057 extends the same
`visible-bootstrap` harness to prove the Minimal DisplayServer v0 asset scene;
`just start` is the separate live interactive check.

### TASK-0055C visible windowd present + SystemUI first frame

`TASK-0055C` proves that visible QEMU output can be fed by the real
`windowd` present lifecycle with a deterministic SystemUI first frame. The
`visible-bootstrap` profile remains only a harness/marker profile.

The visible QEMU runner must not hardcode a mixed pointer set forever. The
intended source of truth for which visible input devices exist is the
SystemUI profile manifest under
`source/services/systemui/manifests/profiles/<id>/profile.toml`, starting with
the `[input]` booleans in the `desktop` seed. Visible harness profiles may
select the manifest via env, but the emulated keyboard/mouse/tablet set should
be derived from that manifest rather than duplicated inside shell scripts.

| Requirement surface | Proof type | Canonical command |
| --- | --- | --- |
| TOML-backed `desktop` SystemUI profile/shell seed and JPEG-sourced deterministic first-frame pixels/checksum | host behavior assertions | `cargo test -p systemui -- --nocapture` |
| Visible present evidence uses `windowd` DisplayServer composition, not a raw SystemUI source-buffer write; invalid mode/capability/pre-marker paths reject | host behavior/reject assertions | `cargo test -p windowd -- --nocapture` |
| `selftest-client` visible SystemUI path compiles for the OS target | OS target compile assertion | `RUSTFLAGS='--check-cfg=cfg(nexus_env,values("host","os")) --cfg nexus_env="os"' NEXUS_DISPLAY_BOOTSTRAP=1 cargo check -p selftest-client --target riscv64imac-unknown-none-elf --release --no-default-features --features os-lite` |
| Service-owned visible-present ladder (`windowd: backend=visible`, `windowd: present visible ok`, `systemui: first frame visible`, `SELFTEST: ui visible present ok`) plus `fps: windowd` / `fps: fbdevd` failure-summary traces | single-VM QEMU visible SystemUI proof | `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap` |

This slice still does not prove input, cursor/focus/click, display-service
integration, dirty-rect scanout, frame-budget smoothness, dev display/profile
preset matrices, or kernel/core production-grade display closure.

### TASK-0057 Minimal DisplayServer v0 asset scene

`TASK-0057` proves that the visible path has one scene authority:
`inputd -> windowd -> fbdevd -> ramfb`. `windowd` owns JPEG-sourced wallpaper,
SVG cursor, text/icon proof targets, and composition into the framebuffer VMO.
`fbdevd` owns scanout only, and `selftest-client` remains observer-only.

| Requirement surface | Proof type | Canonical command |
| --- | --- | --- |
| DisplayServer-v0 protocol frames and reject paths (`OP_UPDATE_VISIBLE_STATE`, visible-state response, malformed/truncated rejects) | host protocol assertions | `cargo test -p input-live-protocol -- --nocapture` |
| JPEG-sourced SystemUI seed and SVG cursor/assets visible in service-owned state | host service assertions | `cargo test -p systemui -p windowd -p fbdevd -- --nocapture` |
| Observer cannot synthesize asset success; summary waits for cursor/wallpaper/text/icon/overlay evidence | host observer assertions | `cargo test -p selftest-client -- --nocapture` |
| Display services compile as os-lite daemons, including standalone `windowd` | OS target compile assertion | `RUSTFLAGS='--cfg nexus_env="os"' cargo +nightly-2025-01-15 check --target riscv64imac-unknown-none-elf --no-default-features --features os-lite -p windowd -p fbdevd -p inputd` |
| Asset marker ladder ends at v2b success, not wheel success | single-VM QEMU visible proof | `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap` |

The live proof is `just start`: the GTK/QEMU window should show the same
DisplayServer scene with JPEG wallpaper, SVG cursor, text/icon targets, and
live pointer movement.

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

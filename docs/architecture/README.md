<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Architecture index (`docs/architecture/`)

**Canonical architecture entry point.** This directory contains high-level
architecture notes for key subsystems. These pages are stable entrypoints;
avoid duplicating fast-moving contracts (like full UART marker sequences) and
instead link to the canonical sources.

- **Vision lens (architecture/security/performance direction)**: `vision.md`
- **Testing methodology (host-first, QEMU-last)**: `../testing/README.md`
- **Execution truth / workflow**: `../../tasks/README.md`
- **RFC process / contracts vs tasks**: `../rfcs/README.md`
- **ADR index**: `../adr/README.md`

## Suggested reading order (onboarding)

1. `05-system-map-and-boundaries.md` — repo mental model + hard boundaries
2. `06-boot-and-bringup.md` — boot chain + who owns which markers
3. `07-contracts-map.md` — where contracts live (Tasks/RFC/ADR) + key canonical specs
4. `08-service-architecture-onboarding.md` — services as thin adapters over userspace libraries
5. Service landings (pick what you're touching): `09-nexus-init.md`,
   `10-execd-and-loader.md`, `11-policyd-and-policy-flow.md`,
   `12-storage-vfs-packagefs.md`, `13-identity-and-keystore.md`,
   `14-samgrd-service-manager.md`, `15-bundlemgrd.md`

## Layering quick reference

Domain logic lives in `userspace/<crate>` libraries behind mutually exclusive
`nexus_env="host"` / `nexus_env="os"` configurations; each crate compiles with
exactly one environment and forbids unsafe code. Daemons under
`source/services/<name>d` are thin adapters: they register with `samgr`, expose
IDL bindings, and forward into the userspace crate compiled with
`nexus_env="os"`. `tools/arch-check` fails CI when a userspace crate depends on
kernel, HAL, samgrd, nexus-abi, or `source/services/` — preserving the
host-tested-logic vs system-wiring separation.

**Control plane vs data plane:** Cap'n Proto schemas live exclusively in
userspace (`tools/nexus-idl` + `userspace/nexus-idl-runtime`); the kernel never
parses Cap'n Proto, it only shuttles handles/VMOs. Bulk payloads move
out-of-band via VMOs + `map()`. The windowd↔gpud **display wire** is
deliberately *not* Cap'n Proto: hand-rolled opcodes + the `nexus-gfx`
`CommittedBuffer` codec, owned by `source/libs/nexus-display-proto`
(ADR-0038); Cap'n Proto stays the control plane.

**Kernel quick reference:** entry `kmain()` brings up HAL, Sv39
`AddressSpaceManager`, syscall table, `Scheduler`, `ipc::Router`; idle loop
drives cooperative scheduling; SMP proofs run a dual-mode deterministic ladder
(`REQUIRE_SMP=1`). Key files: `source/kernel/neuron/src/core/kmain.rs`,
`src/syscall/`, `src/mm/address_space.rs`, `src/core/trap.rs`, `src/mm/satp.rs`.
Don't touch without RFC/ADR: syscall IDs/ABI, trap prologue/epilogue, kernel
memory map/SATP assumptions. Early boot uses raw UART only (no heavy
formatting/alloc before selftests); boot gates v1 (RFC-0013) enforce the
readiness contract (`init: up` vs `<svc>: ready`) in the QEMU harness.

## Kernel

- `01-neuron-kernel.md` — NEURON kernel overview (syscalls, memory model, invariants)
- `16-rust-concurrency-model.md` — Rust ownership & Servo-inspired parallelism (SMP baseline + follow-ups)
- `../rfcs/RFC-0021-kernel-smp-v1-percpu-runqueues-ipi-contract.md` — SMP v1 contract (Complete)
- `smp-ipi-rate-limiting.md` — IPI rate limiting policy (DoS prevention, TASK-0012/0042)
- `hardening-plan.md` / `hardening-status.md` — kernel hardening objectives + status snapshot
- `kernel-task-inventory.md` — complete inventory of all kernel-touch tasks (security consistency check)
- `security-consistency-check.md` — decision points and drift prevention across SMP/QoS/parallelism tasks
- `rust-advantages.md` — why Rust is optimal for a consumer-facing OS

### SMP / soft-real-time

- SMP=4 is the interactive default: declarative CPU placement, phased vmo/exec
  syscalls, lock-free syscall class, cpu0 BKL right-of-way, VMO zero-frontier
  (see `../adr/0049-bkl-lockclass-and-softrt-cpu-placement.md` and the
  ADR-0045..0048 cluster).
- Soft-real-time spine (waitset + timeline fence + DriverKit submit):
  `../rfcs/RFC-0033-soft-real-time-spine-waitset-fence-driverkit.md`,
  `../adr/0033-soft-real-time-spine.md`.
- The `bkl budget ok` gate is enforced in SMP proof lanes (bring-up burst
  logged, steady state gated).

## Testing + CI

- `02-selftest-and-ci.md` — how we validate: deterministic markers + CI wiring (high level)
- **Canonical QEMU harness + marker contract**: `scripts/qemu-test.sh` (do not duplicate marker lists here)
- **CI workflows**: `.github/workflows/`
- **QEMU smoke proof gating (networking/DSoftBus)**: `../adr/0025-qemu-smoke-proof-gating.md`

## Networking

- `networking-authority.md` — canonical vs alternative networking paths, anti-drift rules
- `network-address-matrix.md` — normative address/subnet/profile matrix (QEMU + os2vm)
- Deep dives: `../distributed/dsoftbus-lite.md`, `../distributed/dsoftbus-mux.md`, `../distributed/remote-fs.md`
- **RFC-0006** sockets facade · **RFC-0007** DSoftBus OS transport · **RFC-0008** Noise XK
- **RFC-0009** no_std dependency hygiene · **RFC-0027** dsoftbusd modular daemon
- **RFC-0035** QUIC v1 scaffold (Done) · **RFC-0036** core no_std transport abstraction (Complete)
- **RFC-0060** streams v2 mux/flow-control/keepalive (Done; formerly RFC-0033)
- **ADR-0026**: network address profiles + validation semantics

## Storage (nxfs / VFS / statefs)

- `12-storage-vfs-packagefs.md` — storage/VFS/packagefs onboarding landing
- Deep dives: `../storage/nxfs.md` (user-data CoW filesystem),
  `../storage/vfs.md` (VFS v2), `../storage/statefs.md` (service KV),
  `../storage/vmo.md` (VMO arena semantics)
- **RFC-0071**: nxfs user-data filesystem contract
- **RFC-0072**: VFS v2 — writable providers, readdir, stable errors
- **RFC-0073**: app files surface (`svc.files`, permissions, filemanager role)
- **ADR-0043**: user data in a dedicated CoW fs (statefs stays service-KV)
- **ADR-0044**: single blk device, GPT partitions, block layer

## Services and contracts

### Domain libraries (host-first)

- `03-samgr.md` — `userspace/samgr` (host-first registry library; OS uses `samgrd`)
- `04-bundlemgr-manifest.md` — canonical `manifest.nxb`/`BundleManifest` contract (Cap'n Proto) plus host parser constraints
- **Updates contract (v1.0)**: `../rfcs/RFC-0012-updates-packaging-ab-skeleton-v1.md` + `../packaging/system-set.md` (Complete)
- **Boot gates contract (v1.0)**: `../rfcs/RFC-0013-boot-gates-readiness-spawn-resource-v1.md` (Complete)

### OS daemons (authorities)

- `14-samgrd-service-manager.md` — `samgrd` (OS service registry authority)
- `15-bundlemgrd.md` — `bundlemgrd` (OS bundle/package authority)
- Config v1 authority: `configd` is the canonical typed config distribution
  authority (`../rfcs/RFC-0044-config-v1-configd-schema-layering-2pc-host-first-os-gated.md`);
  `nx config ...` is the only host CLI surface.
- Policy as Code v1 authority: `policyd` is the single policy decision
  authority (`../rfcs/RFC-0045-policy-as-code-v1-unified-policy-tree-evaluator-explain-dry-run-learn-enforce-nx-policy.md`);
  live policy root is `policies/nexus.policy.toml`; `nx policy ...` lives under `tools/nx`.

### UI / windowing authority

- `windowd` is a **compositor service**; window UI lives in app-host widgets
  (RFC-0067 boundary: `../rfcs/RFC-0067-windowd-compositor-service-boundary-rasterizer-app-ui-extraction.md`).
- Contract ladder: RFC-0047 (headless surface/present) → RFC-0048 (visible
  scanout bootstrap) → RFC-0049 (SystemUI first frame) → RFC-0050 (present
  scheduler + input routing) → RFC-0051 (visible input) → RFC-0052/0053
  (host input core / OS live input: `hidrawd → inputd → windowd`).
- Architecture decisions: `../adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md`,
  `../adr/0029-input-v1-host-core-architecture.md`,
  `../adr/0042-cross-process-surface-transport.md`.

### DSL app platform

- Apps are declarative `.nx` programs compiled by `tools/nx` and executed by
  the app-host runtime; service access is routed declaratively
  (`source/libs/nexus-sdk-routes` is the SSOT for svc → route/permission).
- Developer docs: `../dev/dsl/overview.md` (start here), `../dev/dsl/syntax.md`,
  `../dev/dsl/state.md`, `../dev/dsl/runtime.md`, full set under `../dev/dsl/`.
- Lifecycle/registry/notifications contract:
  `../rfcs/RFC-0065-ui-v6b-app-lifecycle-registry-notifications-navigation-contract.md`;
  declarative service chain: `../rfcs/RFC-0066-production-grade-service-chain-declarative-routing-typed-ipc-inprocess-tests.md`.

## On-device inference (`inference/`)

- `inference/nexusinfer-techniques.md` — catalog of confirmed upstream and candidate local-inference techniques
- `inference/nexusinfer-runtime-profiles.md` — hardware-agnostic runtime/profile vocabulary
- `inference/nexusinfer-rust-design.md` — Rust ownership, newtypes, `Send`/`Sync`, zero-copy guidance

## Graphics and compute (`graphics/`)

- `graphics/nexusgfx-compute-and-executor-model.md` — layer model for `NexusGfx`, compute executors, `NexusInfer` relationship
- `graphics/nexusgfx-resource-model.md` — buffers/images/transient resources/import-export posture
- `graphics/nexusgfx-sync-and-lifetime.md` — fences, waits, ownership return, present pacing
- `graphics/nexusgfx-command-and-pass-model.md` — command buffers, passes, pass-locality rules
- `graphics/nexusgfx-compute-kernel-model.md` — portable compute-kernel and dispatch model
- `graphics/nexusgfx-tile-aware-design.md` — bandwidth-first/mobile/tile-aware design stance
- `graphics/nexusgfx-text-pipeline.md` — renderer-facing text acceleration posture
- `graphics/nexusgfx-artifact-pipeline.md` — offline-first, deterministic, signed artifact strategy
- `graphics/nexusgfx-capability-matrix.md` — backend capability vocabulary
- `graphics/gpud-command-ring-and-present-pipeline.md` — gpud's virtio-gpu command ring, batched + pipelined GL present (ADR-0032)
- `graphics/display-output-service-chain.md` — display output service chain
- `graphics/animation-nexusgfx-gpu-three-layer-stack.md` — animation stack layering
- **Device-class driver architecture (capstone)**: `../adr/0039-device-class-driver-architecture.md` —
  SDK → device-class service → DriverKit → bus-HAL → kernel, per device class
- **Rasterization SSOT**: `userspace/nexus-gfx/src/raster/` — the one canonical software rasterizer (RFC-0067)
- **Display-wire SSOT** (windowd↔gpud): `source/libs/nexus-display-proto` + the `nexus-gfx` `CommittedBuffer` codec (ADR-0038)
- **virtio-mmio bus-HAL**: `source/libs/nexus-virtio` — shared across virtio drivers
- **Cross-device submit substrate**: `source/libs/nexus-driverkit` (SubmitRing + fence + budget + QoS); ADR-0018, ADR-0033

## Observability

- **Logging guide**: `../observability/logging.md` — logd v1 usage + crash reports
- Metrics/tracing: `../observability/metrics.md`, `../observability/tracing.md`
- **RFC-0003** unified logging facade · **RFC-0011** logd journal + crash reports (Complete)
- **RFC-0031** crashdump v1 minidumps + host symbolization (Complete)
- **RFC-0068** structured event observability (subject-grouped journal renderer)

## Policy Authority + Audit

- **Policy flow**: `11-policyd-and-policy-flow.md` — `policyd` as single authority, deny-by-default
- **RFC-0015** policy authority & audit baseline (Complete) · **RFC-0045** policy as code v1
- **Security docs**: `../security/signing-and-policy.md`, `../security/policy-as-code.md`
- Device identity: `../rfcs/RFC-0016-device-identity-keys-v1.md`,
  `../security/identity-and-sessions.md`, `13-identity-and-keystore.md`

## Subsystem deep-dive directories

- `../distributed/` — DSoftBus lite/mux, remote-fs
- `../storage/` — nxfs, vfs, statefs, vmo
- `../observability/` — logging, metrics, tracing
- `../security/` — authority model, signing, identity, policy as code
- `../services/` — service lifecycle, os-lite backends
- `../packaging/` — artifact kinds, nxb format, system set, A/B updates
- `../supplychain/` — reproducibility, SBOM, signing policy

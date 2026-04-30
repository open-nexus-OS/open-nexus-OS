<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Architecture index (`docs/architecture/`)

This directory contains **high-level architecture notes** for key subsystems.
These pages are intended to be stable entrypoints; avoid duplicating fast-moving contracts (like full UART marker sequences) and instead link to the canonical sources.

## Suggested reading order (onboarding)

1. `05-system-map-and-boundaries.md` — repo mental model + hard boundaries
2. `06-boot-and-bringup.md` — boot chain + who owns which markers
3. `07-contracts-map.md` — where contracts live (Tasks/RFC/ADR) + key canonical specs
4. `08-service-architecture-onboarding.md` — services as thin adapters over userspace libraries
5. Service landings (pick what you’re touching):
   - `09-nexus-init.md`
   - `10-execd-and-loader.md`
   - `11-policyd-and-policy-flow.md`
   - `12-storage-vfs-packagefs.md`
   - `13-identity-and-keystore.md`
   - `14-samgrd-service-manager.md`
   - `15-bundlemgrd.md`

## Start here

- **Kernel + layering quick reference (canonical entry page)**: `docs/ARCHITECTURE.md`
- **Testing methodology (host-first, QEMU-last)**: `docs/testing/index.md`
- **Testing contracts (v1)**: `docs/rfcs/RFC-0014-testing-contracts-and-qemu-phases-v1.md` (Complete)
- **Execution truth / workflow**: `tasks/README.md`
- **RFC process / contracts vs tasks**: `docs/rfcs/README.md`
- **Vendored third-party code (pinned forks / patches)**: `vendor/` (keep small; document upstream base + local deltas)

## Kernel

- `01-neuron-kernel.md` — NEURON kernel overview (syscalls, memory model, invariants)
- `16-rust-concurrency-model.md` — Rust ownership & Servo-inspired parallelism (SMP baseline + follow-ups)
- `docs/rfcs/RFC-0021-kernel-smp-v1-percpu-runqueues-ipi-contract.md` — SMP v1 contract (Complete; CPU online mask, secondary bring-up, IPI/selftest markers)
- `smp-ipi-rate-limiting.md` — IPI rate limiting policy (DoS prevention, TASK-0012/0042)
- `hardening-status.md` — kernel hardening objectives checklist (status snapshot)
- `KERNEL-TASK-INVENTORY.md` — Complete inventory of all kernel-touch tasks (security consistency check)
- `SECURITY-CONSISTENCY-CHECK.md` — Decision points and drift prevention across SMP/QoS/parallelism tasks
- `RUST-ADVANTAGES.md` — Why Rust is optimal for a consumer-facing OS (comparison with C/C++)

**Current snapshot**:
- SMP/per-CPU kernel behavior is treated as stable baseline and validated through deterministic proof gates.
- Kernel-focused architecture pages in this directory are maintained as long-lived references; volatile proof details stay in task/RFC execution docs.

## Testing + CI

- `02-selftest-and-ci.md` — how we validate: deterministic markers + CI wiring (high level)
- **Canonical QEMU harness + marker contract**: `scripts/qemu-test.sh` (do not duplicate marker lists here)
- **CI workflows**: `.github/workflows/ci.yml`, `.github/workflows/build.yml`
- **QEMU smoke proof gating (networking/DSoftBus)**: `docs/adr/0025-qemu-smoke-proof-gating.md`

## Networking

- `networking-authority.md` — Canonical vs alternative networking paths, anti-drift rules
- `network-address-matrix.md` — Normative address/subnet/profile matrix (QEMU + os2vm)
- **RFC-0006**: Userspace Networking v1 (sockets facade)
- **RFC-0007**: DSoftBus OS Transport v1 (UDP discovery + TCP sessions)
- **RFC-0008**: DSoftBus Noise XK v1 (handshake + identity binding)
- **RFC-0009**: no_std Dependency Hygiene v1 (OS build policy)
- **RFC-0027**: DSoftBusd modular daemon structure v1 (Completed)
- **RFC-0035**: DSoftBus QUIC v1 host-first scaffold contract (Done)
- **RFC-0036**: DSoftBus core no_std transport abstraction v1 (Complete; `TASK-0022` is Done)
- **ADR-0026**: Network address profiles + validation semantics

**Current snapshot**:
- Core networking transport, authenticated session flow, and dual-node harness behavior are established.
- Host-first QUIC transport proofs are available via `just test-dsoftbus-quic`; OS QUIC-v2 session behavior is now proven in `TASK-0023`.
- DSoftBus core no_std transport abstraction seam is implemented via `dsoftbus-core`; `TASK-0023` closes real OS session enablement with QUIC-required marker proofs, and follow-on work moves to `TASK-0024`/`TASK-0044` tuning breadth.
- Networking docs here focus on authority boundaries and invariants; rollout/proof state remains task-owned.

## Services and contracts

### Domain libraries (host-first)

- `03-samgr.md` — `userspace/samgr` (host-first registry library; OS uses `samgrd`)
- `04-bundlemgr-manifest.md` — canonical `manifest.nxb`/`BundleManifest` contract (Cap'n Proto) plus host parser constraints
- **Updates contract (v1.0)**: `docs/rfcs/RFC-0012-updates-packaging-ab-skeleton-v1.md` + `docs/packaging/system-set.md` (**Complete**)
- **Boot gates contract (v1.0)**: `docs/rfcs/RFC-0013-boot-gates-readiness-spawn-resource-v1.md` (**Complete**)

### OS daemons (authorities)

- `14-samgrd-service-manager.md` — `samgrd` (OS service registry authority)
- `15-bundlemgrd.md` — `bundlemgrd` (OS bundle/package authority)
- Config v1 authority: `tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md` + `docs/rfcs/RFC-0044-config-v1-configd-schema-layering-2pc-host-first-os-gated.md`
  - `configd` is the canonical typed config distribution authority
  - canonical runtime/persistence snapshots are Cap'n Proto
  - `nx config ...` is the only host CLI surface for config operations
- Policy as Code v1 authority: `tasks/TASK-0047-policy-as-code-v1-unified-engine.md` + `docs/rfcs/RFC-0045-policy-as-code-v1-unified-policy-tree-evaluator-explain-dry-run-learn-enforce-nx-policy.md`
  - `policyd` is the single policy decision authority
  - live policy authoring root is `policies/nexus.policy.toml`
  - Config v1 carries candidate roots as `policy.root`; no parallel policy reload plane
  - `nx policy ...` lives under `tools/nx`; see `tools/nx/README.md`

### UI/windowing authority

- `windowd` headless contract: `docs/rfcs/RFC-0047-ui-v1b-windowd-surface-layer-present-contract.md` (`TASK-0055` Done)
- `windowd` visible bootstrap contract: `docs/rfcs/RFC-0048-ui-v1c-visible-qemu-scanout-bootstrap-contract.md` (`TASK-0055B` Done)
- `windowd` visible SystemUI first-frame contract: `docs/rfcs/RFC-0049-ui-v1d-windowd-visible-present-systemui-first-frame-contract.md` (`TASK-0055C` Done)
- `windowd` v2a present scheduler + input routing contract: `docs/rfcs/RFC-0050-ui-v2a-present-scheduler-double-buffer-input-routing-contract.md` (`Done`; `TASK-0056` execution closeout complete)
- Dedicated architecture decision: `docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md`
- Scope rule: headless `TASK-0055`, visible `TASK-0055B`/`TASK-0055C`, and v2a `TASK-0056` closure must not be interpreted as cursor polish, perf/latency closure, WM-v2 breadth, full display-service integration, or kernel production-grade closure; those remain follow-up scope (`TASK-0056B`, `TASK-0056C`, `TASK-0199`/`TASK-0200`, `TASK-0251`).

## On-device inference

- `nexusinfer-techniques.md` — catalog of confirmed upstream and candidate local-inference techniques (PLE, effective parameters, KV policies, TurboQuant-like compression)
- `nexusinfer-runtime-profiles.md` — hardware-agnostic runtime/profile vocabulary for CPU/NPU/future compute executors
- `nexusinfer-rust-design.md` — Rust ownership, newtypes, `Send`/`Sync`, and zero-copy guidance for NexusInfer

**Current snapshot**:
- NexusInfer is tracked as a hardware-agnostic, local-first stack with CPU reference execution first.
- Documentation here intentionally avoids CUDA/Tensor-Core assumptions so future Imagination/NexusGfx or NPU paths can fit behind the same contracts.

## Graphics and compute

- `nexusgfx-compute-and-executor-model.md` — layer model for `NexusGfx`, compute executors, shared primitives, and `NexusInfer` relationship
- `nexusgfx-resource-model.md` — buffers/images/transient resources/import-export posture
- `nexusgfx-sync-and-lifetime.md` — fences, waits, ownership return, present pacing, reset posture
- `nexusgfx-command-and-pass-model.md` — command buffers, render/compute/copy passes, and pass-locality rules
- `nexusgfx-compute-kernel-model.md` — portable compute-kernel and dispatch model for graphics-adjacent and scientific workloads
- `nexusgfx-tile-aware-design.md` — bandwidth-first/mobile/tile-aware design stance for likely Imagination-style GPUs
- `nexusgfx-text-pipeline.md` — renderer-facing text acceleration posture aligned to existing UI layout/text contracts
- `nexusgfx-artifact-pipeline.md` — offline-first, deterministic, signed artifact strategy for shaders/kernels/pipelines
- `nexusgfx-capability-matrix.md` — backend capability vocabulary instead of vendor-first design

**Current snapshot**:
- `NexusGfx` is documented as an explicit, hardware-agnostic acceleration stack with CPU reference execution first.
- The graphics/compute docs intentionally assume probable mobile/tile-aware hardware and avoid CUDA-first or legacy-compatibility-first design.

## Observability

- **Logging guide**: `docs/observability/logging.md` — logd v1 usage + crash reports
- **RFC-0003**: Unified logging facade (`nexus-log`)
- **RFC-0011**: logd journal + crash reports v1 (Complete)
- **RFC-0031**: crashdump v1 deterministic minidumps + host symbolization (Complete)

**Current snapshot**:
- Logging, metrics/tracing export, and crash-report flows are active and validated through deterministic tests.
- Security-hardening specifics and reject-path evidence are tracked in dedicated execution documents.

## Policy Authority + Audit

- **Policy flow**: `11-policyd-and-policy-flow.md` — `policyd` as single authority
- **RFC-0015**: Policy Authority & Audit Baseline v1 (Complete)
- **RFC-0045**: Policy as Code v1 unified policy tree/evaluator/`nx policy` (Done host-first; OS/QEMU markers gated)
- **Security docs**: `docs/security/signing-and-policy.md`
- **Policy as Code docs**: `docs/security/policy-as-code.md`

**Current snapshot**:
- Policy authority remains single-source and deny-by-default, with audit evidence as a first-class proof surface.
- Policy v1 reload candidates flow through Config v1 `policy.root`; `policies/manifest.json` is required validation evidence for the active tree hash.
- Device identity and keystore flows are integrated into the same authority model without introducing parallel policy sources.

Related:

- **RFC-0016**: Device Identity Keys v1 (virtio-rng + rngd + keystored) — `docs/rfcs/RFC-0016-device-identity-keys-v1.md`
- Identity/keystore onboarding: `13-identity-and-keystore.md`

## Onboarding landing pages (this directory)

- `05-system-map-and-boundaries.md`
- `06-boot-and-bringup.md`
- `07-contracts-map.md`
- `08-service-architecture-onboarding.md`
- `09-nexus-init.md`
- `10-execd-and-loader.md`
- `11-policyd-and-policy-flow.md`
- `12-storage-vfs-packagefs.md`
- `13-identity-and-keystore.md`
- `14-samgrd-service-manager.md`
- `15-bundlemgrd.md`

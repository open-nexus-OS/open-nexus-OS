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

**Current Status (2026-02-10)**:
- TASK-0012: implementation complete, status **In Review** (deterministic anti-fake SMP proofs green)
- RFC-0021: **Complete** (contract + proof checklist aligned with `REQUIRE_SMP=1` SMP marker gating)

## Testing + CI

- `02-selftest-and-ci.md` — how we validate: deterministic markers + CI wiring (high level)
- **Canonical QEMU harness + marker contract**: `scripts/qemu-test.sh` (do not duplicate marker lists here)
- **CI workflows**: `.github/workflows/ci.yml`, `.github/workflows/build.yml`
- **QEMU smoke proof gating (networking/DSoftBus)**: `docs/adr/0025-qemu-smoke-proof-gating.md`

## Networking

- `networking-authority.md` — Canonical vs alternative networking paths, anti-drift rules
- **RFC-0006**: Userspace Networking v1 (sockets facade)
- **RFC-0007**: DSoftBus OS Transport v1 (UDP discovery + TCP sessions)
- **RFC-0008**: DSoftBus Noise XK v1 (handshake + identity binding)
- **RFC-0009**: no_std Dependency Hygiene v1 (OS build policy)

**Current Status (2026-01-07)**:
- TASK-0003/3B/3C: ✅ Done (loopback scope)
- TASK-0004: Next (dual-node + identity binding)

## Services and contracts

### Domain libraries (host-first)

- `03-samgr.md` — `userspace/samgr` (host-first registry library; OS uses `samgrd`)
- `04-bundlemgr-manifest.md` — `userspace/bundlemgr` TOML manifest parser (developer/tests; OS packaging contract is `manifest.nxb`)
- **Updates contract (v1.0)**: `docs/rfcs/RFC-0012-updates-packaging-ab-skeleton-v1.md` + `docs/packaging/system-set.md` (**Complete**)
- **Boot gates contract (v1.0)**: `docs/rfcs/RFC-0013-boot-gates-readiness-spawn-resource-v1.md` (**Complete**)

### OS daemons (authorities)

- `14-samgrd-service-manager.md` — `samgrd` (OS service registry authority)
- `15-bundlemgrd.md` — `bundlemgrd` (OS bundle/package authority)

## Observability

- **Logging guide**: `docs/observability/logging.md` — logd v1 usage + crash reports
- **RFC-0003**: Unified logging facade (`nexus-log`)
- **RFC-0011**: logd journal + crash reports v1 (Complete)

**Current Status (2026-01-14)**:
- TASK-0006: ✅ Done (logd RAM journal, nexus-log sink, execd crash reports, core service wiring)
- TASK-0014: Done (metrics/tracing exports via logd implemented and closure accepted)

## Policy Authority + Audit

- **Policy flow**: `11-policyd-and-policy-flow.md` — `policyd` as single authority
- **RFC-0015**: Policy Authority & Audit Baseline v1 (Complete)
- **Security docs**: `docs/security/signing-and-policy.md`

**Current Status (2026-01-25)**:
- TASK-0008: ✅ Done (policy engine, audit trail, policy-gated operations)
- TASK-0008B: ✅ Done (device identity keys via virtio-rng + rngd authority + keystored keygen)

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

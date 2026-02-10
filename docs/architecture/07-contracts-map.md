# Contracts map (onboarding)

This page is a **map of contracts**: “what is stable”, “where is it specified”, and “how is it proven”.

Rule: **do not treat this page as the contract**. It should link to the canonical sources so it stays small and drift-resistant.

## Authority model (where truth lives)

- **Tasks** (`tasks/TASK-*.md`): scope, DoD/stop conditions, proof commands, allowed touched paths.
- **RFCs** (`docs/rfcs/RFC-*.md`): interfaces/contracts/invariants and versioning strategy.
- **ADRs** (`docs/adr/*.md`): narrow decisions and rationale.
- **Harnesses/tests**: executable proof (e.g. `scripts/qemu-test.sh`, `cargo test -p …`).

## Core contracts (current)

### Kernel IPC + capability model

- **Contract**: kernel-enforced IPC semantics and capability rules
- **Canonical spec**: `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md`
- **Why it matters**: makes “process-per-service” real without in-proc shortcuts
- **Proof**: host tests + QEMU smoke markers (see `docs/testing/index.md` and `scripts/qemu-test.sh`)

### Loader safety / mapping invariants

- **Contract**: loader guardrails (W^X, provenance, mapping rules)
- **Canonical spec**: `docs/rfcs/RFC-0004-safe-loader-guards.md`
- **Related**: `docs/adr/0002-nexus-loader-architecture.md`

### Runtime roles & boundaries

- **Contract**: single sources of truth per role (init/spawner/loader), host parity, OS-lite gating
- **Canonical decision**: `docs/adr/0001-runtime-roles-and-boundaries.md`
- **Workflow/anti-drift**: `tasks/README.md`

### Packaging (`.nxb`) and bundle execution handshake

- **Contract**: canonical `.nxb` layout and `bundlemgrd` ↔ `execd` handshake
- **Canonical doc**: `docs/packaging/nxb.md`
- **Related**: `docs/adr/0009-bundle-manager-architecture.md`, `docs/adr/0007-executable-payloads-architecture.md`
- **Drift warning**: `docs/bundle-format.md` is explicitly legacy/drifted; don’t treat it as the OS contract.

### Updates (`.nxs` system-set + A/B skeleton)

- **Contract**: system-set archive format, `updated` RPCs, init health gate, slot-aware publication
- **Canonical spec**: `docs/rfcs/RFC-0012-updates-packaging-ab-skeleton-v1.md`
- **Packaging doc**: `docs/packaging/system-set.md`
- **Proof**: `tests/updates_host` + QEMU markers in `scripts/qemu-test.sh`

### Logging / markers discipline

- **Contract**: deterministic, honest markers; no fake success
- **Canonical practice**: `docs/testing/index.md` + `scripts/qemu-test.sh`
- **Standards**: `docs/standards/DOCUMENTATION_STANDARDS.md`

### Observability (logd journal + crash reports)

- **Contract**: bounded RAM journal (APPEND/QUERY/STATS), crash report envelope, core service integration
- **Canonical spec**: `docs/rfcs/RFC-0011-logd-journal-crash-v1.md` (Complete)
- **Related**: `docs/rfcs/RFC-0003-unified-logging.md` (nexus-log facade)
- **Task**: `tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md` (In Review)
- **Proof**: host tests (`cargo test -p logd`, `cargo test -p nexus-log`) + QEMU markers (5 required, all green as of 2026-01-14)
- **Why it matters**: provides bounded, deterministic log collection and crash reporting without kernel changes; core services (`samgrd`, `bundlemgrd`, `policyd`, `dsoftbusd`) emit structured logs to `logd` via `nexus-log`

### Boot gates (readiness + spawn reasons + resource sentinel)

- **Contract**: readiness vs up semantics, spawn failure reason taxonomy, resource/leak sentinel gate
- **Canonical spec**: `docs/rfcs/RFC-0013-boot-gates-readiness-spawn-resource-v1.md` (Complete)
- **Task**: `tasks/TASK-0269-boot-gates-v1-readiness-spawn-resource.md` (Complete)
- **Proof**: `KSELFTEST: spawn reasons ok`, `KSELFTEST: resource sentinel ok` + readiness contract in `docs/testing/index.md`
- **Why it matters**: deterministic early gates that turn "mysterious boot regressions" into actionable failures

### Testing contracts (host-first + phased QEMU)

- **Contract**: service contract tests, phased QEMU smoke gates, deterministic failure reporting
- **Canonical spec**: `docs/rfcs/RFC-0014-testing-contracts-and-qemu-phases-v1.md` (Complete)
- **Proof**: `just test-e2e`, `cargo test -p logd-e2e`, `cargo test -p e2e_policy`, `just test-os`
- **Why it matters**: prevents multi-day QEMU debugging by shifting left to host-first contract tests

### Kernel SMP v1 baseline

- **Contract**: deterministic SMP bring-up + per-CPU ownership boundaries + anti-fake IPI evidence chain
- **Canonical spec**: `docs/rfcs/RFC-0021-kernel-smp-v1-percpu-runqueues-ipi-contract.md` (Complete)
- **Task**: `tasks/TASK-0012-kernel-smp-v1-percpu-runqueues-ipis.md` (In Review)
- **Proof**: `SMP=2 REQUIRE_SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh` and `SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
- **Why it matters**: keeps SMP proofs causal and deterministic, and prevents fake-positive marker greens.

### Device MMIO access model v1

- **Contract**: capability-gated MMIO mapping, init-controlled distribution, policy deny-by-default, per-device windows
- **Canonical spec**: `docs/rfcs/RFC-0017-device-mmio-access-model-v1.md` (Done)
- **Task**: `tasks/TASK-0010-device-mmio-access-model.md` (Done)
- **Proof**: kernel negative tests (`test_reject_mmio_*`) + QEMU markers (`SELFTEST: mmio map ok`, `rngd: mmio window mapped ok`, `virtioblkd: mmio window mapped ok`, `SELFTEST: mmio policy deny ok`)
- **Testing guide**: `docs/testing/device-mmio-access.md`
- **Fast local run**: `just test-mmio` (RUN_PHASE=mmio)
- **Why it matters**: establishes userspace driver capability model + policy enforcement for device access; enables virtio-rng (keystored), virtio-net (netstackd), virtio-blk (virtioblkd)

## Adding a new contract

When a change introduces a new stable interface or cross-boundary behavior:

1. Create/extend an **RFC** describing the contract + versioning + failure model.
2. Create/extend a **task** that adds DoD + proof commands/markers.
3. If it’s a narrow architectural decision, add an **ADR** (one decision, one rationale).
4. Ensure the proof is enforced (tests/harness), not just documented.

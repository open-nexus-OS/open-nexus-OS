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

## Adding a new contract

When a change introduces a new stable interface or cross-boundary behavior:

1. Create/extend an **RFC** describing the contract + versioning + failure model.
2. Create/extend a **task** that adds DoD + proof commands/markers.
3. If it’s a narrow architectural decision, add an **ADR** (one decision, one rationale).
4. Ensure the proof is enforced (tests/harness), not just documented.

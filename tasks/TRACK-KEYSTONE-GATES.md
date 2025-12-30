---
title: TRACK Keystone Gates (closure plan for a coherent OS)
status: Draft
owner: @runtime
created: 2025-12-30
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Authority & naming registry: tasks/TRACK-AUTHORITY-NAMING.md
---

## Purpose

This track turns “gated on X” into a **closure plan**: once these gates are closed, the system stops being a set of islands and becomes a coherent, testable OS core.

## Gate 1 — Kernel IPC + capability transfer (RFC-0005)

### Why it is keystone (Gate 1)

Process-per-service is already the architectural decision. To make services real (not in-proc stubs), we need:

- reliable endpoint capabilities,
- capability transfer semantics,
- bounded, deterministic IPC framing.

### Unblocks (Gate 1)

- real cross-process `contentd`/`grantsd`/`windowd`/`inputd` flows,
- VMO/filebuffer data plane proofs (`TASK-0031`),
- “no fake success” selftests that actually exercise service boundaries.

### References (Gate 1)

- RFC: `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md`
- Closure task (minimum viable slice): `TASK-0267`

### Closure definition (DoD, Gate 1)

- QEMU selftest markers exist and are stable:
  - `SELFTEST: ipc v1 channel create ok`
  - `SELFTEST: ipc v1 sendrecv ok ...`
  - `SELFTEST: ipc v1 capxfer ok ...`
  - `SELFTEST: ipc v1 backpressure ok`
  - (see `TASK-0267`)

Note: kernel concurrency rules (SMP/locking/proof style) are defined in `TASK-0277` to avoid “parallelism ideology drift” in kernel work.

## Gate 2 — Safe userspace MMIO (device access model)

### Why it is keystone (Gate 2)

Your vision is userspace drivers. On QEMU `virt`, virtio and many peripherals are MMIO.

### Closure task (Gate 2)

- `TASK-0010` (device MMIO capability + mapping primitive)

### Closure definition (DoD, Gate 2)

- QEMU proof exists that:
  - a userspace driver can map a permitted MMIO range via a capability,
  - an attempt to map an unpermitted range is deterministically denied,
  - at least one virtio-mmio userspace driver (blk or net) can perform a bounded smoke test.

### Unblocks (Gate 2)

- userspace virtio-blk and virtio-net (persistence + networking),
- i2c/spi device buses and sensor drivers,
- simplefb mapping (`fbdevd`) and other device-class services.

## Gate 3 — Durable `/state` substrate

### Why it is keystone (Gate 3)

Without real persistence, everything else is either fake or volatile: grants, OTA state, crash retention, quotas, write-path recovery.

### Closure tasks (minimum set, Gate 3)

- `/state` bring-up: `TASK-0009`
- write-path integrity + atomicity hardening: `TASK-0025`
- quotas + error semantics: `TASK-0133`, `TASK-0132`

### Closure definition (DoD, Gate 3)

- `/state` mounts on QEMU consistently and survives reboot.
- Durable write-path semantics are proven (atomic replace, fsync barriers) per `TASK-0025`/`TASK-0264`.
- Quota enforcement and error semantics are proven (bounded, deterministic) per `TASK-0132`/`TASK-0133`.

### Unblocks (Gate 3)

- persistable grants (`grantsd`),
- recovery tooling (`TASK-0051`, `.nxra` replay protection),
- crash retention + bugreport bundles,
- durable content write-path (`TASK-0264`/`TASK-0265`),
- OTA/slot state machines beyond “soft reboot”.

## Gate 4 — Canonical packaging contracts

### Why it is keystone (Gate 4)

Supply-chain and updates cannot be coherent if the bundle manifest format drifts.

### Closure tasks (Gate 4)

- canonical manifest for bundles: `TASK-0129`
- update skeleton explicitly depends on the canonical manifest: `TASK-0007`
- supply-chain v1 SBOM/repro/sign policy must not cement legacy formats: `TASK-0029`

### Closure definition (DoD, Gate 4)

- `.nxb` + `manifest.nxb` is the only bundle contract referenced by tasks.
- Updates/install flows reference the canonical manifest (`TASK-0129` + `TASK-0007`) and do not invent aliases.
- Supply-chain policy and signing verifies deterministically on host.

## Gate 5 — Authority & naming consolidation

### Why it is keystone (Gate 5)

If two binaries claim the same role, the system stops being auditable and testable.

### Closure mechanism (Gate 5)

- enforce the canonical registry in `TRACK-AUTHORITY-NAMING.md`
- binding decision contract: `TASK-0266`
- repo placeholder replacement pass: `TASK-0273`
- whenever repo reality contains legacy placeholders (e.g. `powermgr`, `batterymgr`, `ime`, `compositor`), the implementation work must **replace/rename/remove** them so only the canonical authorities remain.
  (No compatibility promises are required for placeholders in planning/bring-up.)

### Closure definition (DoD, Gate 5)

- No parallel daemons exist for the same authority domain (see `TASK-0273` stop conditions).
- Readiness markers and samgr identities use only canonical names.

## Gate 6 — Single CLI entrypoint (`nx` convergence)

### Why it is keystone (Gate 6)

A coherent OS needs coherent tooling: if every subsystem ships its own `nx-*` binary, we get duplicated logic,
inconsistent outputs, and “proof markers” that drift by subsystem.

### Closure tasks (Gate 6)

- Base CLI: `TASK-0045`
- Convergence/no-drift pass: `TASK-0268`

### Closure definition (DoD, Gate 6)

- `nx` is the only CLI binary required by tasks for diagnostics.
- Any `nx-*` binaries (if present) are wrapper-only and forward exit codes faithfully.

# ADR-0001: Runtime Roles & Boundaries (Host + OS-lite)

Status: Accepted
Date: 2025-10-23
Owners: @runtime @init-team

## Context
The system evolved multiple components targeting similar responsibilities (init-lite, execd, nexus-loader, kernel user_loader, demo payloads). Complexity and duplicated concepts increased token usage and slowed progress.

## Decision
Define single sources of truth per role, maintain host parity, and gate OS-lite. Consolidate overlapping implementations into clear modules:

- Init (single): `source/init/nexus-init` with `std_server` (Host) and `os_lite` backends.
- Spawner (single): `source/services/execd` spawns services/tasks.
- Loader (single library): `userspace/nexus-loader` provides user program load/ELF/ABI routines.
- Kernel user loader: `source/kernel/neuron/src/user_loader.rs` remains a thin ABI bridge only.
- Test payloads: `userspace/exec-payloads`, `demo-exit-0` are fixtures/tests only.
- apps/init-lite: deprecated or wrapper that defers to `nexus-init`.

## Rationale
- Reduce duplication, clarify module ownership, and enforce boundaries for production-grade code.
- Keep changes iterative and scoped; preserve UART markers and host compatibility.

## Consequences
- Minor deprecation churn (init-lite), simplified loader usage across services.
- CI must enforce boundaries and duplicates.

## Invariants
- UART markers remain unchanged: `packagefsd: ready`, `vfsd: ready`, `SELFTEST`.
- Host path remains byte-compatible.
- OS-lite code behind `cfg(feature = "os-lite")`.

## Implementation Plan
1. Add standard headers to key modules and link this ADR.
2. Mark `source/apps/init-lite` deprecated with README.
3. Make `execd` use `userspace/nexus-loader` for ELF/load logic.
4. Keep `kernel::user_loader` as ABI bridge; remove duplicate loader code there.
5. Add boundary configs and CI checks (headers present, forbidden deps, jscpd, udeps, deny).

## References
- `docs/agents/PLAYBOOK.md`
- `tasks/TASK-0001-runtime-roles-and-boundaries.md`


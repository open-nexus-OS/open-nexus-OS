# TASK-0001: Runtime Roles & Boundaries (Host + OS-lite)

Status: active
Owner: @init-team @runtime
Scope: init (nexus-init), execd, userspace/nexus-loader, kernel user_loader, apps/init-lite

## Goal
Establish clear runtime roles for Host and OS-lite paths; freeze duplicates; create a minimal deprecation path. Keep changes iterative and scoped. Preserve production-grade invariants.

## Out of Scope
- Kernel internals unrelated to user-space spawn ABI
- Feature additions beyond role consolidation

## Invariants
- Preserve UART markers: `packagefsd: ready`, `vfsd: ready`, `SELFTEST`.
- Host path remains byte-compatible and unchanged where required.
- OS-lite code is gated behind `cfg(feature = "os-lite")`.
- Single source of truth per role (no duplicate loaders/spawners).

## Roles (target end-state)
- Init (single): `source/init/nexus-init` handles Host and OS-lite backends.
- Spawner (single): `source/services/execd` is the task/service spawner.
- Loader (single library): `userspace/nexus-loader` provides load/ELF/ABI routines.
- Kernel user loader: `source/kernel/neuron/src/user_loader.rs` is a thin ABI bridge only.
- Test payloads: `userspace/exec-payloads`, `demo-exit-0` remain tests/fixtures.
- apps/init-lite: deprecated or wrapper that defers to `nexus-init`.

## Steps (small PRs)
1. Add file headers to key modules with CONTEXT/OWNERS/API/DEPENDS/INVARIANTS.
2. Create ADR-0001 documenting roles and boundaries; link from headers.
3. Mark `apps/init-lite` deprecated with README and pointer to `nexus-init`.
4. Ensure `execd` depends on and uses `userspace/nexus-loader` for loading.
5. Keep `kernel::user_loader` minimal; remove duplicate loader logic.
6. Add boundary configs and CI checks (headers present, forbidden deps, jscpd).

## Entry Points
- Init: `source/init/nexus-init/src/lib.rs` (std_server, os_lite backends)
- Execd: `source/services/execd/src/lib.rs`
- Loader: `userspace/nexus-loader/src/lib.rs`
- Kernel bridge: `source/kernel/neuron/src/user_loader.rs`
- Deprecated target: `source/apps/init-lite`

## Stop conditions (for this task)
- ADR-0001 exists and linked.
- Deprecation note in `apps/init-lite` committed.
- At least one concrete call site in `execd` uses `nexus-loader`.

## Test plan
- Host: `cargo test --workspace` remains green.
- OS-lite: `just test-os` shows init markers and downstream readiness unchanged.

## Audit findings (2025-10-23)
- Init selection confirmed in `source/init/nexus-init/src/lib.rs` (`std_server` vs `os_lite`).
- OS-lite bootstrap (`src/os_lite.rs`) spawns seven services via `spawn_service`, emits `init: up <svc>`, yields between launches.
- `execd` already uses `userspace/nexus-loader` (`OsMapper`, `StackBuilder`) for ELF mapping on `nexus_env="os"`.
- Kernel `source/kernel/neuron/src/user_loader.rs` implements an ELF mapper in-kernel; treat as temporary bridge (duplicates userspace loader logic), to be trimmed later.
- `source/apps/init-lite` prints markers and idles; duplicates init role and will be deprecated/wrapped.

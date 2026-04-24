# Current Handoff: TASK-0039 closure + critical deltas applied

**Date**: 2026-04-24  
**Active execution task**: `tasks/TASK-0039-sandboxing-v1-vfs-namespaces-capfd-manifest.md` — `Done`  
**Contract seed**: `docs/rfcs/RFC-0042-sandboxing-v1-vfs-namespaces-capfd-manifest-permissions-host-first-os-gated.md` — `Done`  
**Tier policy**: `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` (Gate B: Security, Policy & Identity, `production-grade`)

## TASK-0039 execution snapshot

- Landed in allowlisted paths:
  - `vfsd` sandbox namespace + CapFd fail-closed helpers and reject tests.
  - `nexus-vfs` path reject guards.
  - `execd` spawn-boundary reject guard test.
  - OS-gated marker sync in selftest/proof-manifest + `scripts/qemu-test.sh`.
  - docs sync in task/rfc/testing/security.
- Host proof floor is green:
  - `cargo test -p vfsd -- --nocapture`
  - `cargo test -p nexus-vfs -- --nocapture`
  - `cargo test -p execd --lib test_reject_direct_fs_cap_bypass_at_spawn_boundary -- --nocapture`
  - `cargo check -p selftest-client`

## Closure deltas (current)

- No remaining technical gate blocker in TASK-0039 scope.
- Critical hardening deltas from post-closure audit are implemented and re-proven.

## Closure gate matrix (Go / No-Go)

- **Gate A (host reject floor)**: GO (green)
  - `vfsd`/`nexus-vfs`/`execd` reject proofs pass.
- **Gate B (OS marker floor)**: GO
  - Required run: `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`.
  - Required marker ladder:
    - `vfsd: namespace ready`
    - `vfsd: capfd grant ok`
    - `vfsd: access denied`
    - `SELFTEST: sandbox deny ok`
    - `SELFTEST: capfd read ok`
  - Verified markers include:
    - `vfsd: namespace ready`
    - `vfsd: capfd grant ok`
    - `vfsd: access denied`
    - `SELFTEST: sandbox deny ok`
    - `SELFTEST: capfd read ok`
- **Gate C (proof quality floor)**: GO
  - Added service-path reject proof: `test_reject_forged_capfd_service_path` (`vfsd` dispatcher path).
- **Gate D (doc/closure sync)**: GO
  - TASK/RFC phase checklists now reflect executed proofs.

## Kernel adjustment review outcome

- Typed memory-layout descriptor added (`AddressWindow`) for page-pool boundaries.
- `Send`/`Sync` review: no additional shared mutable state introduced; no unsafe trait work required.
- Ownership model remains static/immutable for layout constants (intended).

## Post-closure hardening outcomes

- Runtime spawn boundary check now executes in `execd` os-lite spawn path (deny on fs-cap boundary violation).
- `vfsd` os-lite now enforces per-handle owner identity (`sender_service_id`) for read/close.
- Proof rerun after hardening:
  - `cargo test -p execd -- --nocapture`
  - `cargo test -p vfsd -- --nocapture`
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`

## Guardrails

- Keep userspace-only boundary claims explicit (no kernel-enforced wording).
- No scope absorption of `TASK-0043` or `TASK-0189`.
- No fake-success marker claims without enforcing behavior.

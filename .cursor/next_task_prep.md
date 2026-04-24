# Next Task Preparation (Drift-Free)

## Candidate next execution

- **task**: `tasks/TASK-0039-sandboxing-v1-vfs-namespaces-capfd-manifest.md` — `Done`
- **contract**: `docs/rfcs/RFC-0042-sandboxing-v1-vfs-namespaces-capfd-manifest-permissions-host-first-os-gated.md` — `Done`
- **tier**: Gate B trajectory (`production-grade`) per `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`
- **follow-up route**: `TASK-0043`, `TASK-0189` (unchanged, explicit)

## Drift check vs repo state (2026-04-24)

- [x] Namespace/traversal reject floor implemented in `source/services/vfsd/**` + `userspace/nexus-vfs/**`.
- [x] CapFd authenticity/replay/rights reject tests exist in `source/services/vfsd/src/sandbox.rs`.
- [x] Spawn-time direct-fs-cap bypass reject test exists in `source/services/execd/src/lib.rs`.
- [x] Stable TASK-0039 marker literals are wired into manifest + qemu harness paths.
- [x] Task/RFC/testing/security docs updated for current proof shape.
- [x] QEMU marker run for TASK-0039 ladder captured in this cut.
- [x] Closure checkbox sync (`TASK-0039` + `RFC-0042` phase/status sections) after all gates green.

## Acceptance criteria status (next cut)

### Host (mandatory)

- [x] Namespace confinement reject proofs present (`test_reject_path_traversal`, `test_reject_unauthorized_namespace_path`).
- [x] CapFd integrity/replay/rights reject proofs present (`test_reject_forged_capfd`, `test_reject_replayed_capfd`, `test_reject_capfd_rights_mismatch`).
- [x] Capability-distribution boundary proof present (`test_reject_direct_fs_cap_bypass_at_spawn_boundary`).
- [x] Service-path integration reject exists (`test_reject_forged_capfd_service_path`).

### OS / QEMU (gated)

- [x] Marker strings registered as stable labels:
  - `vfsd: namespace ready`
  - `vfsd: capfd grant ok`
  - `vfsd: access denied`
  - `SELFTEST: sandbox deny ok`
  - `SELFTEST: capfd read ok`
- [x] Run and archive `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os` evidence for this ladder.

## Done condition (next closure step)

- Complete TASK/RFC closure only after host + OS gate set is green and proof artifacts are mirrored in SSOT docs.

## Immediate closure checklist (no scope drift)

- [x] Flip `TASK-0039` status line from `In Progress` to closure state after final review.
- [x] Flip `RFC-0042` status line from `In Progress` to closure state after final review.
- [x] Sync `tasks/STATUS-BOARD.md` queue head/contract line.
- [x] Sync `docs/rfcs/README.md` RFC-0042 index status line.
- [x] Keep follow-up scope explicit and untouched (`TASK-0043`, `TASK-0189`).

## Follow-up readiness contract (for TASK-0043 / TASK-0189)

- [x] v1 boundary remains userspace-only and explicitly documented.
- [x] Runtime spawn fs-cap boundary check is wired in `execd` path (not test-only).
- [x] vfsd os-lite handle ownership is subject-bound for read/close.
- [ ] Dynamic per-subject namespace/profile distribution remains follow-up scope (`TASK-0189`).
- [ ] Quota/egress enforcement breadth remains follow-up scope (`TASK-0043`).

## Go / No-Go checklist for 100% closure

- [x] **GO-1** Host reject suite is green.
- [x] **GO-2** OS marker gate run is captured and green (`RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`).
- [x] **GO-3** Service-path CapFd reject proof exists (not helper-only).
- [x] **GO-4** RFC-0042 implementation checklist is updated to executed evidence.
- [x] **GO-5** TASK-0039 stop-condition proof text mirrors final evidence.

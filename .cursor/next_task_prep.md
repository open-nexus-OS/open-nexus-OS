# Next Task Preparation (Drift-Free)

## Candidate next execution

- **recently closed**: `tasks/TASK-0032-packagefs-v2-ro-image-index-fastpath.md` — `Done`
- **task**: `tasks/TASK-0039-sandboxing-v1-vfs-namespaces-capfd-manifest.md` — `In Progress`
- **contract**: `docs/rfcs/RFC-0042-sandboxing-v1-vfs-namespaces-capfd-manifest-permissions-host-first-os-gated.md` — `In Progress`
- **tier**: production-grade trajectory per `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` (Gate B: Security, Policy & Identity)
- **follow-up route**: `TASK-0043`, `TASK-0189` (policy breadth + distribution hardening)

## Drift check vs real repo state

- [x] `TASK-0032` execution SSOT is set back to `Done`.
- [x] `RFC-0041` remains `Done` and linked to closed execution posture.
- [x] `TASK-0039` file exists and contains security section, threat model, and reject-proof expectations.
- [x] `RFC-0042` exists as contract seed and is linked from `TASK-0039`.
- [x] `TASK-0039` red flags now distinguish unresolved boundary truth from resolved manifest-format drift.
- [x] Tracking + handoff reflect task transition (`TASK-0032` archived, `TASK-0039` prep active).

## Acceptance criteria (for next cut: TASK-0039)

### Host (mandatory)

- Namespace confinement proofs exist and are deterministic (`pkg:/` reads in-bounds, traversal denied).
- CapFd integrity/replay reject suite exists and is fail-closed (`forged/tampered/replayed` denied).
- Capability-distribution boundary is explicit: apps do not hold direct fs-service caps.

### OS / QEMU (gated)

- Markers must be stable-label only (no variable values in marker strings):
  - `vfsd: namespace ready`
  - `vfsd: capfd grant ok`
  - `vfsd: access denied`
  - `SELFTEST: sandbox deny ok`
  - `SELFTEST: capfd read ok`

## Security checklist (mandatory)

- [x] Threat model is explicit for traversal, forgery, replay, manifest spoofing, capability bypass.
- [x] Invariants are explicit (deny-by-default, unforgeable CapFd, no direct fs caps to apps).
- [x] Real boundary honesty is explicit (userspace confinement only with kernel unchanged).
- [x] Production-grade gate mapping is explicit to Gate B with follow-up ownership unchanged.

## Linked contracts

- `tasks/TASK-0032-packagefs-v2-ro-image-index-fastpath.md`
- `tasks/TASK-0039-sandboxing-v1-vfs-namespaces-capfd-manifest.md`
- `tasks/TASK-0043-security-v2-sandbox-quotas-egress-abi-audit.md`
- `tasks/TASK-0189-sandbox-profiles-v2-sandboxd-or-policyd-distribution-ipc-vfs.md`
- `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`
- `docs/standards/SECURITY_STANDARDS.md`

## Done condition (current prep)

- Prep is complete; execution is now active for `TASK-0039` with RFC-0042 in progress.

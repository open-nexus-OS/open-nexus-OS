# Cursor Current State (SSOT)

## Current architecture state

- **last_decision (2026-04-24)**: `TASK-0039` gates are now fully green (host + OS marker ladder + service-path reject proof).
- **preserved boundary**: kernel remains untouched; v1 language stays userspace confinement only.
- **preserved follow-up split**:
  - `TASK-0043` keeps quota/egress/ABI-audit hardening breadth.
  - `TASK-0189` keeps profile distribution/policy-plumbing hardening.

## Active focus (execution)

- **active_task**: `tasks/TASK-0039-sandboxing-v1-vfs-namespaces-capfd-manifest.md` — `Done`
- **active_contract**: `docs/rfcs/RFC-0042-sandboxing-v1-vfs-namespaces-capfd-manifest-permissions-host-first-os-gated.md` — `Done`
- **latest_green_host_proofs**:
  - `cargo test -p vfsd -- --nocapture`
  - `cargo test -p nexus-vfs -- --nocapture`
  - `cargo test -p execd --lib test_reject_direct_fs_cap_bypass_at_spawn_boundary -- --nocapture`
  - `cargo check -p selftest-client`
- **tier_target**: Gate B (`production-grade`) per `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`.

## Active constraints (TASK-0039)

- No kernel-enforced claims; userspace boundary honesty is mandatory.
- No fake-success markers; marker additions must follow real behavior.
- Deterministic reject taxonomy must remain explicit (`test_reject_*`).
- Keep spawn authority centralized (`execd/init`) and deny direct app caps to fs services.

## Gate closure evidence (TASK-0039)

- **Gate A (host reject floor)**: green.
- **Gate B (OS marker floor)**: green via
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`,
  - observed markers: `vfsd: namespace ready`, `vfsd: capfd grant ok`, `vfsd: access denied`, `SELFTEST: sandbox deny ok`, `SELFTEST: capfd read ok`.
- **Gate C (proof robustness)**: green with service-path reject proof
  - `test_reject_forged_capfd_service_path`.
- **Kernel blocker resolution captured**:
  - POOL overlap fixed by relocating/synchronizing kernel page-pool mapping constants.
  - Follow-up heap OOM during child bring-up fixed by raising kernel heap budget to 2 MiB.
  - Post-fix hardening: typed memory window (`AddressWindow`) introduced for pool base/len usage to reduce base/len mixup risk.

## Kernel design review (newtype / Send-Sync / ownership)

- **Newtype/typing**: applied minimally where it adds safety without scope drift:
  - `AddressWindow` with `#[must_use] end()` for page-pool boundaries.
- **Ownership**: explicit value semantics (`Copy`) are sufficient for static layout descriptors.
- **Send/Sync**: no new concurrent mutable state introduced by these kernel fixes; no unsafe trait shortcuts needed.
- **Out-of-scope by design**: no allocator redesign, no kernel sandbox claims, no absorption of `TASK-0043` / `TASK-0189`.

## Closure plan (remaining 100% done steps)

- Final status/index sync completed:
  - `tasks/TASK-0039-sandboxing-v1-vfs-namespaces-capfd-manifest.md`
  - `docs/rfcs/RFC-0042-sandboxing-v1-vfs-namespaces-capfd-manifest-permissions-host-first-os-gated.md`
  - `tasks/STATUS-BOARD.md`
  - `docs/rfcs/README.md`

## Critical delta list (post-closure hardening pass)

- **DELTA-01 (runtime spawn boundary)**: helper-only boundary proof promoted into runtime path.
  - **Implemented**: `execd` os-lite spawn path now fail-closes if configured app caps violate fs-service boundary.
- **DELTA-02 (subject ownership floor in vfsd)**: cross-subject handle reuse hardening.
  - **Implemented**: `vfsd` os-lite binds handles to `sender_service_id` and denies read/close from non-owner subject.
- **DELTA-03 (proof revalidation after deltas)**:
  - **Implemented**: host tests and OS gate rerun green after delta changes.

## Residual risk kept explicit (no scope drift)

- Per-subject dynamic namespace manifests and profile-distributed policy are still follow-up scope (`TASK-0189`).
- Quota/egress enforcement breadth remains follow-up scope (`TASK-0043`).

## Contract links (active)

- `tasks/TASK-0039-sandboxing-v1-vfs-namespaces-capfd-manifest.md`
- `docs/rfcs/RFC-0042-sandboxing-v1-vfs-namespaces-capfd-manifest-permissions-host-first-os-gated.md`
- `docs/security/sandboxing.md`
- `docs/testing/index.md`
- `scripts/qemu-test.sh`
- `tasks/TASK-0043-security-v2-sandbox-quotas-egress-abi-audit.md`
- `tasks/TASK-0189-sandbox-profiles-v2-sandboxd-or-policyd-distribution-ipc-vfs.md`

## Carry-over note

- `TASK-0023B` external CI replay artifact closure remains independent and non-blocking for `TASK-0039`.

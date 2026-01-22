<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Updates v1.0 — A/B skeleton (non-persistent)

**Status**: Stable (v1.0 Complete)  
**Canonical contract**: `docs/rfcs/RFC-0012-updates-packaging-ab-skeleton-v1.md`  
**Execution truth**: `tasks/TASK-0007-updates-packaging-v1_1-userspace-ab-skeleton.md`

This document summarizes the v1.0 updates flow: a userspace-only A/B skeleton that is
testable without kernel or bootloader changes. State is RAM-backed and does not persist
across real reboots.

## Scope (v1.0)

- Stage a signed system-set (`.nxs`) into the standby slot.
- Switch to the standby slot via a soft switch.
- Commit health explicitly once the system is stable.
- Roll back if health is not committed before `triesLeft` reaches zero.

## Non-goals

- Persistence across reboots (blocked by `TASK-0009`).
- Real boot-chain slot signals (tracked in `TASK-0037`).
- Delta updates or per-bundle manifest fields (tracked in `TASK-0034`).
- Any update policy beyond signature verification (see `docs/security/signing-and-policy.md`).

## Flow summary

1. **Stage**: `updated` parses the `.nxs`, verifies the system signature via `keystored`,
   validates per-bundle digests, and stages bundles into the standby slot.
2. **Switch**: `updated.Switch()` sets `pending` and `triesLeft`, then calls
   `bundlemgrd` to republish from `/system/<slot>/`.
3. **Health commit**: `init` forwards a health-ok signal (from selftest or a future health
   source) to `updated.HealthOk()`.
4. **Rollback**: On boot attempt without health commit, `init` calls `updated.BootAttempt()`;
   if `triesLeft` reaches zero, `updated` signals a rollback slot and `bundlemgrd` republish occurs.

## Deterministic markers (QEMU)

These are enforced by `scripts/qemu-test.sh`:

- `updated: ready (non-persistent)`
- `bundlemgrd: slot a active`
- `SELFTEST: ota stage ok`
- `SELFTEST: ota switch ok`
- `init: health ok (slot <a|b>)`
- `SELFTEST: ota rollback ok`

## References

- System-set format: `docs/packaging/system-set.md`
- Bundle format: `docs/packaging/nxb.md`
- Init health gate: `docs/architecture/09-nexus-init.md`
- Slot publication: `docs/architecture/15-bundlemgrd.md`

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Updates v1.0 — A/B skeleton (non-persistent)

> **Historical — superseded in part (2026-08-25).** Two statements below drifted
> from shipped reality long ago: boot-control state has been PERSISTENT since
> TASK-0034 Goal 2 (`/state/boot/bootctl.v1`, owner `bootctld` per ADR-0055; the
> shipped marker is `updated: ready (bootctl client)`, not
> `updated: ready (non-persistent)`). The end-to-end OTA contract (component
> manifest, real A/B boot-image partitions, `nxboot` loader, anti-downgrade) is
> **RFC-0089** — read that first; this page stays as the v1 flow summary only.

**Status**: Historical (v1.0 flow summary; superseded in part by RFC-0089)  
**Canonical contract**: `docs/rfcs/RFC-0089-ota-v2-component-manifest-ab-boot-images-nxboot-bsb.md` (current) · `docs/rfcs/RFC-0012-updates-packaging-ab-skeleton-v1.md` (v1, superseded in part)  
**Execution truth**: `tasks/TASK-0007-updates-packaging-v1_1-userspace-ab-skeleton.md`

This document summarizes the v1.0 updates flow: a userspace-only A/B skeleton that is
testable without kernel or bootloader changes.

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

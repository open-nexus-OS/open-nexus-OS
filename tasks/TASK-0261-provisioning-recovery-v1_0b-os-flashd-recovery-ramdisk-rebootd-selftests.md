---
title: TASK-0261 Provisioning/Recovery v1.0b (OS/QEMU): flashd service + recovery ramdisk + rebootd + virtio-serial + selftests
status: Draft
owner: @reliability
created: 2025-12-29
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Authority & naming registry: tasks/TRACK-AUTHORITY-NAMING.md
  - Provisioning core (host-first): tasks/TASK-0260-provisioning-recovery-v1_0a-host-image-builder-flasher-protocol-deterministic.md
  - Recovery baseline: tasks/TASK-0050-recovery-v1a-boot-target-minimal-shell-diag.md
  - Recovery tools: tasks/TASK-0051-recovery-v1b-safe-tools-fsck-slot-ota-nx-recovery.md
  - Persistence: tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We need OS/QEMU integration for Provisioning/Recovery v1.0:

- `flashd` service (target side flasher),
- recovery ramdisk (initrd with flashd, nx-recovery, nx-diag),
- `rebootd` service (next-boot mode control),
- virtio-serial channel for flashing.

The prompt proposes these services. `TASK-0050`/`TASK-0051` already plan recovery mode (boot target, minimal shell, safe tools). This task delivers the **OS/QEMU integration** with `flashd`, recovery ramdisk, `rebootd`, and virtio-serial, complementing the existing recovery architecture.

## Goal

On OS/QEMU:

1. **Recovery target (initrd) & boot path**:
   - add `pkg://recovery/initrd.cpio` containing minimal userspace: `flashd` (see below), `nx-recovery` (menu), `nx-diag` (collects logs)
   - mounts tmpfs, exposes **virtio-serial** port `/dev/vport0p1` or UART `/dev/ttyS0`
   - on boot prints: `recovery: ready (serial=@/dev/vport0p1|/dev/ttyS0)`
   - add boot arg `recovery=1` to trigger recovery; default is normal boot
   - runner: env `RECOVERY=1` passes arg; otherwise standard
2. **flashd service** (`source/services/flashd/`):
   - accepts frames over chosen TTY: **magic + header + seq + len + payload + crc32** (using protocol from `TASK-0260`)
   - commands: `HELLO`, `INFO?`, `WRITE seg=<name> off=?`, `DONE`, `ABORT`
   - streams segments into staging file `state:/flash/staging.img`; validates per-segment SHA-256 against manifest; on `DONE` atomically installs to `build/nexus-os.img` location (or designated block device)
   - supports **resume** via last good seq
   - markers: `flashd: ready`, `flashd: hello host=… proto=1`, `flashd: wrote seg=rootfs bytes=…`, `flashd: verify ok`, `flashd: installed`
3. **rebootd service** (`source/services/rebootd/`):
   - small service with Cap'n Proto: `interface Reboot { mode(target:Text) -> (); }` where `target` is `"normal"|"recovery"`
   - persists next-boot flag `state:/boot/next_mode`
   - `nx flash reboot …` talks to `rebootd` if online; in recovery, `flashd` can write next-boot=normal after install
   - markers: `rebootd: ready`, `rebootd: next-boot=normal|recovery`
4. **Factory reset** (extend `nx reset ...` from `TASK-0260`):
   - SystemUI entry: double-confirmation flow
   - implementation: stop services, unmount user data, remove `state:/apps`, `state:/content`, `state:/settings`, preserve `pkg://trust/**`
   - markers: `nx: reset factory wipe start|done`
5. **Runner/QEMU wiring**:
   - add **virtio-serial**: `-device virtio-serial-device -chardev stdio,id=char0 -device virtserialport,chardev=char0,name=com.nexus.flash`
   - map it to `/dev/vport0p1` inside guest (documented)
   - provide `just recovery` recipe: boots with `RECOVERY=1`, waits for `recovery: ready …`, then example `nx flash send …`
6. **OS selftests + postflight**.

## Non-Goals

- Kernel changes.
- Real hardware (QEMU/virtio-serial only).
- Full recovery mode (handled by `TASK-0050`/`TASK-0051`).

## Constraints / invariants (hard requirements)

- **No duplicate recovery authority**: This task extends recovery mode from `TASK-0050`/`TASK-0051` with flashing capability, not a new recovery system.
- **No duplicate reboot authority**: `rebootd` is the single authority for next-boot mode control. Do not create parallel reboot services.
- **Determinism**: flashd protocol, rebootd state, and factory reset must be stable given the same inputs.
- **Bounded resources**: flashing is chunk-bounded; rebootd state is bounded.
- **Persistence gating**: staging file and next-boot flag require `/state` (`TASK-0009`) or equivalent. Without `/state`, these features must be disabled or explicit `stub/placeholder` (no "written ok" claims).
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED (recovery authority drift)**:
  - Do not create a parallel recovery system. This task extends recovery mode from `TASK-0050`/`TASK-0051` with flashing capability.
- **RED (reboot authority drift)**:
  - Do not create a parallel reboot service. `rebootd` is the single authority for next-boot mode control.
- **YELLOW (recovery ramdisk vs normal boot)**:
  - Recovery ramdisk is a separate initrd. Document the relationship explicitly: recovery ramdisk contains `flashd`, `nx-recovery`, `nx-diag`; normal boot uses standard initrd.

## Contract sources (single source of truth)

- QEMU marker contract: `scripts/qemu-test.sh`
- Provisioning core: `TASK-0260`
- Recovery baseline: `TASK-0050`/`TASK-0051` (recovery mode)
- Persistence: `TASK-0009` (prerequisite for `/state`)

## Stop conditions (Definition of Done)

### Proof (OS/QEMU) — gated

UART markers:

- `recovery: ready (serial=@/dev/vport0p1|/dev/ttyS0)`
- `flashd: ready`
- `flashd: hello host=… proto=1`
- `flashd: wrote seg=rootfs bytes=…`
- `flashd: verify ok`
- `flashd: installed`
- `rebootd: ready`
- `rebootd: next-boot=normal|recovery`
- `nx: reset factory wipe start|done`
- `SELFTEST: reset dry-run ok`
- `SELFTEST: flash receive ok` (only if `RECOVERY=1`; otherwise explicit `stub/placeholder`)
- `SELFTEST: flash reboot flag ok`

## Touched paths (allowlist)

- `pkg://recovery/initrd.cpio` (new)
- `source/services/flashd/` (new)
- `source/services/rebootd/` (new)
- `source/apps/recovery-init/` (extend: recovery ramdisk boot)
- `tools/nx/` (extend: `nx image|flash|reset ...`; no separate `nx-*` binaries)
- SystemUI (factory reset confirmation flow)
- `scripts/run-qemu-rv64.sh` (extend: virtio-serial channel)
- `justfile` (extend: `just recovery` recipe)
- `source/apps/selftest-client/` (markers)
- `docs/provisioning/overview.md` (new)
- `docs/provisioning/protocol.md` (new)
- `docs/tools/nx-image.md` (new)
- `docs/tools/nx-flash.md` (new)
- `docs/tools/nx-reset.md` (new)
- `docs/runner/recovery.md` (new)
- `tools/postflight-provisioning-recovery-v1_0.sh` (new)

## Plan (small PRs)

1. **Recovery ramdisk + boot path**
   - initrd.cpio with flashd, nx-recovery, nx-diag
   - boot arg `recovery=1`
   - runner wiring
   - markers

2. **flashd service**
   - flashd service (frame protocol, resume, verify)
   - markers

3. **rebootd service + factory reset**
   - rebootd service (next-boot mode)
   - factory reset SystemUI entry
   - markers

4. **OS selftests + postflight**
   - OS selftests
   - postflight

## Acceptance criteria (behavioral)

- Recovery ramdisk boots and exposes virtio-serial/UART correctly.
- `flashd` receives/validates an image via virtio-serial; resume works correctly.
- `rebootd` persists next-boot flag correctly.
- Factory reset wipes `state:/` except trust & boot paths.
- All three OS selftest markers are emitted (or explicit `stub/placeholder` if gated).

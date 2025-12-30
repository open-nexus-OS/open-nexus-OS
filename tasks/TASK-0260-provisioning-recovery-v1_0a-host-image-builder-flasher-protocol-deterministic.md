---
title: TASK-0260 Provisioning/Recovery v1.0a (host-first): deterministic image builder + flasher protocol + factory reset + deterministic tests
status: Draft
owner: @reliability
created: 2025-12-29
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Authority & naming registry: tasks/TRACK-AUTHORITY-NAMING.md
  - Recovery baseline: tasks/TASK-0050-recovery-v1a-boot-target-minimal-shell-diag.md
  - Recovery tools: tasks/TASK-0051-recovery-v1b-safe-tools-fsck-slot-ota-nx-recovery.md
  - NXB format: tasks/TASK-0129-packages-v1a-nxb-format-signing-pkgr-tool.md
  - Packagefs image builder: tasks/TASK-0246-bringup-rv-virt-v1_1a-host-virtio-blk-image-builder-deterministic.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We need a deterministic provisioning & recovery path:

- deterministic image builder (reproducible OS images),
- flasher protocol (chunked, CRC'd frames, resume),
- factory reset (wipe `state:/` except trust & boot).

The prompt proposes an image builder and flasher protocol. `TASK-0050`/`TASK-0051` already plan recovery mode (boot target, minimal shell, safe tools). This task delivers the **host-first core** (image builder, flasher protocol, factory reset) that can be reused by both OS/QEMU integration and host tests.

## Goal

Deliver on host:

1. **Image builder library** (`nx image ...` as a subcommand of the canonical `nx` tool):
   - compose image layout (raw file): `[bootloader/OpenSBI]`, `[kernel]`, `[initrd]` (recovery), `[rootfs.squashfs]`, `[pkgfs.img]`, `[state partition header]`
   - deterministic: fixed segment order, byte alignment, mtimes, owner=0:0
   - manifest includes SHA-256 per segment + overall image hash; `os.sig` is Ed25519 over manifest
   - CLI: `nx image build --profile dev|release --rootfs ... --pkgfs ... --out ... --manifest ... --sign ...`, `nx image verify ... --manifest ... --pub ...`
2. **Flasher protocol library** (`nx flash ...` as a subcommand of the canonical `nx` tool):
   - frame format: **magic + header + seq + len + payload + crc32**
   - commands: `HELLO`, `INFO?`, `WRITE seg=<name> off=?`, `DONE`, `ABORT`
   - resume support via last good seq
   - CLI: `nx flash send --img ... --manifest ... --port ... --baud 115200 --chunk 65536 --resume`, `nx flash verify --port ...`, `nx flash reboot normal|recovery`
3. **Factory reset library** (`nx reset ...` as a subcommand of the canonical `nx` tool):
   - wipe `state:/` (except trust & boot): remove `state:/apps`, `state:/content`, `state:/settings`, preserve `pkg://trust/**`
   - CLI: `nx reset factory --yes`
4. **Host tests** proving:
   - composer: build tiny image with two segments; verify manifest & signature; stable hash across runs
   - protocol: in-proc loopback of `nx flash` ↔ `flashd` with injected loss (drop every 5th frame) → resume works, final hash matches
   - factory reset: simulate state tree and ensure preserved paths excluded; output matches golden list

## Non-Goals

- OS/QEMU integration (deferred to v1.0b).
- Real hardware (QEMU/virtio-serial only).
- Full recovery mode (handled by `TASK-0050`/`TASK-0051`).

## Constraints / invariants (hard requirements)

- **No duplicate image format authority**: This task provides image builder library. `TASK-0129` already plans NXB format for bundles. This task focuses on OS image format (not bundle format). Document the relationship explicitly.
- **No duplicate manifest authority**: Image manifest should align with existing signing/verification primitives (e.g., `keystored`, `TASK-0029`). Do not create parallel signature semantics.
- **Determinism**: image builder, flasher protocol, and factory reset must be stable given the same inputs.
- **Bounded resources**: image building is size-bounded; flasher protocol is chunk-bounded.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED (image format authority drift)**:
  - Do not create parallel image formats. This task provides OS image format (bootloader/kernel/initrd/rootfs/pkgfs/state). `TASK-0129` provides NXB bundle format. Document the relationship explicitly.
- **RED (manifest authority drift)**:
  - Do not create parallel signature semantics. Image manifest signing should align with existing signing/verification primitives (e.g., `keystored`, `TASK-0029`).
- **YELLOW (factory reset safety)**:
  - Factory reset must preserve trust & boot paths. Document preserved paths explicitly.

## Contract sources (single source of truth)

- Testing contract: `scripts/qemu-test.sh`
- Recovery baseline: `TASK-0050`/`TASK-0051` (recovery mode)
- NXB format: `TASK-0129` (bundle format, not OS image format)
- Packagefs image builder: `TASK-0246` (packagefs image, not full OS image)

## Stop conditions (Definition of Done)

### Proof (Host) — required

`cargo test -p provisioning_recovery_v1_0_host` green (new):

- composer: build tiny image with two segments; verify manifest & signature; stable hash across runs
- protocol: in-proc loopback of `nx flash` ↔ `flashd` with injected loss (drop every 5th frame) → resume works, final hash matches
- factory reset: simulate state tree and ensure preserved paths excluded; output matches golden list

## Touched paths (allowlist)

- `tools/nx/` (extend: `nx image ...`, `nx flash ...`, `nx reset ...`; no separate `nx-image`/`nx-flash`/`nx-reset` binaries)
- `tests/provisioning_recovery_v1_0_host/` (new)
- `docs/provisioning/overview.md` (new, host-first sections)
- `docs/provisioning/protocol.md` (new)

## Plan (small PRs)

1. **Image builder**
   - image layout composer
   - manifest generation
   - signing hooks
   - host tests

2. **Flasher protocol**
   - frame format + commands
   - resume support
   - host tests

3. **Factory reset**
   - state tree wipe (preserve trust & boot)
   - host tests

4. **Docs**
   - host-first docs

## Acceptance criteria (behavioral)

- Image builder produces stable hashes across runs.
- Flasher protocol is resumable and robust to loss.
- Factory reset preserves trust & boot paths correctly.

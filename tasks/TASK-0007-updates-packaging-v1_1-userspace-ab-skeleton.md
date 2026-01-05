---
title: TASK-0007 Updates & Packaging v1.1 (OS): userspace-only A/B skeleton + system-set bundle index
status: Draft
owner: @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Testing contract: scripts/qemu-test.sh
  - Packaging docs: docs/packaging/nxb.md
  - Depends-on (persistence): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
---

## Context

We want the first verifiable OTA story without touching the kernel or bootloader:

**Staging → Switch → Health gate → Rollback**.

The goal is a **userspace skeleton** that is testable and honest:

- “switch” updates a userspace-controlled boot target (`bootctl.json`) and forces slot-aware publication,
  even if we cannot perform a real reboot yet.
- health is explicitly committed by userspace once the system is stable.
- rollback happens automatically when pending boots do not reach health.

In parallel, we need packaging metadata robust enough for staging:

- per-bundle digest/size metadata
- a signed system-set index that lists the bundles that form a slot.

Prompt mapping note (avoid drift from older plans):

- Some older prompts describe a “NUB” update bundle with `manifest.json` + `payload.tar` + `sig.ed25519`.
- This repo’s v1.1 decision is **not** to add a parallel JSON+tar update container.
- Canonical contracts here are:
  - **`.nxb`** bundle with **`manifest.nxb`** + `payload.elf` (see RED below), and
  - a **system-set** archive/index (tracked as `.nxs` direction in this task) that is signed and lists bundles+diges ts.

## Goal

Prove end-to-end (host tests + QEMU selftest markers):

- A system-set can be staged into the standby slot after verifying signatures + bundle digests.
- Switching sets `pending` + `triesLeft` and flips the active slot selection.
- A health commit clears `pending` for the new slot.
- A forced “no health” path triggers rollback back to the previous slot.

## Non-Goals

- Real persistent storage (block backend) and real reboot integration.
- Delta updates, streaming updates, compression tuning.
- Full policy language for updates (only minimal gating needed for the skeleton).
 - User-facing Settings UI and a developer CLI (`nx update`) (follow-up `TASK-0140`).

## Constraints / invariants (hard requirements)

- **Kernel untouched**.
- **No fake success**: OTA markers must be emitted only after real verification/state transitions.
- **Determinism**: staging/switch/rollback must be deterministic; tests must be bounded.
- **Bounded data**: system-set parsing and extraction must be size-capped; reject oversized inputs.
- **Rust hygiene**: no new `unwrap/expect` in OS daemons; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED (blocking / must decide now)**:
  - **Bundle manifest contract drift (must be fixed in this task)**:
    the repo currently has multiple “truths” (`manifest.json` in tooling/OS-lite vs `manifest.toml` in docs/tests).
    **Best-for-OS decision (no dual mode): standardize on a single, deterministic, signed manifest format.**

    Canonical v1.1 contract for `.nxb`:

    - **`manifest.nxb`**: a **versioned binary manifest** (fixed, deterministic encoding; no whitespace ambiguity),
      designed for signing/verifying and bounded parsing in userland.
    - **`payload.elf`**: ELF payload bytes.

    Tooling may provide `--print-json` / `--print-toml` *views*, but those are derived outputs; the on-disk contract is `manifest.nxb`.

    This must land *before* we add per-bundle digests/size so we do not bake drift into v1.1.

  - **Update payload format drift**:
    - Do not introduce `manifest.json` + tar as a second update payload contract.
    - Update payloads must use the **system-set direction** (signed index + bundle digests) described in this task.
- **YELLOW (risky / likely drift / needs follow-up)**:
  - **“No bootloader changes” means no real reboot**. For QEMU proofs we need an explicit, honest definition
    of “switch”:
    - Option A: a “soft switch” that causes bundlemgrd/packagefs to republish from the new slot in-process.
    - Option B: only prove state transitions (bootctl + health/rollback) and keep true slot activation for a later reboot task.
  - **Boot-chain contract (future, required for real A/B)**:
    the v1.1 skeleton proves *policy and state transitions* only. A production A/B system requires a real
    boot-time slot signal (bootloader/OpenSBI/firmware → early init) so we can prove “slot B actually booted”
    and make rollback affect the next real boot. This is tracked separately as `TASK-0037` (blocked).
  - **Signature verification in OS builds**: the existing `identity` crate is host-oriented; OS verification must
    either be implemented in a no_std-friendly way or delegated to a service API (e.g. keystored verify).
  - **Persistence reality**: until TASK-0009 lands, `bootctl` persistence is not real. If we simulate `/state` in RAM for bring-up,
    the task must label it explicitly as **non-persistent** and must not claim “OTA persisted” in markers.
- **GREEN (confirmed assumptions)**:
  - `bundlemgrd`/`packagefsd` already serve bundle payloads and manifests in OS-lite flows; we can extend that model slot-aware.

## Contract sources (single source of truth)

- **QEMU marker contract**: `scripts/qemu-test.sh`
- **Packaging contract (today)**:
  - `.nxb` layout is documented in `docs/packaging/nxb.md` (needs alignment with code as part of this task).

## Stop conditions (Definition of Done)

### Proof (Host)

- Add a deterministic host test crate (e.g. `tests/updates_host`) that covers:
  - stage verifies signature + digests (and rejects mismatch)
  - switch flips target + sets pending + triesLeft
  - health commit clears pending
  - rollback triggers when triesLeft reaches 0 without health

### Proof (OS / QEMU)

- `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
  - Extend `scripts/qemu-test.sh` expected markers (order tolerant) with:
    - `updated: ready`
    - `bundlemgrd: slot a active` (or `b`, depending on initial state)
    - `SELFTEST: ota stage ok`
    - `SELFTEST: ota switch ok`
    - `init: health ok (slot <a|b>)`
    - `SELFTEST: ota rollback ok`

Notes:

- Postflight scripts (if added) must **only** delegate to canonical harness/tests; no `uart.log` greps as “truth”.
 - Update payload containers in this repo should follow the system-set direction from this task (no zip/manifest.json “NUP” contract without an explicit format decision).

### Docs gate (keep architecture entrypoints in sync)

- If packaging/manifest contract, init orchestration for updates, or bundle publication semantics change, update:
  - `docs/architecture/09-nexus-init.md`
  - `docs/architecture/15-bundlemgrd.md`
  - `docs/architecture/04-bundlemgr-manifest.md`
  - `docs/architecture/07-contracts-map.md`
  - and the index `docs/architecture/README.md`

## Touched paths (allowlist)

- `tools/nxb-pack/` (extend manifest output to v1.1 once contract chosen)
- `tools/` (add an `nxs-pack` tool for system-set creation/signing)
- `source/services/updated/` (new service: stage/switch/health API + marker)
- `source/init/nexus-init/` (boot target selection + tries/rollback + health commit)
- `source/services/bundlemgrd/` (slot-aware publication; marker)
- `source/apps/selftest-client/` (stage/switch/rollback markers)
- `scripts/qemu-test.sh` (canonical marker contract update)
- `docs/` (updates + packaging + testing docs)

## Plan (small PRs)

1. **Fix packaging contract drift (RED)**
   - Standardize `.nxb` on `manifest.nxb` + `payload.elf` and update docs/tests/tools accordingly (single source of truth).

2. **System-set format + host tooling**
   - Introduce a “system set index” file (JSON) listing bundles + digests.
   - Add `nxs-pack` tool to create a `.nxs` archive (tar of `system.json` + `.nxb` bundle dirs/files) and sign `system.json`.
   - Extend `nxb-pack` to emit v1.1 digest/size fields into `manifest.nxb`.

3. **Implement `updated` service**
   - Expose minimal RPC (OS-lite frame protocol) for:
     - StageSystem (verify + unpack into standby slot)
     - Switch (flip target, set pending, triesLeft=2)
     - HealthOk (commit)
   - Emit marker: `updated: ready`.

4. **Init integration (health gate + rollback)**
   - Early boot reads `/state/bootctl.json`:
     - if pending: decrement triesLeft; if exhausted, rollback to previous slot and clear pending
   - On stable boot (core markers + selftest end), call `updated.HealthOk()`
   - Emit marker: `init: health ok (slot <a|b>)`.

5. **bundlemgrd slot-aware publication**
   - Make bundlemgrd publish from `/system/<target>` and emit: `bundlemgrd: slot <a|b> active`.
   - Define “soft switch” behavior explicitly if we need runtime switching without reboot.

6. **Selftest proof**
   - Happy path: stage + switch + health ok marker.
   - Rollback path: force pending without health and verify rollback marker.
   - Keep everything bounded; no busy waits.

7. **Docs**
   - `docs/updates/ab-skeleton.md`: model, bootctl semantics, limitations.
   - Update packaging docs to match the chosen v1.1 manifest + system-set format.
   - `docs/testing/index.md`: how to run host tests + QEMU markers.

## Acceptance criteria (behavioral)

- Host tests cover stage/switch/health/rollback and negative cases deterministically.
- QEMU run prints the OTA markers listed above and `scripts/qemu-test.sh` passes.
- No kernel changes.

## Evidence (to paste into PR)

- Host: `cargo test -p updates_host -- --nocapture` summary
- OS: `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh` + `uart.log` tail with OTA markers

## RFC seeds (for later, once green)

- Decisions made:
  - canonical bundle manifest format + v1.1 fields
  - system-set archive format and signature scheme
  - meaning of “switch” without reboot (soft switch vs state-only)
- Open questions:
  - persistence backend and reboot integration
  - policy model for who may stage/switch/rollback

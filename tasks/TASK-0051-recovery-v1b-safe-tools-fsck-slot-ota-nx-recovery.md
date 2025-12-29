---
title: TASK-0051 Recovery Mode v1b: safe tools (fsck/slot/ota) + restricted statefs + nx recovery CLI + proofs
status: Draft
owner: @reliability
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Recovery v1a: tasks/TASK-0050-recovery-v1a-boot-target-minimal-shell-diag.md
  - Persistence (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Updates/OTA skeleton: tasks/TASK-0036-ota-ab-v2-userspace-healthmux-rollback-softreboot.md
  - Policy as Code (gates + redaction): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Config broker (read-only in recovery): tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
  - Networking / remote fetch (optional): tasks/TASK-0005-networking-cross-vm-dsoftbus-remote-proxy.md
  - DevX CLI: tasks/TASK-0045-devx-nx-cli-v1.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

Recovery v1a provides boot target + minimal graph + safe shell + diagnostics.
Recovery v1b adds the “operator tools” that make recovery actually useful:

- statefs check/repair (bounded),
- slot status/switch scheduling,
- OTA staging (no commit),
- and a host CLI `nx recovery` to drive it non-interactively.

## Goal

Deliver:

1. `statefsd` check/repair API, only enabled in recovery or explicitly configured.
2. Recovery shell commands:
   - `fsck state [--repair]`
   - `slot status`
   - `slot switch <A|B> ...` (schedule only; no commit)
   - `ota stage <path>` (stage only; no commit)
3. `updated` “recovery-safe” helpers that refuse commit in recovery.
4. `nx recovery` host CLI to enter/leave, exec built-ins, and pull diag bundles.
5. Host tests + OS selftest markers + postflight (delegating to canonical proofs).

## Non-Goals

- Kernel changes.
- A full interactive shell.
- Remote OTA fetch by default (optional follow-up once DSoftBus and policy gates are solid).

## Constraints / invariants (hard requirements)

- Recovery remains “safe by default”:
  - RO mounts,
  - stage/schedule allowed,
  - commit blocked.
- Repairs are bounded and fully audited.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED (dependency gating)**:
  - Requires `/state` and `statefsd` (TASK-0009).
  - Slot management requires a stable userspace OTA/slot state machine (TASK-0036) or equivalent.
  - `nx recovery enter/leave` requires a boot target persistence mechanism; without a boot chain, proof is “soft-reboot simulated”.
- **YELLOW (repair safety)**:
  - Repair mode is powerful; must be denied unless:
    - booted into recovery, or
    - explicitly enabled by config/policy.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/recovery_host/`:

- `nx recovery enter/leave` toggles boot target state in a mocked device backend.
- `nx recovery diag` produces an archive with expected sections from fixture data.
- `nx recovery exec "fsck state"` and `"slot status"` run against a mocked shell endpoint.

### Proof (OS/QEMU) — gated

UART markers:

- `recovery: services ready`
- `recovery: shell ready`
- `recovery: fsck ok` / `recovery: fsck repaired` / `recovery: fsck fail`
- `recovery: slot status printed`
- `recovery: slot switch scheduled (to=...)`
- `recovery: ota staged`
- `updated: commit blocked in recovery`

Selftest markers:

- `SELFTEST: recovery fsck ok`
- `SELFTEST: recovery slot ok`
- `SELFTEST: recovery ota staged ok`

## Touched paths (allowlist)

- `source/services/statefsd/` (check/repair)
- `source/services/updated/` (recovery-safe helpers; commit blocked marker)
- `source/apps/recovery-sh/` (add fsck/slot/ota built-ins)
- `tools/nx/` (add `nx recovery ...`)
- `tests/recovery_host/`
- `docs/recovery/index.md` (extend with tools)
- `tools/postflight-recovery.sh` (delegates to canonical proofs)
- `scripts/qemu-test.sh` (recovery marker list; gated)

## Plan (small PRs)

1. **statefs check/repair API**
   - `Check()` and `Repair()` with bounded write budget.
   - All actions audited.

2. **updated recovery-safe helpers**
   - expose stage/schedule interfaces
   - refuse commit, emit `updated: commit blocked in recovery`

3. **shell built-ins**
   - `fsck state [--repair]`
   - `slot status`
   - `slot switch ...` (schedule only)
   - `ota stage ...` (stage only)

4. **`nx recovery`**
   - `enter`, `leave`, `exec`, `diag`, `stage`
   - host backend for tests; OS backend uses UART/DSoftBus once available

5. **Proof + docs**
   - host tests + OS selftests + postflight + docs update

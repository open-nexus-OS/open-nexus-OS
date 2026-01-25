---
title: TASK-0053 Security v3 (Recovery): signed Recovery Action tokens (.nxra) + replay protection + nx helpers
status: Draft
owner: @security
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Recovery v1a: tasks/TASK-0050-recovery-v1a-boot-target-minimal-shell-diag.md
  - Recovery v1b (tools): tasks/TASK-0051-recovery-v1b-safe-tools-fsck-slot-ota-nx-recovery.md
  - Policy as Code (trust + gating): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Keystore / device keys: tasks/TASK-0008B-device-identity-keys-v1-virtio-rng-rngd-keystored-keygen.md
  - Persistence (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - DevX CLI: tasks/TASK-0045-devx-nx-cli-v1.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

Recovery mode provides powerful operations (repair, slot switch, OTA staging).
To reduce operational footguns and to enable audited “break glass” procedures, mutating actions in recovery
should require a short-lived signed authorization token with replay protection.

Kernel remains unchanged; therefore all enforcement is in userspace (`recovery-sh` + service APIs).

This task is **host-first** (sign/verify tooling and format tests) and **OS-gated** (requires recovery shell and `/state`).

## Goal

Deliver:

1. A signed token format `.nxra` (CBOR) with Ed25519 signature.
2. Verification in `recovery-sh`:
   - required for mutating commands (fsck repair, slot switch, ota stage, leave recovery),
   - optional for read-only commands (diag/status) unless policy says otherwise.
3. Replay protection using nonce consumption stored in `/state/recovery/nonce.idx`.
4. Trust and authorization rules integrated with Policy-as-Code:
   - which pubkeys are trusted,
   - which actions are allowed per subject,
   - token lifetime bounds.
5. `nx recovery token make/show` helpers (host).

## Non-Goals

- Kernel changes.
- A general-purpose auth framework. This is narrow: recovery action authorization.

## Constraints / invariants

- Deny-by-default for mutating recovery actions unless a valid `.nxra` is presented.
- Deterministic verification and stable error reasons (for audits and tests).
- Bounded storage for nonce index (GC old nonces).
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED (key provisioning / trust root)**:
  - We must define where trusted pubkeys live and how they are provisioned.
  - If `/state` is not available yet, we can only do host tests; OS proof is blocked.
- **YELLOW (clock / time window)**:
  - Tokens use notBefore/notAfter; OS needs a monotonic time source.
  - If wallclock is not reliable, use a bounded “boot-time ns” epoch and document limitations.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/nxra_host/`:

- sign/verify happy path
- tamper detection
- expiry / not-before enforcement
- replay detection (nonce store simulated)

### Proof (OS/QEMU) — gated

UART markers:

- `recovery: nxra required`
- `recovery: nxra accept (pubkey=... actions=...)`
- `recovery: nxra reject (reason=...)`
- `SELFTEST: nxra require ok`
- `SELFTEST: nxra accept ok`

## Touched paths (allowlist)

- `source/apps/recovery-sh/` (gate mutating commands on nxra)
- `userspace/security/nxra/` (new crate: parse/sign/verify)
- `tools/nx/` (`nx recovery token make/show`)
- `policies/` + `schemas/policy/` (trust + allowed actions)
- `tests/nxra_host/`
- `docs/recovery/nxra.md`

## Plan (small PRs)

1. **Format + crate**
   - CBOR structure with version, subject, actions, time window, nonce.
   - Ed25519 signature envelope.

2. **Policy integration**
   - trusted pubkeys list + action allowlist.
   - map “subject” to allowed actions.

3. **Recovery shell enforcement**
   - require token for mutating commands.
   - store consumed nonces in `/state/recovery/nonce.idx`.

4. **DevX**
   - `nx recovery token make/show`.

5. **Tests + docs**
   - host tests, OS selftest once recovery exists, docs.

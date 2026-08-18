---
title: TASK-0051B Reliability v1f: crash evidence at rest — on-device .nxcd writer + retention/GC + policy redaction
status: Draft
owner: @reliability
created: 2026-08-18
depends-on:
  - TASK-0049 # reanimated producer chain + reason field
  - TASK-0049C # surviving evidence records (correlation source)
  - TASK-0048 # host pipeline: .nxcd container, nxsym, nx crash (Done)
follow-up-tasks:
  - TASK-0141 # export/redaction surface + notifications (rebased on this)
  - TASK-0142 # Problem Reporter UI (rebased on this)
links:
  - Reliability contract: docs/rfcs/RFC-0087-reliability-failure-model-v1.md (§5)
  - Crashdumps v1: docs/rfcs/RFC-0031-crashdumps-v1-minidump-host-symbolize.md
  - Host pipeline: docs/reliability/crashdump-v2.md (TASK-0048)
  - Container/GC SSOT: userspace/crash/nxcd/ (container.rs, gc.rs, zst.rs)
  - Storage split: docs/adr/0043-user-data-in-dedicated-cow-fs-statefs-stays-service-kv.md
  - Authority registry (crash artifact = .nxcd.zst): tasks/TRACK-AUTHORITY-NAMING.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

RFC-0087 Phase 2b — the former OS half of the old TASK-0049 ledger, now on a
foundation that exists: the producer chain is proven again (0049), evidence
records survive reboot (0049C), and the container/GC/symbolization toolchain is
shipped host-side (0048: `userspace/crash/nxcd`, `tools/nxsym`, `nx crash`).

What is missing is exactly one seam: crash artifacts **at rest on the device**
in the canonical format. Today execd writes NMD1 minidumps (`.nmd`) to
`/state/crash/…` with no GC, no size governance, and a second format
(`.nxcd.zst`) that only exists off-device. The authority registry names
`.nxcd.zst` as THE crash artifact ("no parallel dump formats without
decision").

## Goal

1. **On-device `.nxcd` writer**: after execd's minidump write, the crash path
   converts/wraps NMD1 into the `.nxcd.zst` container (0048's minidump-v1
   conversion is the SSOT — reuse, don't fork) and attaches bounded context:
   the `reason` field (0049), recent evidence records from logd's persisted
   scope (0049C, bounded count/bytes).
2. **Retention/GC on device**: `nxcd::plan_purge` (`gc.rs`) driven by TTL +
   `max_bytes` budget; marker `crash: retention gc on (budget=<n>MiB)` once
   per boot; GC actions audited as evidence records.
3. **NMD1 seam closed**: `.nmd` becomes an internal intermediate (deleted after
   successful container write; kept only on conversion failure with a
   deterministic degrade marker). `build_id` derivation stays
   `crash::deterministic_build_id` — shared SSOT with `nxsym`, must not fork.
4. **Policy redaction + export gate**: attachment level (none / stack-only /
   full) and export enablement via `policyd`, conservative defaults, export
   disabled by default; `/state/secret/*` never enters an artifact regardless
   of policy.
5. **Owner decision — no new daemon by default**: the writer lives in the
   existing crash path (execd-side library call), NOT a `crashd` service,
   unless execution shows the conversion cost needs isolation; if a service
   becomes necessary, it enters `TRACK-AUTHORITY-NAMING.md` FIRST. The old
   0049 `crashd` plan is superseded by this decision.

## Non-Goals

- On-device symbolization (host-only via nxsym, RFC-0031 stance).
- Crash notifications / export UI (TASK-0141/0142 — rebased followers).
- Cross-node correlation (TASK-0038 lane).
- Kernel-assisted capture (RFC-0031 limitation stands).

## Constraints / invariants (hard requirements)

- **Placement decision is part of DoD** (RED below) — bulk artifacts must not
  violate ADR-0043's "/state is a service-KV" rule silently.
- Bounded everything: conversion input caps (existing NMD1 caps), context
  attachment caps, GC budget; conversion failure degrades loudly
  (`crash: container write degraded (reason=…)`), never silently drops.
- One format authority: `.nxcd.zst` per registry; the container schema changes
  only in `userspace/crash/nxcd` with host tests.
- Crash path stays best-effort and non-blocking for the dying process
  (capture-side unchanged; conversion is post-mortem work).
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED (artifact placement)**: `.nxcd.zst` files are potentially MB-scale.
  Options: (a) hard size cap per artifact + small count budget under
  `/state/crash/` (KV-tolerable, bring-up honest), or (b) `/data` placement
  once nxfsd write path exists (TASK-0317, storage ladder). Decide BEFORE
  building, record here — otherwise the storage end-state ladder rewrites
  this task. Default recommendation: (a) with explicit caps now, migration
  note to (b) in the ledger.
- **YELLOW (VMO attachments)**: deferred; filebuffer path only in v1 (the old
  ledger's own fallback stance). VMO transfer joins when a consumer proves
  the need.

## Contract sources (single source of truth)

- Container/GC: `userspace/crash/nxcd/`. Build-id: `crash::deterministic_build_id`.
- Redaction/export policy shape: `policies/` + RFC-0087 §5.
- Marker contract: `scripts/qemu-test.sh` + proof-manifest.

## Stop conditions (Definition of Done)

### Proof (Host) — required

- `cargo test -p nxcd` extended: conversion path with reason+context sections,
  size-cap rejects, GC plan under budget fixtures.
- execd-side writer tests (mock store): NMD1→container→delete-intermediate,
  failure-degrade path, `test_reject_*` (oversized context, secret-path
  exclusion, export-disabled).

### Proof (OS/QEMU) — required

- Induced crash (0049 fault child) ⇒
  `crash: dump written (id=<id> bytes=<n>)` with `.nxcd.zst` at the decided
  location; `SELFTEST: crash artifact ok`.
- `crash: retention gc on (budget=<n>MiB)` + forced-GC knob ⇒
  `SELFTEST: crash gc ok`.
- Double boot: artifact survives; `nx crash ls/show` against the exported
  image finds and symbolizes it (host-side check, documented command);
  `SELFTEST: crash report ok` chain from 0049 still green.
- Redaction: policy fixture "stack-only" ⇒ artifact contains no full-memory
  section (`SELFTEST: crash redaction ok`).

## Touched paths (allowlist)

- `userspace/crash/nxcd/` (conversion/context sections + tests)
- `source/services/execd/src/` (writer call site + markers)
- `source/services/logd/` (persisted-scope read edge only)
- `policies/` (redaction/export rules)
- `tools/nx/` (crash verbs against device-exported images if needed)
- `source/apps/selftest-client/` + proof-manifest + `scripts/qemu-test.sh`
- `docs/reliability/crashdump-v2.md` (OS sections become real)

## Plan (small PRs)

1. Placement decision (RED) + caps; record in this ledger.
2. Conversion + context sections host-first (nxcd tests).
3. execd writer + degrade path + markers.
4. GC + redaction + policy fixtures.
5. QEMU proofs incl. double boot + host symbolization check; `just test-all`.

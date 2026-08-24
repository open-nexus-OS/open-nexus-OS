---
title: TASK-0051B Reliability v1f: crash evidence at rest — on-device .nxcd writer + retention/GC + policy redaction
status: Done
owner: @reliability
created: 2026-08-18
updated: 2026-08-24
completed: 2026-08-24
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
  - **DECIDED 2026-08-24 — (a), and the MB-scale premise does not apply
    here**: this device's capture is NMD1 with `MAX_TOTAL_FRAME = 8192`
    (stack preview ≤ 4 KiB, code preview ≤ 256 B), so a converted container
    is ~≤ 12 KiB — comfortably under the statefs `MAX_VALUE_SIZE` (64 KiB)
    KV cap and ADR-0043-shaped (small records). Caps: per-artifact hard cap
    = 32 KiB (reject-with-degrade above), GC budget max_count = 8,
    max_total_bytes = 256 KiB. Migration note: when TASK-0317 gives nxfsd a
    write path, bulk-scale artifacts (full-memory captures) move to
    `/data/crash/` — the KV lane stays the boot-critical floor.
- **YELLOW (VMO attachments)**: deferred; filebuffer path only in v1 (the old
  ledger's own fallback stance). VMO transfer joins when a consumer proves
  the need.

## Execution recuts (documented DoD deltas, Option C — 2026-08-24)

- **On-device artifact = plain `.nxcd`, not `.nxcd.zst`.** The zstd wrapper
  is host-tool-only BY DESIGN (nxcd `Cargo.toml`: "never enable this feature
  from an OS-graph crate — RFC-0009 dependency hygiene"). `nx crash` already
  reads both; the canonical exported artifact stays `.nxcd.zst` (produced by
  `nx crash export`). One container schema, one compression seam, no
  RFC-0009 breach. Authority registry note updated alongside.
- **Evidence records are NOT copied into the artifact.** They already
  survive at rest (0049C ring) and `nx diagnose` (0051) correlates them
  host-side by timestamp/pid. Duplicating them into every container would
  spend the crash budget on bytes that exist one prefix over. The container
  carries the `reason` field (0049) in `header.json` instead; the Logs
  section stays available for future producers.
- **Export gate rides TASK-0141.** There is no device-side export path today
  (artifacts leave the device via image extraction, host-side). The policyd
  attachment-level gate (none/stack-only/full) lands HERE; the export
  enable/disable switch is enforced where an export surface first exists.
- **nxcd goes no_std** (std stays the default feature for host tools):
  serde/serde_json on `alloc` — required so the execd (os-lite) writer can
  link the ONE conversion SSOT instead of forking it.
- **No forced-GC knob; `SELFTEST: crash gc ok` recut.** A GC-force switch
  would be a pure test surface inside the crash path (the exact knob class
  TASK-0049 PR-3 already rejected), and a real in-boot overflow needs >8
  execd-child crashes per boot (the standing injector crashes INIT-children,
  which produce no execd dumps — 2 artifacts/boot is the honest rate). The
  GC truth splits: plan semantics live in the host matrix
  (`gc_plan_keeps_newest_within_budget` + nxcd::plan_purge tests), the OS
  proves activation every boot (`crash: retention gc on (budget=256KiB)`
  required) and `crash: retention gc deleted (n=…)` stays a declared marker
  that fires on real overflow (keep-blk accumulation across boots).
- **Finding (fixed here): execd could NEVER write `/state/crash/` itself.**
  statefsd's cap gate mapped the prefix to the generic `statefs.write`,
  which execd never held — every non-managed crash dump
  (`write_minidump_artifact`, e.g. the demo.fault injector) silently
  returned `None` since TASK-0049; only the demo.minidump CHILD's put went
  through (SID-0 → selftest mapping). The 0051B degrade marker made it
  visible on the first run. Fix is the end-state shape: a dedicated
  `statefs.crash` prefix capability (keystore/boot pattern) granted to
  execd + selftest — never the generic writer grant.

## Contract sources (single source of truth)

- Container/GC: `userspace/crash/nxcd/`. Build-id: `crash::deterministic_build_id`.
- Redaction/export policy shape: `policies/` + RFC-0087 §5.
- Marker contract: `scripts/qemu-test.sh` + proof-manifest.

## Stop conditions (Definition of Done)

### Proof (Host) — required

- `cargo test -p nxcd` extended: conversion path with reason+context sections,
  size-cap rejects, GC plan under budget fixtures.
  ✅ 2026-08-24 (18 tests: reason roundtrip + absent-compat, preview
  sections at cap + oversize rejects, plan_purge matrix; no_std check +
  zst feature both green).
- execd-side writer tests (mock store): NMD1→container→delete-intermediate,
  failure-degrade path, `test_reject_*` (oversized context, secret-path
  exclusion, export-disabled).
  ✅ 2026-08-24 `tests/crash_store_contract.rs` (7 tests: conversion with
  reason+previews, redaction-none, garbage-NMD reject, deny-by-default
  cascade, fail-closed key derivation incl. secret-path, GC budgets).
  Export-disabled recut: no export surface exists yet (TASK-0141).

### Proof (OS/QEMU) — required

- Induced crash (0049 fault child) ⇒
  `crash: dump written (id=<id> bytes=<n>)` with the `.nxcd` at the decided
  location; `SELFTEST: crash artifact ok`. ✅ headless 2026-08-24 — BOTH
  producer paths (non-managed direct write, managed child-dump convert +
  intermediate delete); both markers REQUIRED in the ladder.
- `crash: retention gc on (budget=<n>MiB)` + forced-GC knob ⇒
  `SELFTEST: crash gc ok`. ✅ recut (see above): activation marker required
  every boot; plan semantics host-pinned; deleted-marker fires on real
  overflow, no knob.
- Double boot: artifact survives; `nx crash ls/show` against the exported
  image finds and symbolizes it (host-side check, documented command);
  `SELFTEST: crash report ok` chain from 0049 still green. ✅ keep-blk pair
  2026-08-24: boot-1 artifact present in boot-2's store; `nx diagnose`
  bundles `crash/<id>.nxcd` verbatim → `tar -xf` → `nx crash ls/show`
  decodes it (`reason: "fault"` in the header end to end); command
  documented in docs/reliability/crashdump-v2.md.
- Redaction: policy fixture "stack-only" ⇒ artifact contains no full-memory
  section (`SELFTEST: crash redaction ok`). ✅ probe parses the at-rest
  section table: only header/frames/maps + gated previews may exist,
  stack present exactly per source frame; `crash.attach.full` stays
  denied. Marker required; FAIL is a fatal signature.

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

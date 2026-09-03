---
title: TASK-0035 Delta updates v1b (system sets): nxs delta container + updated orchestration
status: Draft
owner: @runtime
created: 2025-12-22
updated: 2026-09-03
depends-on:
  - TASK-0034
  - TASK-0321
follow-up-tasks: []
links:
  - Vision: docs/architecture/vision.md
  - Depends-on (bundle deltas): tasks/TASK-0034-delta-updates-v1-bundle-nxdelta.md
  - Depends-on (updates service): tasks/TASK-0007-updates-packaging-v1_1-userspace-ab-skeleton.md
  - Depends-on (supply-chain policy): tasks/TASK-0029-supply-chain-v1-sbom-repro-sign-policy.md
  - Signing policy: docs/security/signing-and-policy.md
---

## End-state rewrite 2026-09-03 (binding; executes on TASK-0321 P3)

### Goal (end state)

updated-side orchestration of multi-component bundle sets on the TASK-0321 seam: the normative
apply order, a stage journal (`NXSJ`, inactive system-slot sector 1) that makes restage resume
per bundle, a host reuse index that ships only changed bundles, and `bundle-delta` (kind 4)
reconstructed from the active volume through the unchanged RFC-0090 delta adapter.

### Non-goals

New container or format families; changes to the volume format, verifier, gates or the loader;
network resume (falls out of path staging); anything the 2026-08-14 note already struck
(`.nxs` packing/parsing, persistent `updated`, boot-chain proof).

### Invariants

Resume never skips verification — the engine streams and hashes every component every time; the
journal only avoids rewrites. A journal is bound to the manifest digest and zeroed at commit.
Base binding for kind 4 is by bundle digest present in the ACTIVE index, rejected `delta-base`
before any write. The assembled volume stays byte-identical to the host build regardless of
which components were shipped, reused or delta-reconstructed.

### Packages

- **P1 — Apply order + per-component resume**: `NXSJ` stage journal `{magic, manifest_sha256,
  completed bitmap[32], crc32}` at inactive system sector 1, written by `VolumeSink` after each
  bundle's readback, zeroed at set begin on manifest mismatch and after the NXSV commit. On restage
  with a matching manifest, completed bundles are readback-verified, not rewritten. Marker
  `updated: restage resume (bundles=k/N)`. Host: journal codec `test_reject_journal_crc`,
  `test_reject_journal_manifest`, matrix „cut at every bundle boundary → resume converges“. QEMU
  keep-blk lane `ota-bundle-resume` (kill at `updated: component bundle verified`, boot 2 shows
  `updated: restage clean` / `restage resume` + `SELFTEST: ota stage resume ok`).
- **P2 — Reuse index (host)**: `nx image ota --bundle-set --reuse-from <active.img>` diffs bundle
  digests, ships only changed bundles, prints a JSON reuse manifest; test
  `bundle_set_ships_only_changed_bundles`; the device already reuses (0321 P3) — the lane asserts
  the `updated: bundle reused` count.
- **P3 — `bundle-delta` kind 4**: `nx image ota --delta-from-volume` emits one `.nxdelta`
  (`nxdelta::make`, RFC-0090) per changed bundle with base = the old bundle window; engine
  `check_bundle_delta_binding` (kind_data = base sha256, present in the active index, else
  `delta-base`); `DeltaAdapter<VolumeBase, BundleWriter>` where `VolumeSink` exposes a
  `BundleWriter: ComponentSink` for one bundle window so the adapter is reused verbatim. Host
  `component_set_bundle_delta.rs` (accept + `test_reject_delta_base_bundle`); QEMU lane
  `ota-bundle-delta` (`SELFTEST: ota bundle delta ok`).
- **P4 — Close**: RFC-0089 Phase 10 row ✅, `docs/updates/delta.md`, CHANGELOG, board rows.

### Stop conditions (Definition of Done — replaces the seed DoD)

Host: journal codec rejects + cut-at-every-boundary matrix; `bundle_set_ships_only_changed_bundles`;
`component_set_bundle_delta` accept + `test_reject_delta_base_bundle`. OS: keep-blk lane
`ota-bundle-resume` (`updated: restage resume`, `SELFTEST: ota stage resume ok`) and lane
`ota-bundle-delta` (`SELFTEST: ota bundle delta ok`), both in `test-all`; RFC-0089 Phase 10 ✅;
docs + CHANGELOG + board updated.


## Parked 2026-09-03 — executes after TASK-0321 (Phase B seam)

The Phase-B seam this task's residual scope sits on now has a ledger:
`tasks/TASK-0321-ota-phase-b-verified-system-volume-bundle-set.md` (system
volume + `bundle` components + reuse index). This task starts when 0321 P3
(`bundle` apply) is green; until then nothing here is buildable without
faking the bundle population.

## Rebase note 2026-08-25 (RFC-0089: no aggregate delta container — component kinds instead)

RFC-0089 §3/§11 dissolves this ledger's core artifact: there is no separate
"system-set delta container". The `.nxs` v2 COMPONENT MANIFEST already carries
typed components, so a delta update is simply a container whose components are
`boot-image-delta` (TASK-0034, v1 lane) and — after the Phase-B bundle-set seam
(RFC-0089 §12) — `bundle-delta` entries with unchanged-bundle reuse from the
active system volume. The residual scope of THIS task collapses to the
`updated`-side multi-component delta orchestration for Phase B (apply order,
per-component resume, reuse index); it executes after TASK-0034's
`boot-image-delta` kind and the Phase-B seam exist. depends-on unchanged
(TASK-0034); the 2026-08-14 note below remains accurate about what shipped but
its "aggregate delta container" framing is superseded.

## Rebase 2026-08-14 — unblocked (verified repo reality)

All three original blockers are gone; status moves `Blocked` → `Draft` with
`depends-on: [TASK-0034]` only (the `.nxdelta` bundle-delta format this task
aggregates).

- **`.nxs` tooling exists and is live end-to-end**: `tools/nxs-pack` ships;
  `source/apps/selftest-client/build.rs:53` generates `system-test.nxs` for
  every OS build, parsed by `userspace/updates/src/system_set.rs` (441 LOC).
  Do NOT re-implement `.nxs` packing/parsing.
- **`updated` exists persistently** (no longer a non-persistent skeleton):
  `source/services/updated/src/os_lite.rs` (971 LOC), bootctl state persisted
  at `/state/boot/bootctl.v1` (`os_lite.rs:53`), marker
  `updated: ready (statefs)` gated at `scripts/qemu-test.sh:482`.
- **The boot-chain blocker does not apply to this task's DoD**: that item was
  TASK-0037, which is being superseded by TASK-0289. This task's own DoD only
  requires staging to the inactive slot — which `handle_stage`
  (`os_lite.rs:396`) already does, QEMU-gated via
  `SELFTEST: ota stage ok` (`scripts/qemu-test.sh:536-540`).

Honest residual scope: the aggregate system-set delta container (list of
per-bundle `.nxdelta` patches + integrity index) and the `updated`-side
orchestration on top of the already-shipped stage/switch machinery. This
cannot start before TASK-0034's residual `.nxdelta` lane lands.

## Context

We eventually want system-set (`.nxs`) delta updates that apply a set of bundle deltas and stage an A/B update.

## Goal

Once unblocked, deliver:

- an aggregate delta container for system sets (list of per-bundle patches + integrity index),
- updated-side orchestration:
  - apply per-bundle deltas via bundlemgrd,
  - verify supply-chain policy for all bundles,
  - stage atomically to the target slot,
  - persist checkpoints for resume.

## Stop conditions (Definition of Done)

- Host tests: system delta container make/apply matches expected system set digest.
- OS/QEMU: markers for system delta start/verify/staged and selftest proofs.

## Red flags / decision points

- **RED**: cannot start until TASK-0034's residual `.nxdelta` lane lands
  (the format this task aggregates). The former blockers — `.nxs` tooling,
  persistent `updated`, boot-chain proof — are resolved (see Rebase
  2026-08-14).

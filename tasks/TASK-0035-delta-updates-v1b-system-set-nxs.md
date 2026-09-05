---
title: TASK-0035 Delta updates v1b (system sets): nxs delta container + updated orchestration
status: Done (P1–P4 delivered 2026-09-05)
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

### P1 delivered 2026-09-04 — NXSJ stage journal + per-bundle resume

- `userspace/updates/src/stage_journal.rs`: `NXSJ0001` at the inactive system slot's sector 1
  (RFC-0089 §12.2 reserved): `{volume_sha256, index_sha256, bundles:u16, completed bitmap[32],
  crc32}` — bound to the TARGET volume's identity (the NXSV digests the assembler sees; the set's
  manifest digest is not visible below the engine, and the NXSV binds tighter). Codec tests:
  roundtrip/bits, `test_reject_journal_crc`, `test_reject_journal_magic_and_bounds`.
- `volume_apply.rs`: after the index is verified on disk the journal is read; a journal that does
  not bind (other target / corrupt) is zeroed; every journalled window is READBACK-VERIFIED against
  the new index before it counts (stale bit → cleared) → `restage_resume(k, n)`. A resumed shipped
  bundle is still streamed and hashed by the engine (verification is never skipped) but not
  rewritten (`Current::skip`); after every verified or reused window the journal is persisted
  (write + sync) BEFORE the marker, so a cut right after `bundle reused` resumes past it. The NXSV
  commit zeroes the journal. Host: `restage_resumes_journalled_windows_and_zeroes_the_journal_at_
  commit` (cut at every op → resumed ≤ verified+1, ≥ 1 before the commit point, byte-identical,
  journal zeroed) and `test_reject_journal_manifest_crc_and_tampered_window`.
- OS: `updated: restage resume (bundles=<k>/<n>)` (VolumeMarkers); lane `ota-bundle-resume`
  (`[profile.ota-bundle-resume]`: `NEXUS_RESET_ON_MARKER="updated: bundle reused (name="`,
  count 1 → QMP system_reset mid-stage; boot 2 restages via `RuntimeProfile::OtaBundleResume`,
  verdict `SELFTEST: ota stage resume ok`), `just ci-os-ota-bundle-resume` in `test-all`.
- Recut vs. plan: journal bound to the NXSV digests instead of the manifest digest (see above);
  the power-cut actor is the existing QMP reset watcher (one uart, no keep-blk plumbing needed).
- FINDING (fixed): the selftest's fw_cfg profile buffer was 16 bytes — `ota-bundle-resume` (17)
  was silently truncated into the FULL ladder (the first lane run executed every OTA fixture
  instead of the resume proof). 32 bytes now; any future profile name must fit or fail loudly.
- FINDING (fixed): the bringup nxra break-glass probe is state-neutral ONLY while nothing is staged
  — with the resume proof's freshly staged slot b, its token switch became a REAL switch
  (`SELFTEST: nxra accept FAIL`). The resume proof now runs after the nxra chain.
- Proof (2026-09-04): `ota-bundle-resume` green — boot 1 cut at the first `bundle reused`, boot 2
  `updated: restage resume (bundles=2/22)` → `stage done (slot=b …)` → `SELFTEST: ota stage resume
  ok`, nxra chain still green; 9 host tests; `just check` green; `just test-all` GREEN end to end
  (headless, smp1, reset, ota-flip, ota-bundle, ota-bundle-resume, ota-tamper/downgrade/fallback).

### P2 delivered 2026-09-04 — host reuse index

- `nx image ota --bundle-set <dir> --reuse-from <active.img | dir>`: `image_volume::active_index_from`
  reads the device's `system-a` index straight from a built disk image (NXSV names the index; the
  index digest is checked; no signature needed — a reuse locator carries no trust, the device
  re-hashes every reused window against the NEW index) or rebuilds it from the factory bundle
  directory; `bundle_set_components_reusing` (from 0321 P4a) ships only changed windows. JSON
  reuse manifest `bundle_set.shipped` / `bundle_set.reused`. A base without a volume is a loud
  reject.
- Host test `bundle_set_ships_only_changed_bundles`: v1 disk built with `--system-bundles`, NEXT
  set bumps metricsd → the container carries boot.img + system.idx + `metricsd@1.0.1` only, both
  against the disk image and against the directory; blank image rejected. The device side was
  already proven by the `ota-bundle` lane (21 reused of 22).
- Proof (2026-09-04): host tests green (image_reuse_cli.rs; the volume test file split into
  `tests/common/volume_fixture.rs` for the 600-LOC ratchet); `just check` + `just test-all` GREEN.

### P3 delivered 2026-09-05 — `bundle-delta` (kind 4)

- **Engine**: `check_bundle_delta_binding` (kind_data = the BASE window sha256, 32 bytes); the
  RFC-0090 `DeltaAdapter` gained a bundle mode (`for_bundle`): the TARGET identity comes from the
  stream header, the inner sink's `begin` is deferred to that header — the assembler then binds the
  target to the signature-bound NEW index (`bundle-not-in-index` for an unknown target). New
  `updates::bundle_delta`: `VolumeBase` (byte-addressed reads of one ACTIVE window) and `SetSink`
  (kinds 6/2/4 dispatcher: a kind-4 component wraps the assembler in the adapter over a fresh
  active-volume handle and takes it back at `finish`; the base window is located via
  `VolumeAssembler::active_window` BEFORE any write → `delta-base`). The reconstructed bytes take
  the assembler's normal window path (readback-verified, journalled), so the volume stays
  byte-identical. Event `bundle_reconstructed` → `updated: component bundle-delta reconstructed
  (name=…)`.
- **Host**: `tests/updates_host/tests/component_set_bundle_delta.rs` (accept + byte-identity,
  `test_reject_delta_base_bundle` — nothing written past the index, no NXSV —,
  `test_reject_delta_target_not_in_index`); the engine scene moved to
  `tests/common/volume_scene.rs` (shared with the volume/journal tests).
- **nx**: `image ota --bundle-set <dir> --delta-from-volume <active.img | dir>` emits every changed
  bundle with a same-named predecessor as a kind-4 stream (`nxdelta::make` base window → new
  window), reuses unchanged ones, ships new bundles in full; JSON `bundle_set.delta`. `image
  fixtures --reuse-from` additionally emits `bundle-delta.nxs`. Host test
  `bundle_set_ships_deltas_for_changed_bundles`.
- **OS**: `updated`'s `StageSink` runs kinds 6/2/4 through `SetSink` (base handles = fresh
  partition clients on the ACTIVE system slot). Lane `ota-bundle-delta` (`RuntimeProfile::
  OtaBundleDelta`, `Lane::Delta`, `SELFTEST: ota bundle delta staged ok / ok`), `just
  ci-os-ota-bundle-delta` in `test-all`, `verify-nxupdate` gates the disk.
- FLAKE (historic, not ours): the first lane run had `SELFTEST: statefs auth put FAIL` on boot 2
  while every delta rung was green (`bundle-delta reconstructed` → `stage done` → flip → `ota
  bundle delta ok`) — the boot-2 statefs-envelope flake already logged for the flip lane; the rerun
  was fully green.
- Proof (2026-09-05): `ota-bundle-delta` green (`component bundle verified (name=metricsd@1.0.1)`
  → `component bundle-delta reconstructed (name=metricsd@1.0.1)` → 21 reused → flip →
  `init: spawn from volume svc=metricsd bundle=metricsd@1.0.1` → `SELFTEST: ota bundle delta ok`,
  `verify-nxupdate` ok); 12 host tests across the engine suites + 3 nx tests; `just check` green;
  `just test-all` GREEN end to end (nine lanes).

### P4 delivered 2026-09-05 — close

- RFC-0089: Phase 10 kind 4 ✅, §12.6 journal text aligned with the delivered NXSJ (NXSV-digest
  binding), checklist rows for Phase B orchestration ✅; `docs/updates/delta.md` „Bundle deltas“;
  CHANGELOG; board rows 7–10 + counters. DoD check against the stop conditions below: host —
  journal codec rejects + cut-at-every-op matrix ✅, `bundle_set_ships_only_changed_bundles` ✅,
  `component_set_bundle_delta` accept + `test_reject_delta_base_bundle` ✅; OS — `ota-bundle-resume`
  (`updated: restage resume`, `SELFTEST: ota stage resume ok`) ✅ and `ota-bundle-delta`
  (`SELFTEST: ota bundle delta ok`) ✅, both in `test-all` ✅; RFC-0089 Phase 10 ✅; docs + CHANGELOG +
  board ✅. **TASK-0035 Done.**

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

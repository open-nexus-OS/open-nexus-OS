---
title: TASK-0321 OTA Phase B — verified system volume (system-a/b) + service migration out of the boot image + bundle-set updates with unchanged-bundle reuse
status: Draft
owner: @runtime @security
created: 2026-09-03
updated: 2026-09-03
size: XL
depends-on:
  - TASK-0179
  - TASK-0289
  - TASK-0034
follow-up-tasks:
  - TASK-0035
links:
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
  - RFC (contract, §2 reserved partitions, §3 component kinds 2/4, §12 Phase B seam): docs/rfcs/RFC-0089-ota-v2-component-manifest-ab-boot-images-nxboot-bsb.md
  - RFC (delta stream, extended per bundle in this task): docs/rfcs/RFC-0090-nxdelta-boot-image-delta-stream-format.md
  - ADR (block topology, partition roles): docs/adr/0044-single-blk-device-gpt-partitions-block-layer.md
  - ADR (first-stage loader; unchanged by this task): docs/adr/0059-first-stage-boot-chain-nxboot-handoff.md
  - Layout authority: userspace/storage/src/layout.rs
  - Apply engine v2 (extended, not replaced): tasks/TASK-0179-updated-v2-offline-feed-delta-health-rollback.md
  - Boot trust floor (the chain this task extends downward): tasks/TASK-0289-boot-trust-floor-v1-verified-boot-rollback-measurement.md
  - Delta orchestration that executes AFTER this seam: tasks/TASK-0035-delta-updates-v1b-system-set-nxs.md
  - Per-app A/B (store distribution; NOT this task): tasks/TASK-0239-installer-v1_1b-os-pkgr-atomic-ab-bundlemgr-registry-licensed-selftests.md
  - Testing contract: scripts/qemu-test.sh
---

## Why this ledger exists (seeded 2026-09-03)

The Updates/OTA lane (RFC-0089, packages 0–11) shipped full-image A/B with a
verified first-stage loader, an apply engine with a crown proof, UI/CLI
surfaces and `boot-image-delta` components. RFC-0089 §12 contracts the
target UX — **bundle-set granularity** — as "executed by a follow-on task
family", but no ledger carried that family: TASK-0035 (delta orchestration
for bundle sets) was parked behind a Phase B that had no execution truth.
This ledger is that missing task. It is paper-only at seeding; the RFC §12
invariant is the scope firewall.

## Context

- Today every service is cross-compiled and embedded into ONE flat boot image
  (`scripts/build.sh` `INIT_LITE_SERVICE_LIST` → init-lite payload table);
  an update that changes one service ships the whole image (a 2 MB append
  is ~10 KB as a delta, but a real service change still re-ships the image).
- The GPT disk already reserves `system-a`/`system-b` (32 MiB each,
  `GUID_NEXUS_SYS`) in `userspace/storage/src/layout.rs`; nothing reads or
  writes them. `.nxs` v2 reserves component kinds 2 (`bundle`) and 4
  (`bundle-delta`); `updated` rejects them deterministically today
  (`updated: component kind unsupported`).
- What must NOT change (RFC-0089 §12 invariant): `.nxs` v2 container rules,
  the device trust anchor, the staging pipeline (§8 restage resume), NXBD,
  BSB, `nxboot`. Phase B changes component POPULATION and adds a
  system-volume verifier BELOW the loader-verified boot image.

## Goal

An update that changes one service ships only that service's bundle; the
device boots the new service from a verified system volume, and the loader
chain that verifies the boot image is unchanged. Concretely: two boots in
ONE uart — boot 1 stages an `.nxs` whose components are `[boot-image
(unchanged build), bundle(svc-x v2)]`, boot 2 runs svc-x v2 from
`system-<inactive→active>` with the volume verified by the boot image.

## Non-Goals

- Per-app A/B for store bundles (`.nxb` install via bundlemgrd) — TASK-0239.
- Changing `nxboot`, NXBD, BSB, the trust anchor, or the `.nxs` v2 rules.
- Network transport (no host↔guest transport exists; offline feed stays).
- Kernel changes. The kernel keeps loading init from the boot image; service
  loading below init is a userspace (execd/bundlemgrd) concern.
- `bundle-delta` orchestration (apply order, per-component resume, reuse
  index) — that is TASK-0035, which executes on top of this seam.
- Shrinking the boot image to "kernel + init + loader-of-services floor" in
  one step: the migration is per-service and gated (see Plan P3).

## Constraints / invariants (hard requirements)

- **Chain extends downward, never sideways**: the system volume's root
  descriptor (same 512-byte shape as NXBD, own magic) is a manifest
  component and is verified BY THE BOOT IMAGE (init-side verifier) after
  `nxboot` verified the boot image. `nxboot` never learns about system
  volumes.
- **Deny-by-default partition access**: `system-a/b` writes only by
  `updated` (inactive volume) and the factory image builder; reads only by
  the verifier + the service loader. `virtioblkd` partition gates on
  `sender_service_id` (TASK-0315 pattern), never a payload string.
- **No fake success**: `system: volume verified (slot=…)` / `SELFTEST: ota
  bundle-set ok` only after a real verify + a real spawn from the volume.
  Unsupported paths keep `unsupported`/`stub` markers.
- **Bounded input**: volume descriptor and bundle index are size-bounded
  before parsing; unknown bundle entries ⇒ deterministic reject.
- **Determinism**: image builder emits byte-identical system volumes for
  identical inputs (extends the `nx image` determinism tests).
- **Warnings gate, no `unwrap`/`expect` on untrusted input, no new cfgs.**

## Plan (small PRs; each test-all green)

- **P0 — contract**: RFC-0089 §12 amendment with the system-volume
  descriptor shape, the verifier position (init, pre-spawn) and the bundle
  index format; ADR for "services load from a verified system volume"
  (boundary: boot-image ↔ system volume; execd payload discipline).
- **P1 — host: system volume format + builder**: `pkgimg`-lineage RO image
  with a signed root descriptor; `nx image build` populates `system-a` from
  a bundle list; `nx image verify` checks the descriptor; determinism +
  tamper tests (`test_reject_*` for descriptor digest / unknown entry /
  oversize index).
- **P2 — OS: verifier + loader**: init verifies the active system volume's
  descriptor against the boot image's expectation (build-baked digest or
  manifest-listed component) BEFORE any service is spawned from it; a
  single pilot service (smallest, no ctrl-plane wiring hazard) is loaded
  from the volume instead of the embedded table. Markers: `system: volume
  verified (slot=a build=…)`, `init: spawn from volume svc=…`.
- **P3 — OS: `bundle` component apply**: `updated` accepts kind 2, writes
  the inactive system volume (bundle-level population, NXBD-style
  descriptor-LAST discipline), commit-time floor unchanged; QEMU: the
  two-boot bundle-set proof above, gated in headless/smp1.
- **P4 — migration + dedup**: move services out of the embedded table
  per-service behind the image-budget gate; unchanged-bundle reuse from the
  active volume at apply (the user-facing win); hands the reuse index to
  TASK-0035.

## Stop conditions (Definition of Done)

- Host: builder determinism + verifier reject suite green; `nx image
  verify` fails on a tampered system volume.
- OS/QEMU: `system: volume verified`, `init: spawn from volume`, and
  `SELFTEST: ota bundle-set ok` in ONE uart across two boots (crown-proof
  pattern from TASK-0179), gated in `test-all`.
- Docs sweep: RFC-0089 Status-at-a-Glance (Phase B ✅), CHANGELOG,
  `docs/architecture` updates section, board row 12, TASK-0035 unparked.

## Red flags / decision points

- **RED**: init-side verifier position. If verifying before ANY spawn costs
  boot time beyond the interactive reveal budget, verify lazily per-service
  (descriptor once, bundle digest at spawn) — decide with numbers in P2.
- **RED**: `execd` ctrl-plane slots are historically fixed (TASK-0315 find
  #3); loading from a volume must not touch spawn-time slot wiring. Treat
  as a payload SOURCE change only.
- **YELLOW**: 32 MiB per system volume is a budget, not a measurement.
  `scripts/check-image-budgets.sh` gets a `system-a` row before P3.

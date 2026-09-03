---
title: TASK-0321 OTA Phase B — verified system volume (system-a/b) + service migration out of the boot image + bundle-set updates with unchanged-bundle reuse
status: In Progress (P0 started 2026-09-03)
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

## End-state rewrite 2026-09-03 (binding; supersedes the seed text below where they differ)

**P0 DELIVERED 2026-09-03**: RFC-0089 §12 rewritten as the normative Phase B contract
(§12.1 format, §12.2 NXSV layout, §12.3 verifier + pairing + spawner, §12.4 kinds/ordering/
commit_set, §12.5 gates, §12.6 idempotency, §12.7 markers/rejects, §12.8 migration/budgets;
§3 kinds table rows 2/4/6; open question resolved; Status-at-a-Glance Phase 9/10/B; checklist)
+ ADR-0060 (Accepted) + ADR index + CHANGELOG.

**P1 DELIVERED 2026-09-03 (host formats + builder; honest recuts stated)**:
- `storage::pkgimg_bundles` — pkgimg **v3** (`PKGIMGV3`): per-entry sha256 + bundle table
  `{bundle, version, window off/len, stack_pages, gp, window sha256}`, deterministic 4 KiB-aligned
  windows, `parse_superblock` / `parse_index` (superblock + index ONLY — the boot-time read, index
  ≤ 256 KiB) + lazy `verify_bundle` / `verify_entry`; v2 parser untouched. 7 integration tests
  (`tests/pkgimg_v3.rs`) incl. `test_reject_pkgimg_v3_{entry,bundle}_digest_mismatch`,
  `_bundle_table_out_of_bounds`, `_launch_row_without_bundle`, `_parser_on_v2_image`.
- `bootfmt::nxsv` — NXSV codec (RFC-0089 §12.2 layout golden, sign/verify, reserved fail-closed,
  NXBD magic rejected): `test_reject_nxsv_{sig_and_wrong_key,reserved_and_magic}`.
- `nx image build --system-bundles <dir>` (bundle dirs per ADR-0020: `manifest.nxb` gives
  name/version via `bundlemgr::Manifest`, `meta/launch.json` `{stack_pages}` marks a spawnable
  service, `global_pointer` from the ELF with init-lite's `object` logic) → volume on `system-a`
  NXSV-LAST, side file `<out>.system-a.pkgimg` for the budget gate; `nx image verify` walks
  NXSV sig → pairing with the streamed boot-a digest → volume + index digests → every bundle +
  entry digest, reports `system_a: {absent}` on a factory-empty slot; `nx image ota --bundle-set
  <dir>` emits `[boot-image(-delta), system-volume(kind 6, payload = superblock+index, kindData =
  NXSV), bundle(kind 2, payload = window)…]`. `updates::component_set` gains the kind constants
  2/4/6 (engine still rejects them as `component kind unsupported` until P3 — never skipped).
  Tests `tests/image_volume_cli.rs`: `system_volume_build_is_deterministic_and_verifies`,
  `verify_rejects_tampered_and_unpaired_system_volume`, `bundle_set_container_decodes_kinds_2_and_6`.
- `scripts/system-volume-services.txt` (SSOT, pilot `metricsd`); `scripts/build.sh`
  `prepare_system_bundles` (nxb-pack `--toml` manifest, payload.elf, `meta/launch.json` from the
  service's stack pages) → `build/system-bundles/<svc>/`; launcher passes `--system-bundles`;
  `scripts/check-image-budgets.sh` row `system-a(vol)` (32 MiB − 4 KiB) from
  `build/nexus.system-a.pkgimg`.
- Recuts: the bundle-set QEMU fixture (`bundle-set.nxs` in `image fixtures`) lands with P3 (the
  fixture set self-verifies through the device engine, which accepts kinds 2/6 only from P3);
  `--reuse-from` is TASK-0035 P2; in P1 the listed services ship on the volume AND stay embedded —
  `NEXUS_VOLUME_SPAWN=1` (P2 default) removes them from `INIT_LITE_SERVICE_LIST`.
- Ratchet splits: `tools/nx/src/commands/image_ota.rs` (ota + tar), tests split into
  `image_volume_cli.rs`; test hashing streams the 384 MiB images (parallel whole-file reads were an
  OOM kill on a 16 GB host).
- `nx image patch --part boot-a --system-bundles <dir>` refreshes the PAIRED system slot with the
  boot slot (the keep-blk / flasher write shape); the launcher's keep-blk branch uses it. Test:
  patch without the flag ⇒ verify rejects `pair`, with the flag ⇒ green again.
- Proof (2026-09-03): `cargo test -p storage -p bootfmt -p nx` green (7 + 12 + 8/3/4);
  `just check` green (ratchet splits); `just test-os headless` builds the disk with `system-a`
  populated (`[build] system bundle metricsd -> build/system-bundles/metricsd (stack_pages=1)`)
  and the baseline ladder is unchanged (`verify-uart ok`; one earlier run on a loaded host —
  load 9.6 during builds — missed the timing-sensitive `SELFTEST: i18n switch ok`, a known
  pattern in 27 of 179 headless logs, green on re-run). Fresh image verified paired:
  `system_a: build fresh-1, paired, volume 82342 B, bundle metricsd@1.0.0 stack_pages=1
  gp=0x22368`. FINDING: after a headless run the disk is NOT paired any more — the OTA selftest
  lane stages fixtures, flips to b, commits, stages `fixt-b` back into boot-a; pairing is a
  fresh-build/flasher property until the bundle-set fixture (P3) ships a volume with every image.
  Budget row: `system-a(vol) 82342 / 33550336 (0%)`, init-lite unchanged (metricsd still
  embedded until P2).

Next: P2 (OS verifier + loader + pilot).

Planned against verified repo reality (Explore + Plan 2026-09-03). Principle: every package is a
direct step to the production system — no interim volume format, no second bundle registry, no
placeholder verifier.

### Decisions (D1–D8)

- **D1 Volume format** — pkgimg **v3** payload + a signed **NXSV** root descriptor at volume
  sector 0 (NXBD shape, own magic `NXSV0001`, Ed25519 sig @448; binds `volume_sha256`,
  `index_sha256`, `boot_image_sha256`, `build_id`, `rollback_index`). Resolves RFC-0089's open
  question (system volume format). No nxfs-RO, no new format: pkgimg already has the deterministic
  builder + bounded no_std parser (`userspace/storage/src/pkgimg.rs`).
- **D2 Digest levels** — NXSV binds volume + index; the index binds per-**bundle** sha256 and
  per-**file** sha256. Boot verification is O(descriptor + index); bundle digests are checked when
  a bundle is served; the per-bundle digest is TASK-0035's reuse unit.
- **D3 Verifier + reader = bundlemgrd; init stays the ONLY spawner.** bundlemgrd is CORE, lives in
  the loader-verified boot image ("verified by the boot image" holds literally), already owns the
  payload-VMO discipline (`bundlemgrd/src/os_lite.rs:346-378`) and is the one bundle authority.
  init cannot be a block client (kernel-loaded task has service id 0, `task/mod.rs:364`; gates key
  on kernel sid). init receives the bundle ELF as an RO VMO mapping and passes the slice to
  `exec_v2` (kernel `ensure_user_slice` accepts mapped memory — **no kernel change**).
- **D4 Spawn order** — a second spawn pass after the MMIO grants (`orchestrator.rs:847-856`) and
  before `wire_services` (:871). **Pilot = metricsd**: non-CORE, `SAFE_EXCLUDED`, endpoints already
  `Option`, ~0.5 MB; recovery/safe boots never depend on the volume.
- **D5 Engine shape** — `ComponentSink::commit_set(&mut self)` (default no-op) + the ordering
  contract `[boot-image | boot-image-delta]?, system-volume (kind 6, index first),
  (bundle | bundle-delta)*`. SlotSink and every TASK-0179 test stay untouched (§12 invariant:
  container rules, staging pipeline, NXBD, BSB, nxboot unchanged).
- **D6 Reuse + gates** — `VolumeBase` (active system slot, read-only) paired with `VolumeSink`
  (inactive), the RemoteBase/SlotSink pattern from RFC-0090. virtioblkd gate becomes op-aware
  `Gates::allowed(sender, part, op)`: system READ ← bundlemgrd, updated; WRITE/SYNC ← updated
  (inactive only, engine scopes the slot). Every reused bundle is hashed while copied against the
  NEW (signature-bound) index — updated needs no OS-key anchor.
- **D7 Budgets** — `scripts/check-image-budgets.sh` gets a `system-a` row (pkgimg bytes vs
  32 MiB − 4 KiB) and an `init-lite(embedded) / system-a(volume)` split BEFORE any migration, so
  bytes move visibly instead of vanishing.
- **D8 One bundle model, one activation authority** — bundlemgrd's registry gains
  `BundleSource::{SystemVolume(slot), State}`; the active volume slot = the measured boot slot
  (bootctld `OP_GET_MEASURED`); init's `bundlemgrd_set_active_slot` becomes an assertion (mismatch
  is loud). TASK-0239 per-app A/B lives entirely under `State`. `.nxb` stays the ONE artifact
  (ADR-0020) — no second registry, no second format.
- Launch parameters (`stack_pages`, `global_pointer`) are pkgimg-v3 bundle-table fields computed
  by `nx image` with the same `object` logic as `source/apps/init-lite/build.rs:137-170`; init
  never parses ELF at runtime. Respawn (ADR-0057) reuses the kept mapping — the VMO arena never
  frees, so init never re-requests. `updated: restage clean` (contracted in RFC-0089 §8, never
  implemented) becomes real in P3.

### Goal (end state)

The system volume is a first-class, loader-independent trust tier: pkgimg v3 payload + signed
NXSV on `system-a/b`, verified by bundlemgrd against the baked OS key, the persisted rollback
floor and the measured boot image; every boot service outside CORE is spawned by init from a
bundle served out of that verified volume; `.nxs` sets carry `[boot-image(-delta),
system-volume, bundle…]`; updated assembles the inactive volume deterministically (byte-identical
to `nx image build`), reusing unchanged bundles from the active volume, and lands the NXSV last at
set commit. Two boots in one uart prove that a one-bundle update runs the new bundle from the
flipped volume.

### Non-goals

Kernel changes; nxboot / NXBD / BSB / trust-anchor / `.nxs` container-rule changes; CORE
(wave-1) services on the volume; per-app A/B for `State`-sourced bundles (TASK-0239); network
transport; `bundle-delta`, stage journal / resume, host reuse index (TASK-0035).

### Invariants

Chain extends downward only (loader → boot image → bundlemgrd → volume → bundle). system-X pairs
with boot-X: `NXSV.boot_image_sha256 == measured image sha256` (direct-kernel dev boots report
`pair=unbound`, never `ok`). Deny-by-default op-aware partition gates on kernel sid. No fake
success: `system volume verified`, `spawn from volume`, `ota bundle-set ok` only after a real
verify + a real spawn. Bounded before parse (NXSV 512 B, index ≤ 256 KiB, bundle ≤ partition).
Determinism: volume, container and device-assembled volume are byte-identical for identical
inputs. One bundle authority (bundlemgrd). init remains the sole spawner and capability
distributor; fixed ctrl-plane slots never shift. `ComponentSink` grows only `commit_set`.

### Packages (each test-all green before the next)

- **P0 — Contract** (approval zone `docs/rfcs`): RFC-0089 §12 amendment — NXSV layout (NXBD field
  positions reused: rollback_index@12, volume_size@16, volume_sha256@24, build_id@56,
  pubkey_id@96; new: `boot_image_sha256`@104, `index_len` u32@136, `index_sha256`@140; sig@448),
  geometry (sector 0 NXSV, sectors 1–7 reserved — sector 1 = TASK-0035 stage journal, payload from
  sector 8 = `IMAGE_START_SECTOR`), kind 6 `system-volume` (payload = pkgimg superblock + index,
  ≤ 256 KiB, kind_data = NXSV), kind 2 payload = the bundle's data-region slice (component sha256
  == index bundle sha256), kind 4 kind_data = `base_sha256[32]`, ordering rule + `commit_set`,
  pairing rule, verifier = bundlemgrd, gate matrix, §3 table rows 2/4/6, Status-at-a-Glance rows
  (Phase 9 ✅, Phase 10 ✅ for 0034, Phase B 🚧). **ADR-0060** „verified system volume:
  bundlemgrd verifier, init spawner“ (boundary boot image ↔ system volume; CORE-never-on-volume;
  VMO payload discipline; OS-key baked into bundlemgrd via the shared `build_trust.rs` parser).
- **P1 — Host formats + builder**: `userspace/storage/src/pkgimg.rs` `VERSION_V3` (bundle table
  `{bundle, version, sha256, data_off, data_len, stack_pages u32, global_pointer u64}`, per-entry
  sha256; v2 still parsed until P5; `ParsedPkgImg::bundles()`, `bundle_window()`);
  `userspace/bootfmt` `pub mod nxsv` (clone of `nxbd`: encode/decode/verify/sign + goldens);
  `tools/nx` `image build --system-bundles <dir>` (`<dir>/<name>/` = `.nxb` directory per
  `docs/packaging/nxb.md`) → `build_system_volume()` written NXSV-last via a generalized
  `write_slot(dev, part, body, descriptor)` shared with `write_boot_slot`; `image verify` checks
  NXSV sig + index + every bundle digest + pairing with boot-a; `image ota --bundle-set <dir>
  [--reuse-from <img>]` emits `[boot-image(-delta), system-volume, bundle…]`; `image_fixtures`
  adds `bundle-set.nxs` (metricsd@1.0.1). `scripts/build.sh`: `prepare_service_payloads` emits
  `build/system-bundles/<svc>/{manifest.nxb,payload.elf,meta/}` for services listed in
  `scripts/system-volume-services.txt` and removes them from `INIT_LITE_SERVICE_LIST`;
  budgets row (D7). Tests: `test_reject_pkgimg_v3_entry_digest_mismatch`,
  `test_reject_pkgimg_v3_bundle_digest_mismatch`, `test_reject_pkgimg_v3_bundle_table_out_of_bounds`,
  `pkgimg_v3_determinism`, `test_reject_nxsv_sig`, `test_reject_nxsv_magic`,
  `test_reject_nxsv_reserved`, `system_volume_build_is_deterministic`,
  `verify_rejects_tampered_system_volume`, `verify_rejects_unpaired_volume`,
  `bundle_set_container_decodes_kinds_2_and_6`.
- **P2 — OS verifier + loader + pilot** (approval zone `source/libs` for the bundlemgrd wire
  ops): virtioblkd `Gates { …, sid_bundlemgrd }` + `allowed(sender, part, op)`;
  `bootstrap/blk_plane.rs` adds the bundlemgrd client slot; wire ops `OP_QUERY_BUNDLE {name} →
  {status, size, sha8, stack_pages, gp, version}`, `OP_GET_BUNDLE_ELF {name}` (caller-created VMO,
  CAP_MOVE, header written LAST after digest match), `OP_VOLUME_STATUS → {slot, verified, build8}`;
  `source/services/bundlemgrd/src/volume.rs` (≤ 600 LOC: attach measured slot via
  `RemoteBlockDevice` on `PART_SYSTEM_{A,B}`, NXSV verify against `BAKED_OS_KEYS` from
  `policies/os-trust.toml`, `rollback_index ≥ floor`, pairing, bounded index read, v3 parse,
  `serve_bundle_elf()` streams sectors → sha256 → VMO); `BundleSource` in the registry; init
  `os_payload.rs` `ServiceImage { name, source: ServiceSource::{Embedded{elf,stack_pages,gp},
  Volume{bundle}} }`, new `bootstrap/volume_spawn.rs` (query → `vmo_create` → `OP_GET_BUNDLE_ELF`
  → poll header → `vm_map` RO → `exec_v2` → extracted `attach_ctrl_channel()` +
  `distribute_server_pair_for()`), `src/mapmem.rs` (mirror of updated's), `respawn.rs` keyed on
  `ServiceSource`, `service_source.rs` `VOLUME_SERVICES` SSOT + host tests
  `core_services_never_on_volume`, `volume_list_matches_script`. Markers: `bundlemgrd: system
  volume verified (slot=a build=<8> bundles=N)`, `bundlemgrd: system volume FAIL (<sig|digest|
  floor|pair|bounds|io>)`, `bundlemgrd: bundle served (name=… sha=<8>)`, `init: spawn from volume
  svc=metricsd bundle=metricsd@1.0.0 sha=<8>`, `init: volume spawn FAIL svc=… reason=…`,
  `SELFTEST: blk system volume deny ok` (selftest READ of system-a → `STATUS_DENIED`).
  Proof: headless/smp1 gated; `metricsd: ready` unchanged; `ci-os-reset` green. Measure the
  boot-time cost of Ed25519 + index read before the pilot spawn.
- **P3 — OS apply + two-boot proof** (approval zones `scripts/qemu-test.sh`, `justfile`):
  `component_set.rs` `KIND_BUNDLE=2`, `KIND_SYSTEM_VOLUME=6`, `check_system_volume_binding`
  (NXSV decode, build_id/rollback_index match, `index_sha256 == meta.sha256`, `index_len ==
  meta.size`), `check_bundle_binding`, ordering → `RejectReason::Order` (label `order`),
  `commit_set()` after the loop; `source/services/updated/src/volume_os.rs` `VolumeSink`
  (attach inactive system part; `begin(kind 6)` zeroes sector 0, writes index at sector 8,
  parses it; `begin(kind 2)` locates the bundle window by sha256/size; shared `SectorWriter`
  with SlotSink; `finish` readback-hashes the window; `commit_set` copies every not-yet-populated
  index bundle from `VolumeBase` with digest check → `updated: bundle reused (name=… sha=<8>)`,
  readback `volume_sha256`, NXSV LAST, sync); `delta_os.rs` `State::Volume`; `stage_os.rs`
  markers `updated: component system-volume verified (build=<8> bundles=N)`, `updated: component
  bundle verified (name=…)`, `updated: restage clean` probe. Host
  `tests/updates_host/tests/component_set_volume.rs`: accept (index + 2 bundles + reuse),
  `test_reject_order`, `test_reject_bundle_not_in_index`, `test_reject_volume_digest`,
  `test_reject_volume_binding`, power-cut matrix (after index / mid-bundle / before NXSV → slot
  never NXSV-valid; restage converges byte-identical to `nx image build`). Lane `ota-bundle`
  (two boots, one uart, crown pattern): `updated: stage begin (source=/updates/bundle-set.nxs)`
  → `component boot-image-delta verified` → `component system-volume verified (build=otaS` →
  `component bundle verified (name=metricsd` → `bundle reused (` → `stage done (slot=b
  build=otaS` → `bootctld: switch scheduled (to=b)` → `SELFTEST: ota bundle-set staged ok` →
  reset → `nxboot: verify ok (slot=b build=otaS` → `bundlemgrd: system volume verified (slot=b
  build=otaS` → `init: spawn from volume svc=metricsd bundle=metricsd@1.0.1` → `bootctld: commit
  ok (slot=b)` → `SELFTEST: ota bundle-set ok`. `just ci-os-ota-bundle` in `test-all`; marker
  three-way SSOT (`scripts/qemu-test.sh`, `tools/nx/chains/markers.txt`, proof-manifest
  `markers/ota.toml`).
- **P4 — Migration + dedup accounting**: pinched (extends the ADR-0057 respawn pilot: keep
  mapping) → imed → timed → touchd/hidrawd → netstackd → dsoftbusd → abilitymgr → settingsd →
  sessiond → inputd → gpud → windowd; CORE (`boot_graph.rs:59-78`) + execd's embedded app-host
  stay embedded; each step behind the budget gate + green ladder; `SELFTEST: service restart ok`
  stays green with pinched on the volume; the `ota-bundle` lane asserts `updated: bundle reused`
  count ≥ N−1.
- **P5 — Boot-image floor**: `APP_PAYLOADS` and the `FETCH_IMAGE` pkgimg move into the volume as
  `SystemVolume` bundles; packagefsd mounts via `OP_GET_FILE_VMO` (per-file digest from the v3
  index) instead of RAM bytes (`packagefsd: mounted (system volume slot=a)`); pkgimg v2 transcode
  retired; docs `docs/architecture/09-nexus-init.md`, `15-bundlemgrd.md`,
  `docs/packaging/system-set.md`, `docs/updates/*` swept.

### Stop conditions (Definition of Done — replaces the seed DoD)

- Host: pkgimg v3 + nxsv + engine reject suites green; `nx image verify` fails on a tampered or
  unpaired volume; power-cut matrix converges; determinism cross-check (device-assembled volume ==
  host build).
- OS/QEMU: headless/smp1 gate `bundlemgrd: system volume verified`, `init: spawn from volume
  svc=metricsd`, `SELFTEST: blk system volume deny ok`; `just ci-os-ota-bundle` ends in
  `SELFTEST: ota bundle-set ok`; `ci-os-reset`, `ci-os-ota`, `ci-os-ota-backstops` untouched-green;
  budgets script has the `system-a` row and every migrated service moved its bytes visibly.
- Docs: RFC-0089 §12 amendment + ADR-0060 merged, Phase B ✅ row, board rows, TASK-0035 unparked.

### Risks (tracked)

Boot-time cost of Ed25519 + index read before the pilot spawn (measure in P2; index ≤ 256 KiB
over the 6 KiB/req block plane ≈ 45 requests). init cap-table pressure (one VMO handle per
volume service, bounded by `VOLUME_SERVICES`). VMO arena never frees (`apply_os.rs:54-57`) — init
must never re-request on respawn. `eps` becoming mutable for late metricsd mints must not shift
any fixed slot (guard with the slot-shift probes). Kind-6 index bound vs real bundle counts after
P4/P5 — raise via RFC amendment, never silently. pkgimg v2/v3 coexistence until P5. Direct-kernel
dev boots have no measured image — the pairing rung stays honest (`pair=unbound`).


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

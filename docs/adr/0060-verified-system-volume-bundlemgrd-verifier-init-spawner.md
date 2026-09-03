<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# ADR-0060: Verified system volume — `bundlemgrd` is the volume verifier and bundle authority, `init` stays the sole spawner

- Status: Accepted
- Date: 2026-09-03
- Links:
  - RFCs: `docs/rfcs/RFC-0089-ota-v2-component-manifest-ab-boot-images-nxboot-bsb.md`
    (§12 Phase B — normative), `docs/rfcs/RFC-0090-nxdelta-boot-image-delta-stream-format.md`
    (kind 4 reuses the adapter)
  - Tasks: `tasks/TASK-0321-ota-phase-b-verified-system-volume-bundle-set.md` (execution +
    proof), `tasks/TASK-0035-delta-updates-v1b-system-set-nxs.md` (orchestration on this seam)
  - Related ADRs: `docs/adr/0059-first-stage-boot-chain-nxboot-handoff.md` (the tier above:
    loader → boot image, unchanged), `docs/adr/0055-bootctld-single-boot-state-authority.md`
    (boot-state authority, unchanged), `docs/adr/0057-service-restart-capability-re-resolve.md` (respawn discipline — extended to
    volume-sourced services), `docs/adr/0044-single-blk-device-gpt-partitions-block-layer.md`
    (partition gates), `docs/adr/0020-manifest-format-capnproto.md` (`.nxb` is the ONE bundle
    artifact), `docs/adr/0036-ability-lifecycle-vs-process-vs-registry-service-split.md`
    (registry vs lifecycle split — bundlemgrd is the registry)

## Context

RFC-0089 built full-image A/B: the loader verifies ONE flat boot image that embeds every
service. The target UX is bundle-set granularity — a service update ships one bundle. That
needs a second trust tier below the boot image (a verified **system volume** on the reserved
`system-a/b` partitions) and services that start from it. Three things must be decided
once, because each is a boundary: who verifies and reads the volume, who spawns from it, and
which registry says what is installed. Verified facts that constrain the decision:

- The kernel-loaded init task has service id 0 (`source/kernel/neuron/src/task/mod.rs`);
  `virtioblkd` gates partitions on the kernel-attributed sender. init as a block client would
  need a kernel change (out of scope) or a sid-0 grant (deny-by-default violation).
- init owns the fixed control-plane slots, the route table and supervision/respawn
  (ADR-0057); spawning from anywhere else would fork that authority.
- `bundlemgrd` is a CORE wave-1 service embedded in the loader-verified boot image, already
  serves payloads to children over caller-created VMOs, and is the single "what is installed"
  registry (ADR-0036). TASK-0239's per-app A/B lives in `/state` and needs the same registry.
- `exec_v2` takes a caller-memory slice (`ensure_user_slice`): a read-only mapped VMO is a
  valid ELF source without any kernel change.
- All services are exec'd in one up-front pass before MMIO grants; the block plane is only
  live afterwards (`bootstrap/orchestrator.rs`).

## Decision

1. **Trust tier**: the system volume is verified BY THE BOOT IMAGE, never by the loader. The
   chain is loader → boot image → `bundlemgrd` → NXSV → index → bundle. `nxboot`, NXBD, BSB and
   the trust anchor are unchanged (RFC-0089 §12 invariant).
2. **Verifier + reader = `bundlemgrd`**. It attaches the system slot paired with the MEASURED
   boot slot (bootctld `OP_GET_MEASURED`), verifies the NXSV against the baked OS keys
   (`policies/os-trust.toml`, the same shared parser `nxboot` uses), the persisted rollback
   floor and the measured image digest, reads the bounded index, and serves bundle ELFs over
   caller-created VMOs with the header written LAST after the bundle digest matched.
   Direct-kernel dev boots (no measured image) yield `pair=unbound`: the volume is never
   trusted for spawning without a measured pairing, and no marker says `verified`.
3. **Spawner = `init`, unchanged authority**. A volume-sourced service is spawned in a second
   pass after the MMIO grants and before service wiring: init queries bundlemgrd, receives the
   ELF VMO, maps it read-only, calls `exec_v2` with the mapped slice and then runs the same
   control-channel attach and endpoint distribution as embedded services. Launch parameters
   (`stack_pages`, `global_pointer`) come from the pkgimg v3 bundle table; init never parses
   ELF. The mapping is kept for the process lifetime so respawn (ADR-0057) reuses it — the VMO
   arena never frees, so init never re-requests.
4. **CORE never on the volume**: the wave-1 graph (`boot_graph::CORE`) and execd's embedded
   app-host stay in the boot image; recovery and safe boots never depend on a volume.
   `scripts/system-volume-services.txt` is the SSOT of what lives on the volume, mirrored by a
   nexus-init host test. Pilot: `metricsd` (non-CORE, `SAFE_EXCLUDED`, optional endpoints).
5. **One bundle model, one activation authority**: `.nxb` (ADR-0020) is the only bundle
   artifact; bundlemgrd's registry gains `BundleSource::{SystemVolume(slot), State}`; the
   active volume slot equals the measured boot slot and init's `set_active_slot` handshake
   becomes an assertion. No second registry, no second format.
6. **Partition gates are op-aware**: `virtioblkd` `allowed(sender, partition, op)` — system
   READ for `bundlemgrd` and `updated`, WRITE/SYNC for `updated` only (engine scopes it to the
   inactive slot). Deny-by-default stays.
7. **Engine seam**: `ComponentSink::commit_set()` (default no-op) is the only trait growth;
   ordering `[boot-image(-delta)]?, system-volume, (bundle|bundle-delta)*`; the volume sink
   assembles the inactive volume byte-identically to the host build, reusing unchanged
   bundles from the active volume with per-bundle digest checks, NXSV last.

## Consequences

- Boot verification cost is O(NXSV + index) plus one Ed25519 verify in bundlemgrd before the
  pilot spawn; measured in TASK-0321 P2 and gated by the image-budget script's new
  `system-a` row.
- Each volume service costs init one VMO handle (bounded by the SSOT list). Late endpoint
  minting for volume services must not shift any fixed control-plane slot (slot-shift probes
  stay in the ladder).
- A broken or missing volume degrades exactly the non-CORE services it carries (`init: volume
  spawn FAIL svc=… reason=…`), never the boot spine.
- TASK-0239 (per-app A/B) and TASK-0166 (SDK catalog) build on `BundleSource::State` in the
  same registry; TASK-0035 builds the stage journal, reuse index and kind 4 on the seam.
- Docs that still describe the RFC-0012 flow (`docs/architecture/09-nexus-init.md`,
  `15-bundlemgrd.md`) are swept in TASK-0321 P5.

## Verification

- Host: `cargo test -p storage -p bootfmt -p nx` (pkgimg v3 + nxsv codecs, builder
  determinism, tamper/unpaired rejects), `cargo test -p updates_host` (kinds 2/6, ordering,
  power-cut matrix), nexus-init host tests (`core_services_never_on_volume`,
  `volume_list_matches_script`).
- OS (gated): `bundlemgrd: system volume verified (slot=… build=… bundles=…)`,
  `init: spawn from volume svc=metricsd …`, `SELFTEST: blk system volume deny ok` in
  headless/smp1; `just ci-os-ota-bundle` (two boots, one uart) ends in
  `SELFTEST: ota bundle-set ok`; `ci-os-reset` / `ci-os-ota` / `ci-os-ota-backstops` stay green.

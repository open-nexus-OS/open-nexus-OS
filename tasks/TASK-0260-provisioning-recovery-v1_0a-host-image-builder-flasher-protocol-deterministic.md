---
title: TASK-0260 Provisioning v1.0a (host-first): nx image — deterministic GPT disk assembler + NXBD signer + factory BSB + OTA-container emission (flasher/factory-reset = residual)
status: In Progress
note: image-builder scope (the OTA-lane package) DELIVERED 2026-08-25; the ledger stays open for the residual flasher protocol + factory reset (executes with TASK-0261)
owner: @reliability
created: 2025-12-29
updated: 2026-08-25
depends-on: []
follow-up-tasks:
  - TASK-0315
  - TASK-0289
links:
  - Contract: docs/rfcs/RFC-0089-ota-v2-component-manifest-ab-boot-images-nxboot-bsb.md (§2 layout, §5 NXBD, §6 BSB)
  - BSB factory role: docs/adr/0058-boot-selection-block-dual-actor-discipline.md
  - Block topology: docs/adr/0044-single-blk-device-gpt-partitions-block-layer.md + tasks/TASK-0315-block-topology-consolidation-gpt-virtioblkd-sole-owner.md
  - GPT/GUID authority (shared, host-tested): userspace/storage/src/gpt.rs
  - Signing primitives baseline: tasks/TASK-0029-supply-chain-v1-sbom-repro-sign-policy.md
  - Testing contract: scripts/qemu-test.sh
---

## Image-builder scope DELIVERED 2026-08-25 (test-all green; OTA-lane package 4)

Evidence (`cargo test -p nx --test image_cli` — 6 integration tests;
`just test-all` EXIT=0; flasher/factory-reset stay residual, see below):

- **`nx image build`**: full RFC-0089 §2 GPT disk (`build/nexus.img`,
  384 MiB sparse) via the SHARED authorities — layout from
  `storage::layout::NEXUS_DISK_LAYOUT` (new module, THE table for nx image
  / virtioblkd / nxboot), GPT bytes from `storage::gpt::write_gpt`, NXBD/
  BSB from the new `userspace/bootfmt` crate (no_std codecs + host `sign`
  feature; 6 unit tests: goldens, tamper, torn-block pick rule, zeroed-
  sector-invalid). Factory BSB block 0 (seq 1, active A, committed);
  boot-a written NXBD-LAST (zero descriptor → padded body → signed
  descriptor); boot-b zeroed = invalid; optional state/data seeding;
  per-slot budget enforced (57 MiB kernel ⇒ deterministic build reject).
  Determinism proven: build twice ⇒ byte-identical images.
- **`nx image verify`**: re-parse through the SAME `storage::gpt` parser
  the OS uses, BSB pick rule, NXBD signature (pubkey or seed-derived) +
  streamed payload sha256; tamper and wrong-key rejects proven.
- **`nx image patch --part boot-a|boot-b`**: refreshes ONE slot (NXBD-last)
  — bsb/state/data proven byte-identical before/after. This is the
  keep-blk dev flow after the boot flip AND the flasher's write shape.
- **`nx image ota`**: `.nxs` v2 container (RFC-0089 §3) — capnp
  `ComponentManifest` (schema SSOT extended in
  `tools/nexus-idl/schemas/system-set.capnp`: schemaVersion 2, typed
  components, kinds 2..5 reserved), deterministic tar (mode 644, uid/gid 0,
  mtime 0), publisher signature over `manifest.nxo`. Layout decision
  recorded: the signed NXBD rides in the boot-image component's `kindData`
  (bounded 512 B) — the publisher signature transitively binds it, and the
  descriptor itself carries the OS-image signature (two-layer trust,
  RFC-0089 §4/§5). Fixture keys: `keys/dev-os-image.ed25519.seed`
  (`0b`×32) + `keys/dev-publisher.ed25519.seed` (`07`×32 — the anchored
  fixture publisher); rollback-index/build-id variants via flags for
  TASK-0179's crown/downgrade lanes.
- Recut note: `write_gpt` deliberately writes NO protective MBR (the
  shared gpt.rs doctrine — nothing boots via MBR; nxboot parses GPT
  directly), so RFC-0089 §2's "protective MBR" line is amended by this
  ledger: the GPT-only form IS the contract.

## REWRITE 2026-08-25 (RFC-0089 lane recut — supersedes the pre-rewrite image-builder scope)

The old segment layout (`[bootloader][kernel][initrd (recovery)][rootfs.squashfs]
[pkgfs.img][state header]`) is dead: there is no initrd/ramdisk (recovery is a
declarative stage graph, TASK-0050/0261 notes), no squashfs, and the disk is the
RFC-0089 §2 GPT layout. `nx flash reboot normal|recovery` is void (next-boot is a
bootctld op). The flasher protocol and factory reset REMAIN in this ledger as the
residual section below — they execute later with TASK-0261, unchanged in spirit.
This task now delivers exactly the host image tooling the OTA lane needs.

## Context

The OTA lane needs one host-side authority that produces the disk QEMU boots and
the artifacts the device verifies: the GPT image with factory BSB, signed NXBDs
per boot slot, and (for TASK-0179's fixtures) `.nxs` v2 OTA containers. The GPT
parser/GUID table already lives host-tested in `userspace/storage` — the builder
REUSES it (one layout authority for `nx image`, `virtioblkd`, and later `nxboot`).

## Goal

`tools/nx` subcommands (single `nx` entrypoint per TRACK-AUTHORITY-NAMING; no
separate binaries), all deterministic:

1. **`nx image build`** — assembles `build/nexus.img` per RFC-0089 §2: protective
   MBR + GPT (GUID table exported from `userspace/storage`; deterministic GUID
   derivation from build inputs) + partitions `bsb | boot-a | boot-b | system-a |
   system-b | state | data`; factory BSB (seq=1, active=a, committed, floor=0);
   NXBD for boot-a signed with the dev publisher key (`--sign <seed>`; key files
   under `keys/`, never logged); embeds the boot image into boot-a from sector 8;
   boot-b NXBD zeroed (= invalid); seeds state/data from the existing image-prep
   inputs. Per-slot size budget enforced at build time.
2. **`nx image verify`** — re-parse GPT (via the SAME `userspace/storage` parser),
   CRC checks, NXBD signature + digest verification, BSB validity; stable exit
   classes per the nx CLI contract.
3. **`nx image patch --part boot-a --in <bin>`** — refresh a boot partition
   (image + re-signed NXBD) WITHOUT touching bsb/state/data. This is end-state
   provisioning behavior (the same partition-scoped write a flasher performs),
   and the keep-blk dev flow after the boot flip.
4. **`nx image ota`** — emit the `.nxs` v2 OTA container for a built image
   (`build/ota/os-<buildid>.nxs`; manifest + `boot-image` component + embedded
   `boot.nxbd`), plus fixture variants (`--build-id-suffix`, `--rollback-index`)
   for TASK-0179's crown/downgrade lanes.

## Non-Goals (now residual, executed with TASK-0261 later)

- **Flasher protocol** (`nx flash send|verify` ↔ flashd; framed magic+seq+len+
  crc32, HELLO/INFO?/WRITE/DONE/ABORT, resume via last-good seq) — unchanged
  design, re-anchored: no `reboot` verb (bootctld op), writes are partition-
  scoped per the RFC-0089 layout.
- **Factory reset** (`nx reset factory --yes`; wipe state except trust & boot,
  golden preserved-path list).
- OS/QEMU integration of the image (TASK-0315 wires the launcher; TASK-0289
  flips the boot).

## Constraints / invariants (hard requirements)

- **One layout authority**: partition offsets/GUIDs come from the shared
  `userspace/storage` table; the builder never hardcodes a second copy.
- **No parallel signature semantics**: NXBD/manifest signing uses the same
  Ed25519 primitives as the repo baseline (RFC-0039/keystored lineage); seeds
  from files, never embedded in code, never logged.
- **Determinism**: build twice ⇒ byte-identical image and container
  (`SOURCE_DATE_EPOCH` discipline; no wall clock in any signed or hashed bytes).
- Sparse output (host FS holes) — the 384 MiB raw image must not bloat CI.
- No `unwrap/expect`; nx exit classes stay the CLI contract.

## Stop conditions (Definition of Done)

### Proof (Host) — required (new `tests/nx_image_host/` or tool-internal tests)

- Determinism: `nx image build` twice ⇒ identical bytes (image + ota container).
- Round-trip: `nx image verify` green on a fresh build; GPT parsed by
  `userspace/storage::gpt` in-test (tool and OS parser agree by construction).
- NXBD vectors: signed descriptor verifies; tampered image ⇒ verify FAIL
  (digest); tampered descriptor ⇒ FAIL (sig).
- BSB factory block: valid magic/CRC/seq=1, golden bytes.
- `patch` preserves bsb/state/data byte-identically; budget-overflow input ⇒
  deterministic build failure.
- `ota` fixtures: build-id/rollback-index variants decode + verify correctly.

No QEMU proof in this task (the image is unwired until TASK-0315/0289); the
regression signal is the host suite plus, later, the lanes that consume the image.

## Touched paths (allowlist)

- `tools/nx/` (image subcommands) + `tests/` (host suite)
- `userspace/storage/` (export the GUID/layout table if not yet public)
- `keys/` (dev publisher key material, documented)
- `docs/provisioning/` (new: image layout, key handling, patch flow)
- `Makefile`/`justfile` recipe additions only when TASK-0315 wires the launcher

## Plan (small PRs)

1. Layout/GUID export from `userspace/storage` + GPT writer + factory BSB +
   determinism tests.
2. NXBD encode/sign/verify + boot-a embedding + budgets + verify command.
3. `patch` + preserved-partition proofs.
4. `ota` container emission + fixture variants + docs.

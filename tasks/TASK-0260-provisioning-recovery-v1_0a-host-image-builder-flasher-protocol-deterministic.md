---
title: TASK-0260 Provisioning v1.0a (host-first): nx image — deterministic GPT disk assembler + NXBD signer + factory BSB + OTA-container emission (flasher/factory-reset = residual)
status: In Progress (P0 done 2026-09-26 — the boot medium measured byte for byte: sector 0 shares the boot-ROM header and a protective MBR, the SPL finds its stages through the GPT, the flash vehicle's `flash gpt` cannot carry our partition types; recut to the end state below; P1 next — the layout and the builder)
note: image-builder scope (the OTA-lane package) DELIVERED 2026-08-25. RECUT 2026-09-22 (Block 1 B1.6 of the hardware fast track): the residual "flasher protocol" is answered — fastboot over the boot-ROM download mode IS the protocol (TASK-0327) — and the residual becomes the boot-ROM head partitions in `nx image`; factory reset stays residual; the nxboot-as-FIT-payload half is TASK-0260B
owner: @reliability
created: 2025-12-29
updated: 2026-09-26
depends-on: []
follow-up-tasks:
  - TASK-0315
  - TASK-0289
  - tasks/TASK-0260B-nxboot-as-fit-payload-chosen-node.md
links:
  - Contract: docs/rfcs/RFC-0089-ota-v2-component-manifest-ab-boot-images-nxboot-bsb.md (§2 layout, §5 NXBD, §6 BSB)
  - BSB factory role: docs/adr/0058-boot-selection-block-dual-actor-discipline.md
  - Block topology: docs/adr/0044-single-blk-device-gpt-partitions-block-layer.md + tasks/TASK-0315-block-topology-consolidation-gpt-virtioblkd-sole-owner.md
  - GPT/GUID authority (shared, host-tested): userspace/storage/src/gpt.rs
  - Signing primitives baseline: tasks/TASK-0029-supply-chain-v1-sbom-repro-sign-policy.md
  - Testing contract: scripts/qemu-test.sh
  - Measurement: docs/board/measurements/2026-09-26-boot-medium/README.md (P0), docs/board/measurements/2026-09-22-stock-system/README.md (boot flow)
  - Decisions: docs/adr/0066-boot-chain-on-hardware-nxboot-as-fit-payload.md, docs/adr/0067-one-block-owner-backend-selected-by-fdt.md
---

## RECUT 2026-09-26 — P0 measured: the boot medium, byte for byte (Block 1 B1.6; supersedes the 2026-09-22 recut below where they differ)

Measured on the desk board over `adb` (the stock microSD boots; nothing written) and in the pinned
vendor pieces (`docs/board/measurements/2026-09-26-boot-medium/`):

- **Sector 0 is shared.** A bootable medium carries the boot-ROM header in bytes 0–79 (the pinned
  `bootinfo_sd.bin`, byte-identical) AND a protective MBR in the same sector (one `0xee` entry from
  LBA 1 over the disk, `0x55aa`). U-Boot's GPT driver — and so the SPL — sees no GPT without the
  protective MBR (upstream `part_test_efi` → `is_pmbr_valid`). `storage::gpt` writes none today
  (RFC-0089 §2's 2026-08-25 amendment): a board disk built that way has no partitions for the SPL.
- **The head is four GPT partitions in a fixed order:** `fsbl` 128 KiB/256 KiB (#1), `env`
  384 KiB/64 KiB (#2), `opensbi` 1 MiB/1 MiB (#3), `uboot` 2 MiB/2 MiB (#4); the SPL finds the
  last two through the GPT (by name or by number — its strings carry both the names and
  upstream's number-path message), so names, numbers AND offsets stay as measured. The payload's
  slot keeps the SPL's name `uboot` whatever it holds (TASK-0260B's FIT with nxboot).
- **The GPT's backup lies at the device's last sector** (the stock medium: 62 423 039); the eMMC
  has 30 535 680 sectors, all of 0–33 zero, no partition (never flashed).
- **The vehicle cannot write our GPT through `flash gpt`:** it builds a GPT from its JSON with
  `gpt write` (every partition typed basic data — the stock medium's GPT; the JSON has no type
  field); upstream's raw `flash gpt` path is absent. Our stages find partitions by name AND type
  (`find_partition_named`), so a vehicle-built GPT would leave nxboot without its `bsb`. The
  vehicle does have raw writes (`bootinfo`, `hidden` JSON regions, `fastboot_raw_partition_*`,
  sparse images) and read-back (`oem read` + `upload`).
- The ledger's 2026-09-22 line "one `fastboot flash` per named partition" therefore cannot be
  the flash path, and `swap` needs no reservation: disposable and last, M7 appends it when it
  has measured its size — it moves no other partition.

### End state (binding)

- **One layout, one builder.** `storage::layout` describes the whole disk: the head as GPT
  partitions 1–4 with the measured names, offsets and order, typed `NEXUS-FW-v1` (no stage of
  ours reads or writes them at run time — `blkd` refuses them, deny-by-default); then our
  volumes from 4 MiB in RFC-0089 §2's order (`bsb`, `boot-a/b`, `system-a/b`, `state`, `data`).
  `swap` (M7) is appended last when M7 sizes it; `data` growing into the rest of the eMMC is a
  later provisioning step (nxfs grow), not this task.
- **Every image carries a protective MBR** (`storage::gpt::write_gpt`); the **board image adds
  the boot-ROM header** in bytes 0–79 of the same sector. RFC-0089 §2's "GPT-only" amendment is
  withdrawn.
- **The GPT describes the device it lands on:** `nx image build --target qemu|bpi-f3` — the
  target names the disk (QEMU: the image, including the SDHCI lane's 4 GiB, which the launcher
  passes to the builder instead of growing the file afterwards; the board: the eMMC's measured
  sector count) and the head's contents. The backup header sits at the device's last sector.
- **The board image is the QEMU layout plus the head's contents:** `bootinfo` (sector 0),
  `fsbl`, `opensbi` from the pinned vendor pieces (`fetch-board-inputs.sh` checks their hashes),
  `env` zero (no U-Boot reads it), `uboot` = TASK-0260B's FIT. A QEMU image needs no vendor piece:
  its head partitions stay zero.
- **The flash path writes our bytes.** `nx image` derives the flash plan from the layout;
  `board-flash.sh` writes the image and our backup GPT through the vehicle's raw writes — never
  its JSON GPT as the disk's truth — and reads the GPT back and compares it with the image. The
  eMMC is written only with the user's go.

### Packages

- **P0 — measured and recut ✅ 2026-09-26.** The measurement above; this recut; RFC-0089 §2,
  RFC-0098 C6 and ADR-0066 amended.
- **P1 — the layout and the builder (host).** The head in `storage::layout` (`NEXUS-FW-v1`);
  the protective MBR in `write_gpt`; the device size in the GPT (backup at the device's end);
  `nx image build --target`; the board head (sector 0 header + the pinned pieces); the launcher's
  image size into the builder; `blkd` refuses the head (`test_reject_*`); the GPT marker
  (`blkd: gpt ok (parts=…)`) moved with `scripts/qemu-test.sh` + docs. Host: goldens — the head at
  the measured offsets and numbers, sector 0 byte-exact against the stock medium's shape (header
  + protective MBR), the backup at the device's end, the QEMU and board images identical but for
  sector 0's header, the head's contents and the disk size; our volumes where the parser finds
  them; `sgdisk -v` clean on both. QEMU: `test-all` green with the volumes moved to 4 MiB.
- **P2 — the flash path (board, with the user).** First measured in U-Boot fastboot mode with
  nothing written (`board-flash.sh --stage-only`, download mode by the user): which raw writes
  the vehicle takes beyond `bootinfo` (a `hidden` region at offset 0 over the image, sparse above
  `max-download-size`), and the read-back. Then the flash plan from the layout, the eMMC written
  (the user's go), read back byte-exact, and the stock system reading the eMMC's GPT over adb
  (our names, our types). If the vehicle writes no raw region beyond `bootinfo`, P2 stops and
  records it; the answer then is our own flash vehicle (TASK-0261's gadget), never a
  vehicle-typed GPT.
- **P3 — with TASK-0260B:** the board boots nxboot from the `uboot` slot — the serial ladder
  (`nxboot: platform=…` → kernel banner → `init: ready`); which lookup the SPL used is recorded.

### Red flags / decision points

- **YELLOW: the SPL's stage lookup (name or number)** — answered by keeping both; P3 records it.
- **YELLOW: the vehicle's raw writes beyond `bootinfo`** — measured on the next SoC generation's
  public table, not yet on this board's vehicle; P2 measures before writing.
- **YELLOW: the board image's `data` is 128 MiB of 14.56 GiB** — enough for Block 1; growth is
  a later provisioning step.
- **GREEN: the eMMC is empty and the boot ROM tries the microSD first** — a wrong eMMC write
  never keeps the stock system from booting; download mode recovers.
- **DECISION: `swap` is not reserved here** — disposable and last, M7 appends it with a measured
  size (the 2026-09-22 order row reserved it; a guessed size would be an unmeasured number in
  every image).


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

## RECUT 2026-09-22 — the residual on hardware (Block 1 B1.6; ADR-0066, ADR-0067, RFC-0098 C5/C6) — superseded where the P0 recut above differs (the flash path, the head's order and names, `swap`)

Measured 2026-09-21/22 on the reference board (`docs/board/bpi-f3.md`, `docs/board/measurements/
2026-09-22-stock-system/README.md`): the boot ROM reads an 80-byte `bootinfo` header at offset 0
of the medium, the SPL from `fsbl` (128 KiB, 256 KiB), OpenSBI's FIT from `opensbi` (1 MiB) and
the payload FIT from `uboot` (2 MiB, 2 MiB); the vendor's flasher is plain fastboot
(`fastboot flash <partition> <file>` from a U-Boot staged into RAM), which `scripts/board-flash.sh`
already speaks. So:

- **The flasher protocol residual is closed by decision**: no `nx flash send|verify` ↔ `flashd`
  framing; fastboot over the boot-ROM download mode is THE protocol (TASK-0327 T2 measured it),
  and the device-side flashd of TASK-0261 becomes a recovery-target fastboot gadget later.
- **The residual that stays here**: `nx image` emits the board's boot-ROM head in the SAME
  image it builds for QEMU — `bootinfo` (the vendor header, pinned), `fsbl` (the pinned SPL),
  `env` (empty placeholder, no U-Boot), `opensbi` (the pinned `fw_dynamic.itb`), the FIT slot
  (TASK-0260B builds its content) — as partitions of the layout SSOT
  (`userspace/storage/src/layout.rs`, ADR-0067), followed by `bsb`, `boot-a/b`, `system-a/b`,
  `state`, `data` (+ `swap`, reserved for M7). `scripts/board-flash.sh` learns the partition
  list from that SSOT (one `fastboot flash` per named partition), and the "vendor boot vehicle"
  wording in its banner dies with it.
- **Factory reset** stays residual (executes with the recovery target).

Packages: **P1** layout SSOT head partitions + `nx image` emission (host-tested GPT goldens:
the head at the vendor offsets, our volumes after it; `sgdisk` verifies the built image);
**P2** `board-flash.sh` from the SSOT + the eMMC written on the desk board
(`fastboot flash` per partition, `partition-size:*` answers afterwards — today they fail on the
empty eMMC); **P3** with TASK-0260B: the board boots the image (`nxboot: slot a` on serial).
Gate: `contract-image-layout` extended to the head; the board reads its GPT back
(`sgdisk -p` over `blkd` later, `fastboot getvar partition-size:boot-a` now).

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

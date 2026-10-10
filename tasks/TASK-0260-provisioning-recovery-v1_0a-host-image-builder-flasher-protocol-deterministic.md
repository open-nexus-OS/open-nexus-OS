---
title: TASK-0260 Provisioning v1.0a (host-first): nx image — deterministic GPT disk assembler + NXBD signer + factory BSB + OTA-container emission (flasher/factory-reset = residual)
status: Done 2026-10-10 (Block 1 closure — P3 ran as TASK-0260B P2/P3: the boot ROM boots our boot0 from the eMMC with the microSD out, the SPL takes our FIT and finds its stages by partition name, nxboot verifies and jumps, the board reaches `init: ready` 2026-09-27/29 and boots the desktop on every cycle since; residuals named below: factory reset → TASK-0261, `data` growth → a provisioning step after Block 4, `swap` → M7; was "In Progress (P2 done 2026-09-26 — the flash path: the eMMC boots from boot0 (the vendor source), our disk and boot0 written through raw partitions and read back exactly; P3 next — with TASK-0260B, the board boots it; P1 done 2026-09-26 — the layout and the builder on the host: every image a complete GPT with a protective MBR for its disk, the head as partitions 1–4, `nx image build --target`, `blkd` by name and type; P2 next — the flash path, with the user at the board; P0 done 2026-09-26 — the boot medium measured byte for byte: sector 0 shares the boot-ROM header and a protective MBR, the SPL finds its stages through the GPT, the flash vehicle's `flash gpt` cannot carry our partition types; recut to the end state below; P1 next — the layout and the builder)
note: image-builder scope (the OTA-lane package) DELIVERED 2026-08-25. RECUT 2026-09-22 (Block 1 B1.6 of the hardware fast track): the residual "flasher protocol" is answered — fastboot over the boot-ROM download mode IS the protocol (TASK-0327) — and the residual becomes the boot-ROM head partitions in `nx image`; factory reset stays residual; the nxboot-as-FIT-payload half is TASK-0260B
owner: @reliability
created: 2025-12-29
updated: 2026-10-10
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

### End state (binding; the head recut at P2 — the eMMC boots from boot0)

- **One layout, one builder.** `storage::layout` describes the whole disk: the head the SPL loads
  by name as GPT partitions 1–2 (`opensbi`, `uboot`), typed `NEXUS-FW-v1` (no stage of ours reads
  or writes them at run time — `blkd` serves no selector for them); then our volumes from 4 MiB
  in RFC-0089 §2's order (`bsb`, `boot-a/b`, `system-a/b`, `state`, `data`).
  `swap` (M7) is appended last when M7 sizes it; `data` growing into the rest of the eMMC is a
  later provisioning step (nxfs grow), not this task.
- **Every image carries a protective MBR** (`storage::gpt::write_gpt`); RFC-0089 §2's "GPT-only"
  amendment is withdrawn. The eMMC's boot-ROM header and SPL live in its boot0 hardware
  partition, beside the user area (below), not in sector 0.
- **The GPT describes the device it lands on:** `nx image build --target qemu|bpi-f3` — the
  target names the disk (QEMU: the image, including the SDHCI lane's 4 GiB, which the launcher
  passes to the builder instead of growing the file afterwards; the board: the eMMC's measured
  sector count) and the head's contents. The backup header sits at the device's last sector.
- **The board image is the QEMU layout plus the head's contents, and boot0 beside it:**
  `opensbi` from the pinned vendor piece, `uboot` = TASK-0260B's FIT; `<image>.boot0` = the eMMC
  boot-ROM header at 0 (checked as the boot ROM checks it, and the eMMC's) and the SPL at the
  offset the header names (`fetch-board-inputs.sh` checks the pieces' hashes). A QEMU image needs
  no vendor piece: its head partitions stay zero and it has no boot0.
- **The flash path writes our bytes.** `nx image flash-plan` turns a board image into raw
  regions — the user area in chunks of one download, the backup GPT, boot0 — each with its
  digest; `board-flash.sh --plan` declares each in the vehicle as a raw partition
  (`oem env:set fastboot_raw_partition_<name>`, 64-bit block addresses, hardware partition
  included), checks its size, writes it; `--verify` reads every region back from the stock
  system. The vehicle's JSON GPT and JSON regions are never used. The eMMC is written only with
  the user's go.
  **Amended 2026-10-04:** the vehicle sniffs every download for gzip by the header's method byte
  alone (offset 2 = 8, no magic check) and inflates what it takes for one: a user-area chunk that
  began `44 e4 08 00` failed `unzip gzip data fail`. The plan starts no region on such a sector —
  a chunk boundary moves back to the nearest one the vehicle does not sniff (within 1 MiB, else
  refused), the backup GPT's region grows back into the free space, a sniffed sector 0 is refused
  (`image_flash_cli`: `no_region_starts_where_the_vehicle_sniffs_gzip`,
  `test_reject_a_boundary_the_vehicle_sniffs_all_the_way_back`; five mutations each killed).
  Proven on the desk board: the corrected plan (boundary at sector 524 287) written with
  `--skip-stage` (`docs/board/measurements/2026-10-04-boot-led-flag/`).

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
- **P1 — done 2026-09-26.** `storage::gpt::write_gpt` lays down a complete GPT for the disk it
  writes: the protective MBR in sector 0 (UEFI CHS values; the boot code area left free), the
  primary header and entries, and the backup entries and header in the last sectors. The
  partitions get distinct unique GUIDs (`NEXUS_DISK_GUID` plus their index). It refuses
  partitions outside the usable range, overlapping or badly named. `parse_gpt` refuses a header
  that is not the primary and partitions outside the usable range. `storage::layout` starts with
  the head (`Place::At`, `NEXUS-FW-v1`), our volumes follow from 4 MiB, and `type_of(name)`
  gives the layout's type. `nx image build --target qemu|bpi-f3`: QEMU's image at the layout's
  384 MiB or `--disk-bytes`, which the launcher now passes instead of growing the file
  afterwards. A board's image comes from `config/board/<board>/image.toml`: exactly the disk's
  size (sparse), the boot-ROM header in sector 0, checked as the boot ROM checks it (length,
  magic, CRC-32 of the first 64 bytes), the SPL and OpenSBI in their partitions, and `--fit`
  into `uboot`. `blkd` serves its volumes by layout name AND type (`blkd::parts`, host-tested);
  until now it matched the name alone, so the P0 recut's "our stages find partitions by name and
  type" held for nxboot and `nx image` only. Measured: `blkd` counts the volumes it serves, so
  the marker stays `blkd: gpt ok (parts=7)`; the head has no selector at all. Proof: host —
  `storage` (the writer: the protective MBR, both headers and entry arrays, distinct unique
  GUIDs; a layout the disk cannot hold refused; the parser refusing a backup at LBA 1, partitions
  over either GPT copy, an empty usable range; the head at the measured offsets and numbers),
  `blkd` `tests/parts.rs` 3 (every selector on the layout; the head never served; a volume under
  another type refused), `nx` 116 with `image_board_cli` 4 new (a QEMU disk of the size a lane
  asks for, its backup at the end; a board image exactly its disk — sector 0's header beside the
  protective MBR, the head's pieces, the volumes byte-identical to QEMU's; the boot ROM's
  refusals; a disk or a piece that does not fit). Three tests that carried the old offsets now
  read the layout. Six mutations each fail their test: no protective MBR, no backup header, the
  head off its offsets, served by name only, the header's CRC unchecked, the parser accepting any
  range. Real images: `sgdisk -v` finds no problem in the QEMU image or in the board image built
  from the pinned pieces. `sgdisk -p` shows the head at 256/768/2048/4096 and the volumes from
  8192. The board image is 15 634 268 160 bytes, the eMMC exactly. Its sector 0 matches the stock
  medium in bytes 0–79 and in the protective entry's type and start; only CHS (UEFI values) and
  the size (another disk) differ. `nx image verify` passes both. `just check` green;
  `build-os-workspace` 0 warnings; `just test-all` green (EXIT=0, 12 QEMU lanes, every OTA lane
  among them). 21 loader boots (20 virtio, 1 SD host), no skip. `blkd: gpt ok (parts=7)` and
  `blkd: backend ok` appear in all 19 boots that reach init, with no parse failure. smp1 shows
  430 and the SDHCI lane 429 ok lines, as before the package.
- **P2 — the flash path — done 2026-09-26.** Read first in the vendor's public U-Boot tree (its
  fastboot flash driver and the board's defconfig; `docs/board/measurements/2026-09-26-boot-medium`,
  addendum), and it corrects P0/P1's head. The eMMC boots from its **boot0** hardware partition:
  `flash bootinfo` writes an eMMC header there at 0, `flash fsbl` (`MMC_BOOT1_NAME="fsbl"`) the
  SPL at `0x200`. The SPL loads `opensbi` and `uboot` from the user area **by name**
  (`SYS_LOAD_IMAGE_*_PARTITION_NAME`). P1's sector-0 header and user-area `fsbl`/`env` were the SD
  card's form. The vehicle's JSON regions compute offsets in 32 bits and cannot reach the backup
  GPT, but its environment is writable and it keeps upstream's raw partitions. Measured in U-Boot
  fastboot mode with nothing written: a raw partition declared through `oem env:set` answers
  `getvar partition-size` exactly — at `0x3000`, at the eMMC's last 34 sectors, and as boot0
  (`mmcpart 1`). Built: the layout's head is `opensbi` + `uboot` (partitions 1–2). The board image
  writes no header into sector 0; boot0 (`<image>.boot0`) holds the eMMC header and the SPL at the
  header's offset (checked: magic, CRC, media tag `eMMC`, the SPL within the header's limit and
  boot0). `nx image flash-plan` emits the raw regions and their digests. `board-flash.sh --plan`
  declares, sizes and writes them, and `--verify` reads them back over adb; the vendor
  boot-vehicle write is deleted. `just board-image` builds the disk and the plan from the last OS
  build. Written with the user's go, read back exactly. Proof: on the desk board, the vehicle
  staged into RAM (`--stage-only`) and the raw partitions probed with nothing written; then, with
  the user's go, the plan was written. The user area took 256 MiB in 16.7 s and 117 MiB in 8.0 s;
  then came the backup GPT at sector 30 535 647 and boot0 (`mmcpart 1`), each sized through
  `getvar` first. After the reset into the stock system, `--verify` finds every region's SHA-256
  equal to the plan's. The stock kernel reads `mmcblk2: p1 … p9` without a GPT warning, with our
  names, offsets, type GUIDs and distinct unique GUIDs (transcripts in the measurement). Host:
  `nx` 119 tests pass, with `image_board_cli` 4 on the eMMC model and `image_flash_cli` 3 new
  (the regions rebuild the disk exactly; the same image gives the same plan; what is no board
  disk is refused). `storage` covers the head as `opensbi` + `uboot`, and `blkd` its parts test.
  Six mutations each fail their test: backup region off by one, boot0 on the user area, the last
  chunk uncut, the media tag unchecked, the SPL at the vendor's `0x200` instead of the header's
  offset, zero pieces dropped. That fifth one survived at first, because the fixture used
  `0x200`; the fixture now uses `0x400`. `just board-image` builds the disk and the plan from the
  real artifacts. `just check` green; `build-os-workspace` 0 warnings; `just test-all` green
  (EXIT=0, 12 QEMU lanes on the nine-partition layout): 21 loader boots with no skip, and
  `blkd: gpt ok (parts=7)` in all 19 boots that reach init. smp1 shows 430 and the SDHCI lane 429
  ok lines, as before.
- **P3 — with TASK-0260B: ✅ (run as TASK-0260B P2/P3, 2026-09-27/29; closed 2026-10-10).**
  The board boots nxboot from the `uboot` slot: with the microSD out the boot ROM takes our boot0
  from the eMMC (TASK-0260B P2 step 3), the SPL picks the FIT configuration by `description`
  against `product_name=k1-x_deb1` and finds `opensbi`/`uboot` by partition name — the lookup is
  recorded in `docs/board/measurements/2026-09-27-first-emmc-boot/README.md` — and the ladder runs
  `nxboot: platform=…` → `nxboot: verify ok` → kernel → `init: ready` (TASK-0260B Done). The
  channel is the boot trace on the eMMC (RFC-0107), not a serial console (TASK-0327B P0's recut).

### Red flags / decision points

- **GREEN (P2): the SPL's stage lookup** — by name (the vendor SPL's configuration).
- **GREEN (P2): the vehicle's raw writes** — raw partitions through its environment, 64-bit,
  boot0 included; written and read back exactly.
- **GREEN (P3, TASK-0260B 2026-09-27): boot0's header path boots** — with the microSD out the
  boot ROM takes our boot0 from the eMMC; the SD card stays the desk's way back to the stock
  system.
- **YELLOW (residual, not Block 1): the board image's `data` is 128 MiB of 14.56 GiB** — enough
  for Block 1; growing it to the device is a provisioning step after Block 4 (with the first
  user data that needs the space).
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

(2026-10-10: the flasher protocol below is answered by decision — fastboot over the boot-ROM
download mode, TASK-0327; what stays residual is factory reset, TASK-0261, parked until the
USB stack has gadget mode.)

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

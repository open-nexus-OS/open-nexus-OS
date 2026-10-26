<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# ADR-0081: The boot image is one verified container — the kernel and a bootfs of sealed images; the loader places both, the kernel adopts the bootfs, init and every core program run from it, and no program embeds another

- Status: Accepted (2026-10-26, board cycle 71 — `docs/board/measurements/2026-10-26-boot-image/`)
- Date: 2026-10-26
- Links:
  - Tasks: `tasks/TASK-0333-memory-the-efficient-one-and-the-app-lifecycle-layer-6.md` (6c-2)
  - RFCs: `docs/rfcs/RFC-0089-ota-v2-component-manifest-ab-boot-images-nxboot-bsb.md` (§5 the
    payload's layout, §7 the loader's steps — amended by this decision), `docs/rfcs/RFC-0098-…`
    (C2 the loader owns `/chosen`, C6 the window), `docs/rfcs/RFC-0100-…` (§2 the sealed kind)
  - Related ADRs: `docs/adr/0059-…` (the loader's handoff: measurement in `/chosen`),
    `docs/adr/0060-…` (the verified volume: unchanged — the bootfs is the tier above it),
    `docs/adr/0066-…` (the loader owns the tree), `docs/adr/0080-…` (a process runs from a
    sealed image)

## Context

The boot image is a flat kernel binary. The kernel's `.rodata` holds init-lite's ELF
(`include_bytes!`, 9.7 MB); init-lite's `.rodata` holds every core service's ELF
(`SERVICE_IMAGE_TABLE`); execd's `.rodata` holds app-host's and the recv-wake probe's ELFs
(6.4 MB of execd's 6.8 MB). Measured on board cycle 70 (`board--2026-10-26T10-32-12`): the
kernel image excludes 4 671 frames (18.2 MiB) from the pool, of which 9.7 MB are init-lite's
bytes; the kernel copies those bytes page by page into init's space at boot (`KSELFTEST: segment
copied`), a second 9.7 MB; init copies every core ELF out of its own rodata into a sealed object
(`sealed_image`, `vmo_write` in 64 KiB chunks), a third copy of ≈ 8 MB; execd's own image carries
app-host's bytes, and execd copies them once more into app-host's sealed object. Every byte of a
core program is therefore in memory three to four times, and two of those copies are made at
every boot. ADR-0080 made a process run from a sealed object without a copy; the objects
themselves are still made by copying.

The alternatives are the ones every capability OS has chosen between: a kernel that embeds its
first program (ours today; a chain of embeddings), a kernel linked with the program set as a
section, or a bootfs the loader places beside the kernel and the kernel hands to its first
program as an object (Zircon's ZBI with its bootfs item handed to userboot; Linux's initrd named
in `/chosen`). The loader already verifies ONE signed payload against ONE descriptor (RFC-0089
§5), owns the tree it hands the kernel and writes the loader's decisions into `/chosen`
(RFC-0098 C2, ADR-0066), and the kernel already hands init a kernel-owned, read-only, fixed
object — the device tree — in a fixed slot (`boot_fdt::init_alias`, RFC-0098 C3).

## Decision

- **The boot payload is a container of two items, verified as one.** The bytes the NXBD's
  `image_size`/`image_sha256` cover are: one header sector (`NXBI0001`: the kernel item's
  offset, length and memory footprint, the bootfs item's offset and length), the kernel item (the
  flat kernel, its own bytes only), the bootfs item. The NXBD, its signature, the BSB and the
  loader's slot rules are unchanged: one digest covers the kernel AND every program that starts
  before the volume is verified. A tampered bootfs byte is a tampered image (`nxboot: verify FAIL
  (… digest)`), nothing new to sign, nothing new to trust.
- **The loader places both items.** `nxboot` reads the header first, bounds every field against
  the slot's budget and its window, reads the kernel item to the window's base and the bootfs item
  to the first 2 MiB boundary past the kernel's memory footprint (its `.bss` and stack included —
  the bootfs is never inside memory the kernel zeroes), streams the digest over the three regions
  in payload order, prints `nxboot: bootfs (at= len=)`, and names the bootfs to the kernel as
  `/chosen/nexus,bootfs-start` and `/chosen/nexus,bootfs-len` (physical, each a u64 in the
  tree's byte order) in the tree copy it already owns. No kernel address is contracted between
  loader and kernel (ADR-0059 as amended) — the kernel learns the bootfs the way it learns
  everything else: from the tree. The loader's FAIL vocabulary grows by `payload-header (…)`,
  `payload-size` and `payload-window`.
- **The kernel adopts the bootfs and never writes it.** The range is excluded from the frame pool
  like the kernel image and the tree. The bootfs's table of contents (`NXFS0001`: page-aligned
  files, each with a name, a stack size and a global pointer) is parsed by the one bounded codec
  (`nexus-bootfs`, host-tested, shared by the packer, the loader's tests, the kernel and init);
  every file becomes a **fixed object born sealed** — frames the pool does not own and no writer
  can reach, the sealed kind ADR-0080 defined, which `exec_vmo` accepts and `vmo_write`, a
  writable map and `vmo_runs` refuse. A fixed object born unsealed (device memory, RFC-0098 C3)
  stays unsealable: `vmo_seal` keeps refusing the fixed kind; only the kernel makes a sealed
  fixed object, only from the bootfs. Proof rung: `KSELFTEST: bootfs ok (files= bytes=)`; no bootfs
  or a bad table is a boot-gate failure, never a guess.
- **init is the bootfs's first file and runs through the one loader.** The kernel spawns init
  with `exec::load_image` from the sealed object named `init-lite` — the same code path, the same
  shared text, the same `KINIT: exec image` record as every other process; the kernel's private
  ELF copier (`load_init_elf`, `selftest/init_image.rs`) is deleted. init receives, in fixed slots
  `nexus-abi` names (`INIT_BOOTFS_TOC_SLOT` = 4 next to the factory, the tree and the console
  ring; `INIT_BOOTFS_FILES_SLOT` = 32 and up), the table of contents read-only and one `VmoRo`
  capability per file, in table order.
- **init spawns the core services from their file objects and hands the rest to their owners.**
  A file is a core service unless the topology (`nexus-service-topology::bootfs`) declares it a
  hand-off: `app-host` and `recv-wake-probe` go to execd's declared slots (`APPHOST_IMAGE` = 31,
  `RECVWAKE_IMAGE` = 32) at execd's spawn (`cap_transfer_to_slot`, the MMIO-grant pattern) and
  execd starts app-hosts from the slot's object. init-lite's build script generates no table; execd's build script embeds no service;
  the kernel's build takes no `EMBED_INIT_ELF`. The retired-name gate keeps the chain gone.
- **Out of scope**: the volume's bundles (ADR-0060 stands: the recovery path needs no volume,
  the bootfs is what it boots from); the probe fixtures execd keeps for its own tests (tiny
  generated ELFs, not programs of the system); A/B at bootfs granularity (the bootfs is part of
  the one image the slot carries); dropping clean pages under pressure (6e/6f).

## Consequences

- **Positive**: every core program's bytes exist once, in frames the loader verified and nobody
  can write; the kernel image shrinks to the kernel (≈ 0.5 MB of file), init-lite and execd to
  themselves (≈ 0.3–0.4 MB each); two boot-time copies (the kernel's page copy of init, init's
  `vmo_write` of every core ELF) are gone; the kernel builds without the service set, so a
  service change relinks nothing above it; the image a tamper or a downgrade backstop rejects
  covers the programs too.
- **Negative / accepted cost**: a payload header and a bootfs table — two fixed-layout formats
  with their reject matrices; `nxboot` reads two regions instead of one and its tests carry a
  container fixture; the direct `-kernel` boot (`NEXUS_DIRECT_KERNEL`, a loader-less development
  knob) has no bootfs and is retired — the loader is in every chain; the kernel's `cap` table
  for init grows by one slot per bootfs file.
- **Follow-ups**: 6d — init reaps no dead child (measured 6c-1b); 6e/6f — pressure.

## Alternatives considered

- **Link the bootfs into the kernel as a page-aligned section.** No loader change and no new
  format, and the memory property is the same — but the kernel is relinked for every service
  change, the kernel's `.text` segment carries 11 MB it never executes, and the programs' bytes
  are inside the kernel's own mapping for ever. Rejected: the chain is shortened, not gone.
- **A second signed artifact (its own NXBD, its own partition) for the bootfs.** Clean
  separation, but a second descriptor the loader verifies, a second A/B pairing, a second
  rollback floor — a trust-root change for no gain over one digest covering both items.
  Rejected.
- **Append the bootfs to the flat kernel and let the kernel find it at its image end.** The
  kernel's `.bss` (8.8 MB) follows its file bytes in memory: the bootfs would either be zeroed by
  the kernel's own start or need 8.8 MB of padding in every slot and every digest. Rejected: the
  loader knows the footprint; it should place the item.
- **One object for the whole bootfs and an `exec_vmo` that takes an offset.** Fewer
  capabilities, but an image stops being an object, every spawner parses the table, and the ABI
  grows an argument whose misuse maps the wrong program. Rejected: one file, one sealed object,
  `exec_vmo` unchanged.

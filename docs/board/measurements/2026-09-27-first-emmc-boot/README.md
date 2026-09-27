<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# The first boot from the eMMC — 2026-09-27 (TASK-0260B P2)

The board's own chain on the desk board, no serial adapter attached: the boot ROM, the vendor SPL
and OpenSBI (pinned, `resources/board/bpi-f3/PROVENANCE.md`), then our loader from the FIT in
`uboot`. The only witness is the boot trace (RFC-0107). Each attempt ran like this:

1. The disk was flashed (`just board-image`, `just board-flash --plan`) and read back exactly
   (`--verify`).
2. A baseline `just board-log` found the trace partition empty.
3. The microSD came out and the board was reset (the boot ROM, finding no card, boots the
   eMMC), and it ran for a minute.
4. The microSD went back in, the board was reset, and `just board-log` pulled the trace from
   the stock system.

A USB watcher logged the board's USB identity throughout.

Files:
- `trace-first-boot.txt`: the trace of the first boot that reached our loader.
- `usb-watch.txt`: the USB identities during the attempts.
- `ext_csd.txt`: the eMMC's boot configuration.
- `vendor-opensbi-strings.txt`: what the pinned OpenSBI matches and reads, from its binary.

## Attempt A: nothing reached the loader

The first FIT carried nxboot and our tree as it was, and two tries gave the same result:
- The trace kept no boot (`just board-log` exit 5).
- USB was silent for the whole boot: the board was never in the boot ROM's download mode, and
  never in the SPL's fastboot wait (`usb-watch.txt`). So neither fell back.
- Only the power LED was lit.

What the chain reads, measured before anything was changed:

- **The eMMC's boot configuration is not the cause.** `PARTITION_CONFIG` is `0x00`
  (`ext_csd.txt`).
  - That is the state the vendor's own eMMC flow leaves: its fastboot `clear_emmc()` sets
    `mmc_set_part_conf(mmc, 0, 0, 1)`, and nothing after it enables a boot partition.
  - So the boot ROM reads boot0 by partition access.
- **The SPL takes our FIT.** Its strings:
  - It picks a configuration by `description` against `product_name=k1-x_deb1`.
  - It knows payloads by their OS name.
  - It finds `opensbi`/`uboot` by partition name.
  - On a failure it hangs (`SPL: failed to boot from all boot devices`,
    `### ERROR ### Please RESET the board ###`): consistent with the silence, but not
    specific.
- **OpenSBI reads our tree.** The SPL hands the FIT's tree to OpenSBI, so the pinned OpenSBI
  runs on our `board.dts`, not on the vendor's.
  - Its K1 platform code matches `spacemit,k1`, which we carry, and reads nothing from the
    tree (its source).
  - It does not read `riscv,isa` at all (`vendor-opensbi-strings.txt`), so our
    `riscv,isa-extensions`-only CPUs are fine.
  - **Our CLINT node had no `interrupts-extended`.** OpenSBI's ACLINT drivers learn the harts
    they serve from that list (OpenSBI v1.3):
    1. `fdt_parse_aclint_node` returns success with 0 harts when the list is missing.
    2. `aclint_mtimer_cold_init` accepts 0 harts.
    3. `aclint_mtimer_warm_init` then finds no timer for the boot hart and returns
       `SBI_ENODEV`.
    4. `init_coldboot` prints "timer init failed" and calls `sbi_hart_hang()`.
    That is a silent stop before the next stage, exactly what attempt A showed. QEMU never
    showed it because QEMU brings its own tree, CLINT list included.
  - Its console driver matches `spacemit,pxa-uart`, which our UART lacked. That is harmless,
    but it meant no firmware console.

The fix, in `config/board/bpi-f3/board.dts`:
- The CLINT names every hart's machine software (3) and machine timer (7) interrupt, as the
  mainline K1 tree does.
- The UART also carries `spacemit,pxa-uart`.

A host test now holds the board tree to what the firmware reads
(`nexus-fdt` `the_board_firmware_finds_every_hart_on_the_clint_and_its_console`). `just
board-goldens` keeps the golden that test reads byte-equal to `board.dts`.

## Attempt B: the loader runs from the eMMC (`trace-first-boot.txt`)

```
nxboot: platform=bananapi,bpi-f3 tree=0x268000 size=13376
nxboot: trace slot=0 seq=1
nxboot: bsb ok (slot=a seq=1)
nxboot: verify ok (slot=a build=dev-67a4 rbidx=1)
nxboot: fdt ok (harts=8 tb=24000000Hz slot=a disk=/soc/storage-bus/mmc@d4281000)
nxboot: jump slot=a base=0x400000
```

- **The chain runs:** boot ROM → SPL → OpenSBI → nxboot → the verified kernel. nxboot:
  - found the eMMC through the SD host the tree names (`mmc@d4281000`);
  - read the boot selection block;
  - verified slot A's descriptor signature and image digest;
  - wrote `/chosen`;
  - jumped to the kernel at `0x400000`, the lowest free 2 MiB-aligned window above
    OpenSBI's reserve and the loader.
- **R1, which tree reaches `a1`: ours, at `0x268000`.** That is `0x200000` + `0x68000`, the
  payload padded to nxboot's whole footprint (416 KiB). The SPL put the tree right after the
  payload's bytes, as its source says.
  - Unpadded, the tree would have landed at `0x220a28`, inside nxboot's `.bss`.
  - Its size, 13 376 bytes, is our 11 792 plus what the SPL and OpenSBI added in place.
- **One boot kept:** no reset loop within the minute.
- **What this does not show:** how far the kernel came. The kernel's console reaches the
  trace with RFC-0107 Phase 2 (TASK-0327B P2).

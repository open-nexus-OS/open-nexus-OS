# 2026-10-04 — the boot LED behind a deprecated flag; the flash plan and the vehicle's gzip sniff

After step 3a the operator asked why the monitor stays black so long before the splash, and
whether the LED ladder is still needed now that the greeter shows and the eMMC trace is read
after every boot. It is not: the trace (RFC-0107) observes everything from the loader's first
line, and the ladder cost every board boot ~22 s of busy waits. Operator decision: keep the
ladder behind a flag, marked deprecated.

## Question

How much of the black phase before the splash is the LED ladder, and does the board still reach
the desktop with the LED as a two-state witness only?

## What changed

- `/chosen/nexus,boot-led-ladder` (an empty property; `nexus_fdt::Chosen::boot_led_ladder()`,
  golden-tested on both trees and on a copy with the property set) turns the slow ladder on.
  Absent (the default, both trees), the LED is lit at milestone 1 — nxboot lights it, no hold —
  and goes dark at milestone 11 (the runtime). A kernel that stops early leaves it lit; a panic
  still flickers it. With the flag, the kernel prints `KINIT: boot led ladder on (deprecated
  diagnostic: …)` and pulses as before; nxboot's line ends in `ladder=deprecated`.
- `scripts/board-test.sh` and `docs/board/bpi-f3.md` read the LED that way.

## Gates (set before the cycle, from the step-2/3a traces)

| gate | expected | measured |
|---|---|---|
| `KINIT: milestone 11` | a few seconds (was 23 432 / 23 412 ms) | **1 716 ms** |
| `KINIT: milestone 1` | earlier by nxboot's hold + gap (~2.7 s; was 3 713 ms) | **988 ms** |
| `nxboot: boot led ok (…)` | no `ladder=deprecated` | `nxboot: boot led ok (gpio=0xd4019000 bank=3 line=0)` |
| `KINIT: boot led ladder on` | absent | absent |
| `board-test.sh --profile=board-visible` | `[PASS]` | `[PASS] board-visible: 24 rungs, 1 operator marker(s), no FAIL marker` |

The kernel now reaches its runtime 21.7 s sooner. What is left of the black before the splash is
everything before milestone 1 (988 ms on the counter: the boot ROM, the SPL, the SBI firmware,
the loader), the kernel's own 0.7 s, and the userspace bring-up up to gpud's first frame; the trace
has no clock after milestone 11, so that last part is unmeasured here.

## The flash: the vehicle sniffs gzip by one byte

The first write of this image stopped at the second user-area chunk:

```
Writing 'nxdisk1'      FAILED (remote: 'unzip gzip data fail')
```

The vendor flash vehicle inflates every download it takes for a gzip stream, and its check reads
only the header's method byte (offset 2, 8 = deflate) — not the magic. The chunk began at
sector 524 288, inside the data partition, with `44 e4 08 00`; of the four regions only that one
had an 8 at offset 2 (`nxdisk0` `00 00 00 00`, `nxgpt` `4e 45 58 55`, `nxboot0` `f0 14 07 b0`).
In the first 800 000 sectors of the disk, 337 of 60 746 non-zero sectors carry that byte (0.55 %);
the 64 sectors on either side of the boundary carried none.

Fix (`nx image flash-plan`, `tools/nx/src/commands/image_flash.rs`): no region starts on a sector
the vehicle would sniff. A chunk boundary moves back to the nearest one it would not (within one
MiB, else the plan is refused); the backup GPT's region grows back into the free space before it;
a sniffed sector 0 is refused. The corrected plan moved the boundary to sector 524 287 and was
written with `just board-flash --skip-stage` — the board still sat in the vehicle, no second
download-mode entry. Host proof: `tools/nx/tests/image_flash_cli.rs`
(`no_region_starts_where_the_vehicle_sniffs_gzip`,
`test_reject_a_boundary_the_vehicle_sniffs_all_the_way_back`).

## Files

- `board-boot-2026-10-04-led-flag.txt` — the board's trace of this cycle (image dev-a94fe976),
  with the operator's `board-visual: desktop`.

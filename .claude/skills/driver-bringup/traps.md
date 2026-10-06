# Traps (each with date and reference)

Every trap below cost at least one board cycle. Read them before the first cycle of a new
driver; add a new one the day it is won, with its reference, never from memory.

## QEMU hides what the board shows

- **Timer re-arm through the wrong path** (2026-09-28, `source/kernel/neuron/src/hal/platform.rs`
  `arm_timer_ticks`, CHANGELOG "Fixed - 2026-09-28"): the tick was armed through `stimecmp`
  but re-armed through SBI in the trap handler; QEMU's firmware folds the two, the board's does
  not — the kernel stopped 16–20 ms after init's spawn with no message. Found with the LED
  ladder and `sip` stamps, not with markers.
- **A QEMU literal as a bound** (2026-09-28, `hal/plic.rs` `MAX_IRQ`): 95 was QEMU virt's source
  count; the board's storage host is IRQ 101 of 159. Bounds come from the tree (`plic_ndev()`).
- **Sub-page device bases** (2026-09-28, `source/init/nexus-init/src/bootstrap/device_tree.rs`
  `window_of`): init refused a block whose registers start inside a page (the APMU at
  0xd4282800); a window is the pages the registers touch plus an in-page offset.
- **No `sfence.vma` after a PTE change** (2026-09-28, `mm/vm_ops.rs` `fence_asid`): QEMU's TLB
  forgave it; the board's first store into a DMA table faulted.
- **Console tearing across harts** (2026-09-29, `hal/console_line.rs`, `console_line.rs`,
  `docs/board/measurements/2026-09-29-platform-stage/`): the kernel's lock-free trap/fault
  printers interleaved with a service's marker BYTE by byte on four real harts; every grep in
  every proof lane read "missing". Each hart's line now leaves as one unit.
- **MTTCG is not a board either**: `ci-os-smp` (two harts, MTTCG) has been red on
  `KSELFTEST: ipc call budget FAIL` since at least 2026-09-23 on every attempt and is NOT in
  `test-all` (only `ci-os-smp1` is) — it is a tearing witness, not a gate
  (`docs/board/measurements/2026-09-29-platform-stage/README.md` §Proof).

## Reading the evidence wrong

- **"Missing" may mean "torn"** (2026-09-29, same folder): before assuming a lost IPC frame,
  search the transcript with a regex that allows one stray character between letters. The
  instrumented witnesses (`init: ctrl frame unknown`, `init: ctrl recv err`) fired nothing;
  the de-interleaved grep found every line.
- **The loader loop that would not come back** (2026-09-29,
  `docs/board/measurements/2026-09-29-emmc-after-linux/`): two cycles reproduced a
  `verify FAIL … nxbd` loop after the stock kernel, two later cycles with the same steps did
  not; the instrument (`nxboot: disk sdhci mode=`, the `nxbd-<why> head= reread=` verdict)
  stays and the fix waits for a trace — do not "fix" what the trace has not shown.
- **DRAM retention is a coin the reset flips** (2026-09-28, `2026-09-28-dram-retention/`):
  three cold cycles lost the console ring, one warm cycle kept it with bit errors — a rescue is
  a witness on this board, the trace on the disk is the proof.

## The stock system is not a lab bench

- **`dd if=/dev/mem` faults on MMIO; byte-wise mmap reads answer `0xffffffff`** (2026-09-29,
  `2026-09-29-display-regs/README.md` §Instrument): 32-bit loads through `ctypes` on the
  mmap.
- **`fb0/blank` is a layer blank, not a power-down** (2026-09-29, same README): 17 controller
  words and one APMU bit moved; the encoder and its clock stayed on.
- **Unbinding the vendor display driver oopses it** (2026-09-29,
  `2026-09-29-display-regs/oops-pinctrl-dram-fb.txt`): the system survives, the pipeline
  stays up, the domain's power-down protocol is not observable that way — nor by unplugging
  the cable (2026-09-30: only the HPD bit moves).
- **Pulling the microSD kills the stock system** (2026-09-29, `2026-09-29-emmc-after-linux/`):
  its root filesystem is on the card; `adb reboot` is not available, the reset button is the
  reboot.

## Our own assumptions

- **"No knobs in `/chosen`" is not "a board"** (2026-09-29, CHANGELOG "Fixed - 2026-09-29",
  `source/apps/selftest-client/src/os_lite/boot_cfg.rs` `Machine`): on QEMU an absent
  `nexus,boot-mode` IS the proof boot (RFC-0098 C2); the `smp1` lane ran the board ladder
  for one afternoon. The machine is decided by the tree's root `compatible`.
- **A profile name is a ladder key** (2026-09-30, `scripts/qemu-test.sh` `case "$PROFILE"`,
  CHANGELOG "Changed - 2026-09-30"): a new profile not in the list falls into the `full`
  ladder and is red on markers it never emits; and `visible` had skipped `verify-uart`
  entirely until its profile extended `headless`.
- **A layout constant has derivatives** (2026-09-30, `source/libs/nexus-display-proto/src/layout.rs`):
  the maximum lived in one place, its rows, offsets, strides and the surface ceiling in
  twelve; `mode.stride` must stay the layout pitch (`source/services/windowd/src/smoke.rs`
  `for_visible`) or the first frame write panics on the band buffer.
- **The board never drops a power domain while it runs**: the stock kernel keeps the display
  domain up, so no on/off diff exists. Before planning a cycle for it, read the stock tree
  from the pinned archive (`measurement-recipes.md` 3b): its power controller named the
  control and status bits of all nine domains (2026-09-30, `2026-09-30-power-domains/`); the
  transition itself is then judged on our chain from socd's before/after register words.
- **Board cycles are slow and not free**: a cycle is flash → LED → microSD → reset →
  `just board-log`; a USB-UART adapter would shorten every cycle more than any script
  (plan note 2026-09-29). Batch hypotheses so one cycle decides several.
- **The flash vehicle sniffs gzip by one byte** (2026-10-04, `2026-10-04-boot-led-flag/`):
  every download whose byte 2 is 8 is "inflated" and refused (`unzip gzip data fail`) — the
  check skips the magic, so it depends on the image's CONTENT and appears in one build, not
  the next. `nx image flash-plan` places region starts around it. A write that stops
  mid-plan leaves the board in the vehicle: re-plan, then `just board-flash --skip-stage
  --plan …` — no second download-mode entry.
- **Two working references do not make a working third** (2026-10-03, `2026-10-03-first-light/`):
  the composer's layer word came from the boot loader (layer 0 reading RDMA3, `7`), the RDMA
  channel from the stock kernel (RDMA1) — each source consistent, the mix scanned nothing. Take
  a coupled group of words from ONE source, or derive the coupling (`cmps_layer_word(rdma)`) and
  test it (`test_reject_a_layer_reading_a_channel_the_control_word_does_not_enable`).
- **pinctrl-single's "pin N" is a register index, not GPIO N** (2026-10-03): the status LED's
  pad was taken from debugfs `pin 96` (index 96, an unclaimed pad at 0x180); GPIO 96 is index
  120 at 0x1e0 — `pinmux-pins` names the claiming GPIO, the GPIO controller's `gpio-ranges` the
  mapping. The LED worked anyway (its real pad was already GPIO), which hid the error for a week.
- **A counter that ticks on the stock system may stand still on yours** (2026-10-03): the
  post-processing line counter moved with the stock kernel (it dithers) and never on the
  first-light path (no post-processing) — a single witness made a scanning controller look dead
  in the gate. Prove liveness with two independent witnesses.
- **A model built from your assumptions repeats your bugs** (2026-10-05, TASK-0328 U1): the xHCI
  model's hub descriptor followed the parser's own length rule, so the host tests agreed with a
  wrong parser — QEMU's real hub (10 bytes for 8 ports) refused it on the first lane run. Build a
  model's device data from the device (measured bytes, the emulator's source behaviour), never
  from the driver under test; the lane is the measurement, set its gates from what it printed.
- **A wait that makes a race disappear is hiding it** (2026-10-05): the mutant without the 100 ms
  attach debounce re-enumerated a hub — a root port queued by the scan AND by its power-change
  event; the debounce had only kept the first task waiting long enough to be deduplicated. Run the
  mutation suite with every timing wait removed: what still fails is a real ordering bug.
- **`qemu-xhci` numbers its USB 3 ports first** (2026-10-05): with four of each, a device on
  `bus=xhci.0,port=1` appears on xHCI root port 5. Read the Supported Protocol capabilities;
  never assume a port's revision from its number.
- **A gated block's window is not a window** (2026-10-05): granting the board's USB node before
  socd powered and clocked it would let the driver's first read stall the bus — a tree node joins
  its plane together with its glue, never ahead of it.

- **The board ladder's mirror check wants the rung whole** (2026-10-05): `board-test.sh` greps each
  ladder literal inside the manifest's declared markers — a rung with exact values (`xhcid:
  controller ok (version=1.10 …)`) is not covered by its prefix marker (`xhcid: controller ok (`);
  declare the full line (gated on the board profile) or the ladder fails before it judges.
- **A supply is glue, and it waits** (2026-10-05): an on-board hub's power lines are GPIO outputs
  with a measured start-up delay; the glue owner drives them through the bank's masked set
  registers (the loader and the kernel pulse the LED on the same bank — no read-modify-write of
  another writer's lines) and spends the delay on its kernel one-shot. A plan with a settle and no
  timer must fail (`step=settle`), never silently skip the wait.
- **A node without registers is not a bus device** (2026-10-05): `dtc` warns (`simple_bus_reg`)
  for a hub or regulator node under `/soc`; it belongs at the root, like a regulator.
- **A heap-less driver's state is a stack budget** (2026-10-05): xhcid's 22 KiB of device table
  and rings live on its stack, and a by-value move out of a bring-up function holds two copies;
  8 pages held until one more frame tipped it (`[USER-PF] STORE` just below the stack, the
  service exits with `reason=fault`). Measure `size_of` the state in a test, set `stack_pages`
  from it, and read a service exit with a stack-adjacent fault address as an overflow first.
- **`just board-image` packs, it does not build** (2026-10-05): it takes "the artifacts the
  last OS build left" — a cycle flashed after a code change without `just build-os-workspace`
  boots the previous OS volume and repeats the previous result (USB cycle 2 was cycle 1
  again). Always `just build-os-workspace && just board-image`; after flashing, `scripts/
  board-flash.sh --verify` only matches before the OS has booted (it writes its own regions).
- **Read the vendor driver before the tenth cycle, not after** (2026-10-05): nine USB cycles
  measured one glue word each; the vendor PHY and reset tables (names and bits only) named the
  gate in one reading — a PHY held in a reset whose polarity is the one inverted entry of its
  table (`0x3cc` bit 8: SET = held). A block that reads all zeros is in reset or unclocked;
  look for its reset before its clocks.
- **An armed layer that shows nothing: read the composer's z-order** (2026-10-06): the pointer
  layer was programmed, running (its state words appeared) and read back — under the desktop,
  which first light had put on composer layer 0. A lower layer index composites ABOVE a higher
  one on this controller; the stock desktop runs on layer 7 under the pointer's 6. Put the
  desktop where the stock puts it.
- **Not every word of a stock dump is glue** (2026-10-05): a running channel shows state
  words (`0xcc`, `0xe0..=0xfc`) that a fresh channel refuses to take (read back unchanged); the
  desktop's own channel holds neither. Compare the channel against a sibling channel before
  copying a word; write what a vendor driver writes, read back what a running block shows.
- **A channel has a composer row** (2026-10-06): the DMA channel's `+0x04` (`compsr_y_offset`)
  is the row of the composer its first line lands on — write it with the layer's top on every
  move, or the layer shows nothing (the vendor header named it; the dump showed it as `971`).
- **A forbidden marker must not fire on the path's own start** (2026-10-06): the first
  present carries windowd's software pointer before the overlay is armed (ADR-0034 restores
  that region after); refusing it whole stopped the desktop, and naming it tripped the
  forbidden gate on a correct boot. Skip and count before the arming, name it only after.
- **A human's wait is longer than a lane's** (2026-10-06): a 60 s bounded wait for the
  operator's first mouse move ran out during the SD swap and reset; the probe polls 180 s now.
- **Interrupt pipes run dry at the driver's wake interval, not the device's** (2026-10-05): four
  queued TRBs covered 4 ms; the board woke the driver every ~6 ms under load (`dry=44/s`,
  reports lost at the device, "the mouse moves very slowly"). Count the drains that find a
  pipe's every TRB complete and print them per second; eight TRBs covered it.
- **A byte-wise marker never reaches the board's trace** (2026-10-06): the kernel's
  `debug_putc` writes the raw UART only (`sys_debug_putc` → `uart::raw_writer()`), while
  `debug_write` goes through the console line funnel the eMMC trace records. On the desk with
  no UART adapter, a marker emitted byte by byte (`emit_bytes` + `emit_u64`) is on no trace at
  all — cycle 15's `SELFTEST: input usb hid ok (…)` — while the `FAIL` line of the cycle
  before, emitted whole, was. Build a marker line whole and emit it in one write; grep the
  selftest-client for `emit_bytes(` when a board marker goes missing.
- **A gate that reads stdin is no gate** (2026-10-06): `tools/deadcode-scan.sh` ran `rg` with
  no path — under a terminal it scans the tree, under a harness it reads stdin: inert in CI
  ("`just deadcode` can never fail", noted 2026-07-31) and hung for 80 minutes under a
  never-closing stdin. A scanner names its path; run a gate once with `< /dev/null` to see
  what it really does.
- **Read the static capabilities at start, not at run** (2026-10-06): the USB 3 root port's
  link can train before the driver's run phase; its change arrived with the ports' revisions
  still unread, and the SuperSpeed hub was enumerated as a USB 2 device. Anything an event
  handler consults must be known before the first event can arrive.

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


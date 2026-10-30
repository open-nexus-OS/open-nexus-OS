<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# ADR-0089: Lane choreography is declared, the guest opens the input window, every console line is whole

- Status: Accepted (2026-10-30, TASK-0333 6f-3 — test-all green; after the rules: 3/3 usb-visible,
  3/3 lowmem, 1/1 input-flood, every scenario step at its first attempt; board cycle 77)
- Date: 2026-10-29
- Links:
  - Tasks: `tasks/TASK-0333-memory-the-efficient-one-and-the-app-lifecycle-layer-6.md` (6f-3)
  - RFCs: `docs/rfcs/RFC-0100-…` (§5: the `lowmem` lane these lanes now run declaratively)
  - Related ADRs: `docs/adr/0027-…` (the selftest client's two axes; the proof manifest as SSOT),
    `docs/adr/0068-…` (the modal lane the injector first drove), `docs/adr/0086-…` (the kill
    chain whose event orders are proven here)
  - Code: `source/kernel/neuron/src/console_line.rs` (`TaskLines`),
    `source/libs/nexus-proof-manifest/src/scenario.rs`,
    `source/apps/selftest-client/proof-manifest/scenarios/`, `tools/qmp_scenario.py`,
    `tools/qmp_input.py`, `source/services/memd/tests/kill_chain.rs`,
    `source/drivers/storage/virtio-blk/src/inflight.rs`,
    `source/drivers/storage/virtio-blk/src/mmio/completion.rs`

## Context

6f-3's first test-all runs were red three times for three different reasons, none of them the
change under test:

1. **A torn marker.** `abilitymgr: kill (inst=` came out as `abilitymgr: execd: exit pid=117 …`
   and `kill (inst=3 …)`. abilitymgr wrote its line a byte at a time; the kernel gathers console
   bytes per HART (TASK-0327B: lines whole across harts), and a hart runs many tasks — a task
   switched out mid-line had the next task's line appended to its half. Sixteen crates still
   write lines in pieces.
2. **The test driver raced the guest.** The visible lanes' injector is a second actor on the
   host's clock: it greps UART text for incidental lines and sleeps fixed times. Its first pointer
   burst landed inside the guest's IPC benchmarks once (`ipc call budget FAIL (rt=90us)`, 47 µs in
   every other run — green runs had started ~70 UART lines after the benchmark, by the emulator's
   speed alone); a press sent while the guest recomposited after a kill never surfaced, and its
   retry travelled from an assumed position and pressed the clock. On the USB lanes three steps
   of its early phase failed in EVERY run and were logged and skipped ("continuing injection");
   long idle waits (`wait_settled` with no tap) hid ordering the lanes depended on.
3. **A gate measured the hart's load, not the path.** The IPC budget gated the MEAN over 64
   calls — wall time including everything else the one hart did meanwhile.

And one race in the product: the killed app's window closed before execd's death report, and
abilitymgr thawed a process the parent had already reaped. And one bound in the product that
turned a slow device into a dead one: the block driver gave a request up after a 2 s watchdog,
the request's slot stayed busy for the rest of the boot, and when the emulated device stalled
for seconds (the whole guest stalled with it: a frame went unacknowledged for 2.0 s just before)
four given-up requests left a device that answered everything later with `slots exhausted` — the
memd journal's write and the swap-out with it (2026-10-29).

Every one of these used to get a local patch — one more wait, one more marker grep. The class
needs rules.

## Decision

1. **Console lines are whole per TASK** (kernel). Userspace's console bytes (`debug_putc`,
   `debug_write`) gather per task (`TaskLines`: sixteen partial lines at a time, 256 bytes each)
   and enter the hart funnel only as whole lines; a longer line leaves in whole pieces, a task
   that finds no free slot writes through (torn rather than silent, never a wait), a task that
   dies mid-line has its piece ended at the death (`exit_task`). The fragment emitters need no
   migration to be correct; `nexus_abi::DebugLine` remains the cheap way to write a line (one
   syscall instead of one per byte) and abilitymgr and execd use it.
2. **The guest says when input may come.** The harness prints `SELFTEST: input window open` once
   its own work is behind it (the IPC budget in its phase, the console and BKL budgets at the end
   phase's start, the end-phase probes, its last one the UI smoke); the injector sends NOTHING
   before that line. The guest decides; the host no longer guesses which incidental line comes
   last.
3. **A timing gate judges the path, not the hart.** The IPC call budget judges the FASTEST of 64
   calls timed one by one (the clock's own cost taken off): a regression that puts a kernel entry
   back in the exchange raises every round, the rest of the hart's work only some; the mean is
   printed beside it, never judged.
4. **The choreography is data, every step acknowledged.** A lane's input is a scenario
   (`proof-manifest/scenarios/<name>.toml`, chosen by the profile's `QEMU_INPUT_SCENARIO`): steps
   of ONE action (`login`, `click`, `move`, `chord`, `key`, `type`, `drag`, `wait`) and the
   markers that acknowledge it within the step's bound: `expect` — each a new line after the
   action, in ANY order, because services write concurrently and only causality orders their
   lines (a close's `app window closed` from windowd and the app's `persist saved` have no order
   between them; a list that demanded one failed a green close, 2026-10-29) — and `then` — each
   after the LAST `expect` line, for what follows from them (a re-created surface's first frame
   only counts after its resize: the first surface's frame came before it). A `wait` step has no
   action of its own and counts from the action of the step before it (from its own start it
   missed a restore line that came while the relaunch before it was still being acknowledged,
   2026-10-29). A click is
   also acknowledged by its own readback — a HIT on the point (a miss there means the surface
   under it is not laid out yet: wait, press again) — and a drag by its drag trace; an action that writes no line (typing
   — the keystroke privacy rule) names the later step that covers it (`acked_by`); a pause with no
   event to wait for is declared (`settle_s`). `nexus-proof-manifest scenario` validates every
   scenario against the manifest — every literal a declared marker or a specialisation of one,
   every point and word named, every action acknowledged, a `then` only after an `expect`,
   bounds bounded — and hands it to ONE
   runner (`tools/qmp_scenario.py`), which retries a step as declared and otherwise ENDS the run
   at the first unacknowledged step, naming it and the missing marker. A click whose press never
   surfaced homes before it retries (its position is unknown). The display points and words live
   in `scenarios/targets.toml`, read by the injector AND by the shell's conformance tests — one
   source; a moved widget fails a host test. The lines the scenarios wait for are declared
   markers; a renamed one fails the host check, not a lane at minute nine.
5. **A proof lane's disk does not wait for the host's platter.** A proof session attaches its
   (per-run) disk image with `cache=unsafe`: the guest's FLUSH stays a no-op on the host instead of
   an `fdatasync` of the whole image, which on a busy, nearly full host disk stalled the guest's
   block requests for seconds (`virtio-blk: timeout` → `blkd: trace os FAIL (write)` → a failed
   app launch, 2026-10-27 and 2026-10-29). Writes still reach the host page cache in order —
   a reset lane stays in one QEMU process, the trace contract reads the image afterwards — and an
   interactive session keeps QEMU's default.
6. **A race in the product is closed by state and proven over every order.** An instance whose
   kill the lifecycle authority carried is `dying`: only its death report moves it. The kill
   chain is proven over the REAL pure state machines (memd's authority, abilitymgr's broker,
   band feed and kill policy) in every interleaving of the window's `gone`, the death report and
   stray sightings (128 orders; the test fails without the fix).
7. **A device's answer is an event, never a deadline** (virtio-blk). A request waits for the
   device's interrupt and for nothing else — no watchdog period, no poll, no bound: a stalled
   device only delays it. The line is bound when the device opens, before its first request (the
   warm-up read included); a device whose line cannot be bound is refused (`virtio-blk: no irq
   line`) instead of polled. A device that failed says so with its own event — a configuration
   interrupt with `DEVICE_NEEDS_RESET` in its status — and the request and every later one end in
   an error with one line. Slot ownership is a host-tested table (`inflight.rs`): a slot is the
   device's from submission until its chain comes back, a request whose wait failed stays the
   device's and only its late completion frees the slot — never the driver's timeout.
8. **After its ladder the harness is an observer.** From the input window on, selftest-client
   does no work of its own beside the system under test: its end phase sleeps on its declared
   timer until the next due look and on nothing once none is due (on a board: at once). The
   `yield_` spin it replaces kept the one hart busy for the whole lane — every task ran beside a
   harness that never slept, and under `-icount` the guest's clock ran on the spin's
   instructions instead of idling with the host's. What it still DOES there is declared and
   profile-gated: the memory drill on the QEMU visible lane (never on a board, forbidden on
   `lowmem`) and the `lowmem` hold — stimuli the memory chain's proofs need, armed by what the
   harness observes (memd's `STATUS`, looked at once a second while armed).
9. **What a running system writes is the system's, not the image's** (the board's flash
   read-back). The disk layout declares the partitions a running system writes
   (`storage::layout::LIVE`: `bsb`, `state`, `data`, `trace`); the flash plan carries their first
   bytes and gives every region a read-back digest with them zeroed; `board-flash.sh --verify`
   reads them as zero. One boot between the flash and the read-back used to fail it on exactly
   those three (board cycle 77) — a red that said nothing about the flash.

## Consequences

- **Positive**: a lane fails at the first unmet step with its name; the injector cannot disturb a
  timing gate; markers cannot tear whatever writes them; the choreography is reviewable data
  checked on the host; a widget, a marker or a word moves in one place; each step's duration
  is in the hypothesis log — the lanes measure their own choreography.
- **Negative / accepted cost**: user console output appears at its newline (a half line waits for
  the rest, or for the task's death); the kernel holds up to sixteen half lines (4 KiB); the
  scenario schema is one more contract to keep; the first key after boot is a step of its own —
  a chord as the first input got one empty report from QEMU's USB keyboard and nothing after it
  (lowmem, 2026-10-29), which the old choreography had masked by pressing a letter first; the
  cause is open for the keyboard package.
- **Accepted**: a device that hangs SILENTLY — no completion, no failure event — holds its
  request, and blkd's clients wait for their replies, as for any peer that never answers
  (`check-wait-not-poll.sh` rule 3: a reply is waited for until it arrives or the peer dies).
- **Not decided here**: per-event acknowledgements through the input chain (event ids in the hop
  markers — a protocol change and a UART line per event); masking interrupts around benchmarks
  (not what a service sees); the SD host core's and gpud's device watchdogs (the controller's own
  error interrupts can replace the SD host's — board storage, its own task); an asynchronous
  block server (requests submitted, replies sent on completion, up to the queue's depth); an
  event channel from memd to the harness, so the drill is armed by memd's band changes instead
  of a `STATUS` look once a second; the display-bootstrap observer (gated off since it drives
  windowd — it polls windowd's visible state every 16 ms) stays off.

## Alternatives considered

- **More waits in the imperative injector** — each found race became one more marker grep; the
  next race was found by the next red run. Rejected: the class, not the instance.
- **Move the benchmarks out of the ladder** — the budget would no longer be judged where it
  regresses; the input window keeps them in place.
- **Line-buffer in each service's emitter** (migrate all sixteen crates) — every new emitter could
  regress it; the kernel stage makes it a property of the system.
- **A scenario parser in the injector** (Python) — a second reader of the contract; the one
  validator is the manifest's.

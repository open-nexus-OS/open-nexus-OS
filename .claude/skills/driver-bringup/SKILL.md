---
name: driver-bringup
description: The driver / board / SoC-block bring-up playbook of this repo — measure on the stock system first, hypotheses with a decision criterion, design the gate, then code, then judge the gate against the measurement, then sharpen or drop the gate. Use when a new driver, a new board or a new SoC block is brought up, or when a board proof goes red for a reason QEMU never showed.
---

# Driver bring-up (measure → hypothesis → gate → code → gate → sharpen)

The method is Alyssa Rosenzweig's reverse-engineering discipline applied to our own
hardware path: the hardware and the stock system are the ground truth, every claim is a
measurement with a file in the repo, and a gate exists only because a measurement showed
what it must catch. Everything below was distilled from this repo's board work of
2026-09-22 … 2026-09-30 (TASK-0260/0260B, 0327B, 0246, 0245B, 0250/0251); each rule names
the example it was won on. Legal rule: no company or product names in code, comments or
docs — a device-tree `compatible` string is data and may be quoted.

## When to run

A new driver (a device our kernel has never granted), a new board (a tree we have never
booted), a new SoC block (a clock/reset/power domain socd has never written), or a board
lane red for something QEMU is green on. Skip for a fix inside a driver that already has
its measurement folder and its gate — use `verify` there.

## The order, as a rule

1. **Measure on the stock system** before any code: registers, clocks, resets, power
   domains, tree nodes, the buffers' addresses and reach — from the running vendor
   system over adb, read-only. Recipes: `measurement-recipes.md`. Protocol per folder:
   `docs/board/measurements/<date>-<topic>/README.md` with the four headings
   **Question → Instrument → Results → Verdict** (exemplars: `2026-09-29-display-regs/`,
   `2026-09-28-dram-retention/`, `2026-09-29-platform-stage/`).
2. **Hypotheses with a decision criterion** — write, before the cycle, which observation
   decides between them (`2026-09-29-platform-stage/README.md` §"Hypotheses the next
   trace decides": three hypotheses, two witnesses, one trace; the trace then showed a
   fourth cause — that is the method working, not failing).
3. **Design the gate** — the marker or check that will prove the driver, declared in the
   proof manifest (`source/apps/selftest-client/proof-manifest/markers/*.toml`) and
   REQUIRED in the harness (`scripts/qemu-test.sh` for QEMU, `scripts/board-test.sh` for
   the board). No `ok` marker without the behavior behind it (CLAUDE.md `no-fake-green`).
4. **Code** — host-first (a model with goldens from the measurement, e.g. the SDHCI
   behavioural model `source/drivers/storage/sdhci/model/`), then the OS build, then the
   board. Run the `architecture-review` lenses first: **scope** (files in, side quests
   parked), **invariant** (+ the `test_reject_*` that proves it), **contract** (the SSOT
   row / RFC / ADR extended).
5. **Judge the gate against the measurement** — the gate must be red on the measured
   failure and green on the measured success, in that order (`2026-09-29-platform-stage`:
   the lane was red before the fix, `[PASS] board-headless` after, with the transcript
   archived next to the README).
6. **Sharpen or drop the gate** — a gate that never fired since it went green is either
   sharpened to what the next failure would look like or removed with the retired-name
   gate keeping its vocabulary from returning (`gates.md`).

## The three lenses, applied to hardware

- **Architect:** which SSOT does the driver extend — the tree (`config/board/<board>/board.dts`
  + `scripts/check-board-goldens.sh`), the service topology (`source/libs/nexus-service-topology`,
  `scripts/check-slot-ssot.sh`), the SoC glue table (`source/libs/nexus-soc`), the layout
  (`source/libs/nexus-display-proto/src/layout.rs`)? A driver that keeps its own copy of any
  of these is a double structure (CLAUDE.md `architecture-boundaries`, memory rule "no dual
  structure").
- **Security:** MMIO is granted `USER|RW`, never exec; the driver receives windows from
  init's tree walk, never guesses an address (`source/init/nexus-init/src/bootstrap/device_tree.rs`);
  identity is `sender_service_id`; input sizes are bounded before parsing; a sensitive
  operation goes through policyd.
- **Pragmatist:** the smallest honest proof set — one measurement folder, one host model
  with goldens from it, one lane, one board cycle per hypothesis; everything else is a
  follow-up task in the ledger, not a side quest.

## What "done" means for a driver

The ledger template (`ledger-template.md`) ends with a Definition of Done that has four
lines: the measurement folder exists and answers its question; the host model reproduces
the measured registers; the lane (QEMU or board) is green on the REQUIRED ladder with no
tolerated red that lacks a reference; and the gate has been shown red once on a real
failure. A driver without the fourth line has a proof that was never tested.

## Files

- `measurement-recipes.md` — board-independent recipes (registers, clocks, tree, DRM,
  EDID, dump annotation, boot trace, LED ladder).
- `gates.md` — the gate library with provenance.
- `traps.md` — the traps, each with date and reference.
- `ledger-template.md` — the per-driver bring-up ledger.

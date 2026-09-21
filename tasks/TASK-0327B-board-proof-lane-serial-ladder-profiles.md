---
title: TASK-0327B Board proof lane: the marker ladder over the debug UART, `board-*` profiles, opt-in in `test-all`
status: Draft (seeded 2026-09-21 at Block 0 P0; rewritten to end state when Block 1 boots — it needs a booting board to prove anything)
owner: @devx @runtime
created: 2026-09-21
depends-on:
  - tasks/TASK-0327-board-developer-tooling-flash-serial-one-command.md
follow-up-tasks: []
links:
  - Execution order (Block 0 T3 / Block 1 B1.6): tasks/IMPLEMENTATION-ORDER.md
  - Playbook: CLAUDE.md
  - The harness this mirrors: scripts/qemu-test.sh, docs/testing/README.md, docs/testing/proof-manifest.md
  - Profiles SSOT: source/apps/selftest-client/proof-manifest/profiles/harness.toml
---

## Origin

B part of `TASK-0327`: the tooling lets a developer flash and watch the board; this ledger makes a
board boot a PROOF — the same marker manifest, read over the debug UART instead of QEMU's stdout.

## Context

`scripts/qemu-test.sh` boots a profile, records `uart.log`, and `nexus-proof-manifest verify-uart`
judges the marker ladder (expectations suppress surprise, REQUIREs live in the harness). A board log is
the same text on a serial port; nothing in the harness reads one today, and no profile names a board.

## Goal

`just board-test PROFILE=board-headless|board-visible`: flash (`TASK-0327`'s recipe) → reset (SBI
SRST from our OS when it is up; a manual power cycle otherwise, with the lane waiting on the serial
banner) → capture the serial log into `build/logs/board--<ts>/uart.log` → `verify-uart` with the board
profile → the QEMU-only proofs replaced by their board equivalents (the pixel proof by the display
controller's marker + `board-visual: desktop`; input by `board-visual: typed`). Profiles
`board-headless`/`board-visible` `extends` the QEMU ones (env: no QEMU args; `runner =
scripts/board-test.sh`). In `just test-all` only under `NEXUS_BOARD=1` — opt-in, never silent, and the
report says "board lane skipped" otherwise.

## Non-Goals

Automated power control (no relay), a second board, the 2-VM network lane (`TASK-0331`).

## Constraints / invariants

- One marker manifest for QEMU and board; a `board-visual:` marker is declared like any other and
  written only by `just board-ack` from a human at the monitor.
- Wait loops self-terminate (timeout cap + failure signatures: `PANIC`, `alloc_error`, `FAIL` gate).
- No fake success: a lane that never saw the serial banner fails, it does not time out green.

## Definition of Done

`[PASS] board-headless` and `[PASS] board-visible` on the desk board with Block 1's markers; the
profiles registered in `harness.toml`; `docs/testing/README.md` documents the lane and the opt-in.

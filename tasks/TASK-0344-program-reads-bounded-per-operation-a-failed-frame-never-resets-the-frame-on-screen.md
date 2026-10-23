---
title: TASK-0344 An app's program reads bounded per operation; a failed frame never resets the frame on screen — the keyboard and the hangs
status: Done 2026-10-22 (board cycle 63: no runtime `ERR`, no app crash, `abandoned=0`, the chat's heap flat while typing — `carve=0/0`, `heap steady`, no `alloc-fail`)
owner: @runtime @ui
created: 2026-10-22
depends-on: [TASK-0077C (the generation arena, ADR-0065)]
follow-up-tasks: [layer 8's closing (the operator 2026-10-22): the latency of a large CPU re-raster — the shell's control center 0.65 s, the theme switch 0.74 s, a tap into a text field 0.46 s (cycle 63); the frame arena's spill itself (memory lane, 6b); the allocator's free lists default-on for every service (TASK-0333 6b)]
links:
  - ADR: docs/adr/0065-app-host-frame-phase-allocates-from-a-generation-arena.md (amended: a failed frame is abandoned)
  - Measurement: docs/board/measurements/2026-10-22-program-reads/README.md (cycle 63's decision lines)
  - Cause: docs/board/measurements/2026-10-22-frame-pace/README.md (§ the keyboard and the hangs)
  - Before: tasks/TASK-0077C-dsl-v0_2c-runtime-long-session-large-data-contract.md (the `Malformed` defect recorded there)
---

## Context (board cycle 62)

The operator after cycle 62: "everything great except the keyboard and hangs". Measured:

- **The chat died while typing**, in cycle 61 and in cycle 62 alike —
  `[USER-PF] LOAD … stval=0xffffffdedededede` (the frame arena's poison) in
  `nexus_dsl_runtime::focus::subtree_is_secure::walk`, the focus walk of a tap into the text field
  (resolved through the linker map of a byte-identical rebuild of the shipped app-host). Before it
  `apphost: text commit ERR Malformed`; in settings four taps answered
  `apphost: tap dispatch ERR Malformed` — a tap that does nothing, which reads as a hang.
- **Why `Malformed`:** the runtime reads an app's program through ONE capnp reader for the app's
  whole life, and capnp's traversal budget (`TRAVERSAL_LIMIT_WORDS`, 4 MiB) belongs to the reader:
  every emit and every dispatch spends from it, nothing refills it (capnp 0.24's `ReadLimiter` only
  counts down). Once spent, every read of the program fails. Host: settings toggling its window
  menu fails after 28 toggles, the greeter's field after 482 keystrokes; with eight times the
  budget after 260 and 3 886 — linear in the budget. TASK-0077C's recorded defect ("settings
  `WinMenu` × 34 returns `Malformed` on a plain heap too") is this.
- **Why it then crashes:** a failed emit has already opened its arena generation — which resets the
  generation two back — and keeps the scene it failed to replace; a second failed frame resets the
  half that scene lives in while it is still shown, and the next tap's focus walk reads it. The
  layout region has the same hole: a layout that fails (the engine's box cap) keeps the boxes of a
  generation the next layout resets.
- **And the heap ran out** (a second session on the cycle 62 image, `board--2026-10-22T18-18-34`):
  the chat died of `alloc-fail … size=0x25000` with its 4 MiB heap full. Every keystroke re-lays
  out the whole chat page; its boxes need more than the frame arena's half, the rest spills to
  the base heap as blocks above the largest size class — and those were never reused:
  `carve=0/636928` every four layouts, ~155 KB a keystroke, dead after ~20. TASK-0333 6a had
  planned a bounded large-block list for the allocator (moved to 6b); the operator chose to pull
  it forward here (2026-10-22), no size raised.

## Goal

An app answers every tap and every keystroke however long it runs; a frame that fails leaves the
frame on screen intact and readable.

## Non-Goals

- The theme switch's full CPU raster per window (the second, shorter hang) — a follow-up.
- The frame arena's spill itself on the chat's large frames, and any size raise (the memory lane,
  6b) — its blocks come back now, the spill still costs base-heap allocations.
- A larger traversal budget: the bound is right, its lifetime was wrong.

## Architecture verdict (three lenses)

- **Scope** — in: `userspace/dsl/ir` (`read.rs`: `Program`, the validated bytes an app keeps;
  `Program::reader`, a fresh `ProgramReader` with its own budget per operation),
  `userspace/dsl/runtime` (`lib.rs`, `view.rs`, `persist.rs`: a reader per operation;
  `FrameScopeGuard::complete`, a dropped guard abandons), `source/libs/nexus-service-entry`
  (`generation.rs`: `Generations::abandon`; `os/frame_arena.rs`: `abandon_frame_generation`,
  `FrameGeneration::complete`, the abandoned count; `large.rs` + `os/global_alloc.rs` + `lib.rs`:
  the bounded large-block list under the existing `small-object-free-list` opt-in — app-host, vfsd,
  screencapd), `source/services/app-host` (`probe/mount.rs`
  the emit scope; `probe/scroll.rs` + `probe/interaction.rs` a failed layout keeps the boxes on
  screen; `probe/frame_arena.rs` `abandoned=`), the CLI's and the shell harness's reader call
  sites, `tests/dsl_apps_conformance` (settings back in the arena harness, a failed emit under the
  poisoning arena, a long session), ADR-0065. Out: the theme switch's raster, the spill, the
  budget's size.
- **Invariant** — one operation's reads stay bounded
  (`test_reject_one_operation_past_its_traversal_budget`); no session runs an app's program reads
  dry (`a_long_session_never_runs_the_program_reads_dry`: settings 1 000 menu toggles, the greeter
  5 000 keystrokes); a failed frame never resets the frame on screen
  (`test_reject_resetting_the_frame_on_screen_after_failed_frames` in `generation.rs`; the arena
  harness's duplicate-key emit under the poisoning arena); a spilling session's heap stops growing
  (`a_spilling_session_stops_growing_after_two_layouts`), a double free and a full list refused or
  counted, never corrupting (`test_reject_a_double_free`, `test_reject_growing_past_its_slots`).
- **Contract** — ADR-0065 amended (a frame that fails is abandoned: its generation discarded, the
  retained one kept); the IR's reading contract in `read.rs` (the budget bounds one operation). No
  wire, ABI or syscall change.

## Plan

1. **A reader per operation (IR, runtime).** `Program<'a>` holds the validated canonical bytes
   (`Program::from_canonical_bytes` validates once); `Program::reader()` wraps the same bytes in a
   fresh capnp reader — no copy, no allocation, a full budget. The runtime keeps the `Program`
   and every operation (an emit, a dispatch step, a persist pass, a lookup) reads through its own
   reader.
2. **A failed frame is abandoned (arena).** `Generations::abandon` ends a scope without keeping
   it: the next open flips onto the same half again, so the half holding the last frame that
   completed is never the next one reset. `FrameGeneration` and the runtime's `FrameScopeGuard`
   keep a generation only on `complete()`; a guard dropped on a failure path abandons it.
3. **app-host.** The emit scope closes or abandons by the runtime's word; a layout that fails is
   abandoned and the boxes on screen stay (`relayout_bounded` puts the taken ones back);
   `apphost: frame arena (… abandoned=)`.
4. **Large blocks come back (allocator).** Blocks above the largest class go to a bounded list in
   address order (32 free blocks): first fit, the rest split off, a freed block merged with the free
   neighbours it touches, a double free refused, a block with no slot dropped and counted; requests
   and frees rounded alike to 16 bytes; poisoned on free like the classes.
   `apphost: frame arena (… large=blocks/bytes lost=)`.
5. **Proof** — host tests (IR, generation rule, runtime, the arena harness with settings back and a
   failed emit under poison, the long session); `just check`, `just diag`; QEMU `headless` and
   `visible`; board cycle 63: the operator types in the chat, toggles the settings menus and taps
   on — no `ERR Malformed`, no `USER-PF`, `abandoned=0` or the failed frames named.

## Results

- **Built 2026-10-22 (board cycle 63 pending).** Host: `nexus-dsl-ir`
  (`every_operation_reads_the_program_with_its_own_budget`,
  `test_reject_one_operation_past_its_traversal_budget`), `nexus-service-entry`
  (`a_failed_frame_is_discarded_and_the_frame_on_screen_kept`,
  `test_reject_resetting_the_frame_on_screen_after_failed_frames`), `dsl_apps_conformance`
  (`a_long_session_never_runs_the_program_reads_dry`: settings 1 000 menu toggles, the greeter
  5 000 keystrokes — dead after 28 and 482 before; the poisoning harness with settings back and
  three failed emits in a row — under the old rule it aborts on a slice whose length is the
  poison), and the DSL, shell-harness and app-host suites: 445 tests green. `just check` and
  `just diag` green. QEMU `headless` and `visible` green (13/13 chain markers,
  `frame arena (… abandoned=0)` throughout, no `ERR Malformed`, the FAIL lines the run before had).
- **The large-block list (2026-10-22, the operator's choice after the second cycle-62 session's
  `alloc-fail`):** `large.rs` with 7 host tests (`a_spilling_session_stops_growing_after_two_layouts`
  replays the board's three spilled blocks per layout: the bump stops after two layouts;
  `test_reject_a_double_free`, `test_reject_growing_past_its_slots`); the chain fmt → 452 host
  tests → `just check` → `just diag` → QEMU `headless` (13/13) and `visible` → board image green;
  QEMU's arena line `large=3/33488 lost=0 carve=0/0`.
- **Board cycle 63 2026-10-22** (`docs/board/measurements/2026-10-22-program-reads/README.md`,
  build `dev-8693`): no runtime `ERR` in the session (cycle 62: 6), no app crash (only the
  self-test's two), every app stopped with `code=0`; `abandoned=0` throughout; the chat typed in
  (`spill=1`) held `base=674096` from layout 4 to 24 with `carve=0/0`, `lost=0`, `heap steady` 31
  times, no `alloc-fail` (the old image died at ~20 layouts). The app's answer while typing 17–21 ms
  on average, worst 123–152 ms. **Handed on:** where a large area is re-rastered on the CPU the
  answer takes long — the shell's control center 654 ms, the theme switch 742 ms, a tap into the
  text field 464 ms — the follow-up above. Settings' long session ran on the host (1 000 toggles),
  not on the board this cycle.

# Handoff — TASK-0059 / RFC-0058 (ShadowCache Heap Exhaustion Fix)

Date: 2026-05-21
Session: crash diagnosis → production-grade fix plan

## Crash Diagnosis

```
alloc-fail svc=windowd site=alloc size=0xdc8
heap_start=0x455d28 heap_end=0x4d5d28 (512KB)
cur_before=0x4d5a4f → 729 bytes free
```

**Root cause:** `to_vec()` in `compute_shadow_row` (os_lite.rs:1591) + `Vec<u8>` storage
in `ShadowCache` (cache.rs:26) allokieren pro unique Shadow-Row permanent auf dem
Bump-Allocator. ~500 unique Rows × ~800 bytes = ~400KB → Heap voll.

**Why tests passed:** QEMU ran with `DISPLAY_BACKEND=none` → rendering path never executed.
`write_current_frame()` / `compute_shadow_row()` were never called.

**RFC alignment:** This is exactly P1 "Shadow per Box" + P2 "Zero-Alloc 2D Blur" from
the Production-Grade Delta, now escalated to P0 (correctness bug, not just perf/quality).

## Fix Strategy

Three tightly coupled changes, implemented together:

1. **ShadowArena** — pre-allocated `[u8; 65536]` at init, bump-alloc within it per frame,
   reset at frame start. Replaces `Vec<u8>` / `to_vec()` pattern.

2. **Per-Box Caching** — cache key from `(box_id, rel_y, blur_r)` → `(box_id, blur_r)`.
   One cache entry per box instead of one per row. Reduces entries 30–60×.

3. **blur_separable_zero_alloc** — `blur_1d` variant that takes pre-allocated
   `row_scratch`/`col_scratch` buffers instead of allocating internally.

## What Gets Changed

| File | Change |
|------|--------|
| `userspace/ui/effects/src/cache.rs` | `ShadowCache` → Arena-basiert (`offset`/`len` statt `Vec<u8>`) |
| `userspace/ui/effects/src/blur.rs` | `blur_1d_zero_alloc()` + `blur_separable_zero_alloc()` (neu) |
| `source/services/windowd/src/os_lite.rs` | `compute_shadow_row` → 2-Phasen: Box-Render + Row-Blit |
| `source/services/windowd/src/os_lite.rs` | Neue Fields: `shadow_arena: [u8; 65536]`, `col_scratch: Vec<u8>` |
| `tests/ui_v4_host/` | Neue Tests für ShadowArena, per-box caching, zero-alloc blur |

## What Gets Deleted

| What | Why |
|------|-----|
| `to_vec()` call in `compute_shadow_row` (line 1590-1591) | Ersetzt durch Arena-Allokation |
| Per-row cache key generation | Ersetzt durch per-box key |
| `blur_row_horizontal` inline (line 1579-1585) | Ersetzt durch `blur_separable_zero_alloc` |
| `shadow_scratch` as render target (line 1570) | Ersetzt durch Arena-Slot |

## Test Plan (TDD)

1. **Unit: ShadowArena** — alloc, reset, overflow, multi-entry lifecycle
2. **Unit: blur_separable_zero_alloc** — golden comparison vs alloc-basierter blur_separable
3. **Unit: Per-box cache key** — determinismus, collision-freiheit
4. **Integration: compute_shadow_row_zero_alloc** — host test mit mock framebuffer
5. **Integration: damage_pipeline** — bestehende Tests müssen grün bleiben
6. **OS smoke: QEMU mit Display** — `alloc-fail` darf nicht mehr auftreten

## Resume Commands

```bash
# Host tests (existierende + neue)
cargo test -p ui_v4_host
cargo test -p windowd

# OS build check
cargo check -p windowd --target riscv64imac-unknown-none-elf --features os-lite
just dep-gate

# QEMU smoke (MIT Display — das hat vorher gecrasht)
RUN_UNTIL_MARKER=1 just test-os visible-bootstrap

# QEMU smoke (ohne Display — backwards compat)
QEMU_DISPLAY_BACKEND=none RUN_UNTIL_MARKER=1 bash scripts/qemu-test.sh --profile full
```

## Blockers

None. All dependencies exist. `nexus-effects` library already has `blur_separable` +
`blur_1d` logic; just needs zero-alloc variants. ShadowArena is a new struct with no
external deps.

## State

| Item | State |
|------|--------|
| Crash diagnosis | Done |
| State files updated | Done |
| Plan | Done |
| `to_vec()` removed from OS hot path | Done |
| ShadowArena struct + tests | Done |
| Alloc-fail prevention tests | Done |
| OS check (cross-compile) | Blocked (root-owned target files) |
| QEMU smoke | Pending |
| Per-box caching (follow-up) | Pending |

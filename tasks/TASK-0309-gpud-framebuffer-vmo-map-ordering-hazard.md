---
title: TASK-0309 gpud framebuffer VMO map — a VA-layout defect the boot order exposed
status: Partially fixed (2026-07-28) — VA overrun closed; a SECOND cause remains
owner: @ui
created: 2026-07-27
links:
  - Exposed by: tasks/TASK-0308-dsl-slots-and-window-scaffold.md
  - Evidence (fail): build/logs/headless--2026-07-28T08-29-57/uart.log
  - Evidence (pass): build/logs/headless--2026-07-28T08-39-51/uart.log
---

## Context

`just test-os` failed: gpud could not map the framebuffer VMO windowd hands
it, so the display chain never reached `gpud: chain G4 scanout ok` and the
chain-marker contract reported 1/9 missing. It passed on `2469cdd6` and failed
deterministically on the TASK-0308 tree — which changed no file under
`source/`.

The seed of this task guessed a bringup ORDERING hazard between gpud's
framebuffer VMO and execd's shared atlas VMO. **That guess was wrong.**
Instrumenting the failing call answered it in one boot.

## Root cause

```
gpud: attach map slot=0xc idx=0x1 va=0x24fd2000 off=0x02bd2000 len=0x02ee0000 vmolen=0x02ee0000
```

```
GPU_RESOURCE_STRIDE = 32 MiB
framebuffer         = 46.9 MiB   ← 1.46 × stride
```

`alloc_resource_va_index` handed out one slot index regardless of the
mapping's size, and the caller then looped over the resource's byte length
from `base + index * stride` — two independent calculations with nothing
relating them. Every framebuffer mapping therefore ran 14.9 MiB past its own
slot into the next one.

It only ever worked because the framebuffer happened to get slot 0, with
nothing above it yet. The moment anything claimed slot 0 first, the
framebuffer landed in slot 1, ran into slot 2, and the kernel refused the
remap 11.8 MiB in. The second failure in the same boot is the proof:
`resource map idx=2 va=0x24400000 off=0x0` — the next resource wanting the
base VA the framebuffer had already taken.

TASK-0308 only moved the allocation order (window-kit compiles into every
consumer, so payloads and the kernel image grew). It was the trigger, never
the defect.

The constant's own comment told the story:

> *32 MB per resource VA slot. The external framebuffer is now 1280×**6400**×4
> ≈ 31.3 MB … so the 16 MB stride would overflow into the next slot.*

The stride had already been raised once, from 16 to 32 MiB, for the same
reason. The framebuffer has since grown to 1280×**9600**. Same trap, second
lap.

## Fix

**Not** "raise the stride to 64 MiB" — that re-arms the trap for whenever the
plane layout or the atlas band grows again. Two changes instead, so the CLASS
is gone:

1. **Size-aware reservation.** `alloc_resource_va_index(byte_len)` computes
   `byte_len.div_ceil(GPU_RESOURCE_STRIDE)` slots and reserves them
   consecutively. A resource bigger than one slot now owns the slots it
   actually spans.
2. **The reservation is binding.** It returns a `VaWindow { index, base_va,
   len }` and callers get their address from `window.page_va(offset)`, which
   REFUSES an offset outside the window. The caller can no longer compute
   `base + index * stride` on its own — both constants are gone from
   `attach.rs`, where they became dead imports.

Two real errors replace one silent overrun, both `GfxError::ResourceExhausted`
and both with numbers, before any page is touched:

- `gpud: va region full idx=… need=… max=… len=…` — the region has no room.
- `gpud: va window overrun idx=… base=… win=… off=…` — a mapping wants past
  its own reservation.

## Why it was undiagnosable

Two layers of discarded information on the boot's most load-bearing mapping:

- `nexus_abi::vmo_map_page` collapses every kernel rejection into
  `IpcError::Unsupported` INSIDE the wrapper. `vmo_map_page_sys` — which
  virtio-blk already uses — keeps the code. gpud called the lossy one.
- gpud then wrapped that in `.map_err(|_e| …)` and emitted a static marker.

So the log said `gpud: resource vmo_map_page fail` and nothing else: not which
page, not which address, not which error. gpud had no numeric UART output at
all; every marker was a fixed string.

`source/libs/**` was NOT touched — the right function already existed, gpud
called the wrong one.

## What landed

| File | Change |
|---|---|
| `source/drivers/gpud/src/diag.rs` | NEW — `kv_line`/`err_line` over `debug_putc` (no `format!`, no allocation), gated to the real OS build. One-shot failure paths only. |
| `source/drivers/gpud/src/backend/resources.rs` | `MAX_RESOURCE_VA_SLOTS`, `VaWindow` + `page_va` bounds check, size-aware `alloc_resource_va_index` |
| `source/drivers/gpud/src/backend/attach.rs` | maps through the window; `vmo_map_page_sys`; numeric failure log |
| `source/drivers/gpud/src/backend/transport.rs` | same for the internal resource path; stride comment corrected |
| `source/drivers/gpud/src/lib.rs` | `pub mod diag` |

`abi_error_name` is deliberately EXHAUSTIVE with no `_` arm. Its first cut
folded five variants into `"other"` and the boot log duly said `err=other` —
a longer way of saying nothing. A new `AbiError` variant must now break the
build so somebody names it.

## Proof — and what it does NOT prove

```bash
RUN_UNTIL_MARKER=1 RUN_TIMEOUT=240s just test-os
```

Before the fix: **3 of 3** consecutive runs red, all missing `G4 scanout ok`.
After: **3 of 4** green (three headless + one interactive `just start`).

That is a real improvement and NOT a fix. One run still failed at the same
address with the same shape:

```
gpud: attach map err=invalid-argument
gpud: attach map … va=0x24fd2000 off=0x02bd2000 len=0x02ee0000 vmolen=0x02ee0000
```

## The second cause (open)

The offset is INSIDE the reservation now — `page_va` let it through, so the VA
overrun is genuinely closed. The kernel is refusing the page itself, with
`InvalidArgument`, ~45.9 MiB into a 46.9 MiB VMO that windowd created and
handed over. `vmolen` says the VMO is that big; the map says its later pages
are not usable.

That points at the FRAMEBUFFER VMO's backing, not at gpud's address space —
a different owner (windowd), a different allocator (the kernel VMO arena), and
an intermittency that tracks how much arena was consumed before windowd
created it. Same symptom, second cause.

Next: log the VMO's committed extent at creation time in windowd and compare
against what gpud can map. If the arena is handing out a VMO whose later pages
are not backed, `vmo_create` succeeding is itself the lie.

## Honest limits on the attribution

- The green runs carry BOTH the size-aware reservation and the window guard;
  neither was isolated. The reservation is what makes the mapping fit, the
  guard is what makes a future violation loud.
- The intermediate run that failed WITH size-aware allocation in the tree may
  have used a stale gpud binary (cargo reported cached builds throughout). Not
  isolated either.

## Separate, pre-existing, NOT fixed here

- `SELFTEST: metrics {security rejects, counters, gauges, histograms} FAIL`
  and `SELFTEST: tracing spans FAIL` appear identically in the passing
  baseline and are tolerated by the lane. Worth their own look.
- `SELFTEST: ime v2 candidates` is flaky — it flipped FAIL → ok between two
  runs of the same tree.

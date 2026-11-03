<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# ADR-0093: Tables grow, bounded by the pool, never by a constant — reserving costs nothing, touching costs pages

- Status: Accepted (2026-11-03, TASK-0352 — board cycle `board--2026-11-03T17-28-06`)
- Date: 2026-11-03
- Links:
  - Tasks: `tasks/TASK-0352-the-memory-model-used-no-fixed-process-tables-an-owner-s-footprint-includes-its.md`
  - RFCs: RFC-0100 (the memory object model v2: commit on touch, one pool, pressure as an
    event), RFC-0085 (kernel-owned VA — its window bounds are amended by TASK-0352 P9)
  - Related ADRs: ADR-0091 (the kernel bounds what a caller sizes: `ENOMEM`, never a panic),
    ADR-0077 (lazy objects), ADR-0086–0088 (bands, squeeze, swap)

## Context

Layer 6 made memory lazy (RFC-0100): pages are committed on touch, a VMO dies with its last
capability, the GPU shares the one pool. What stayed were the tables sized by a constant: the
kernel heap (8 MiB `.bss`), a lazy object's page map (one u32 per reserved page, dense — a 1 GiB
reservation costs 1 MiB of kernel heap before a byte is touched), 64 regions per address space,
256 capabilities per task, a 768 MiB user mapping window, a service's static heap chosen by a
`heap-*` cargo feature, a 512 KiB app payload, and the app-count tables of abilitymgr (32
instances), memd (32 banded processes, 8 GPU charge clients). Each is a ceiling a product
decision never set: the 33rd app is refused, the ninth app's surfaces go uncharged, a settings
container at 93 % of its payload bound was the reason the bound was last raised. The measured
state is in TASK-0352 § Measured.

## Decision

- **No table of the memory model is bounded by a constant.** A kernel table grows with its use
  and is bounded by the frame pool (and `kheap::RESERVE`, ADR-0091) or by a per-process ceiling
  that is a security bound (a DoS limit, named as such), never by a size somebody guessed. A
  service table grows with the apps; the apps' number is bounded by memory alone.
- **Reserving costs nothing, touching costs pages.** A lazy object's page map is sparse: its
  cost scales with the pages touched, not the range reserved. An app's heap is one 64 MiB region
  of its image, which the kernel maps as a lazy object (ADR-0077) — no `heap-*` feature sizes
  it, a page costs the pool once touched; its payload object is lazy and 16 MiB long, so a
  payload is sized by its bundle. A process's mapping window is the whole user half.
- **An owner's footprint includes its surfaces.** The lifecycle authority's band push names each
  process's OWNER (the app's kernel service id, `app:<bundle id>`; memd wire v2), so memd joins
  gpud's surface charge — keyed by that id — to the processes it is for.
- **A limit per app, refused by name.** Two today: an app's instances (64 by default; the table
  itself has no bound) refused by the lifecycle authority, and an owner's surface objects —
  the GPU's ceiling less the compositor's eighth — refused by gpud. Each refusal names the app,
  its figure and its limit; every other app keeps running. The per-process endpoint count (1024)
  stays as a DoS valve, named as such.
- **Every growth is fallible and answered by name.** Growth past the pool is `ENOMEM`, past a
  ceiling `ENOSPC`/`STATUS_FULL`, logged once by the authority that refused — never a panic,
  never silent (ADR-0091's rule extended from "what a caller sizes" to "what a table grows by").
- **Out of scope**: the pressure order itself (RFC-0100's), swap and compressor policy, SMP, the
  GPU paths (package 3), capability-in-flight refcounts and pid reuse (follow-ups of TASK-0347).

## Built (2026-11-03, TASK-0352)

| Table | Was | Now | Refusal |
|---|---|---|---|
| kernel heap | 8 MiB `.bss` | arena 0 = the image's 8 MiB; grows from the frame pool in doubling blocks up to 32 arenas (`KHEAP: grew`) | `KHEAP: growth refused`, the ask answers `ENOMEM` |
| lazy page map | one u32 per reserved page | chunks of 1024 words, a chunk on the first touch inside it, freed when empty (`lazy_words`) | `CommitError::NoHeap` → `ENOMEM` |
| regions per address space | 64 embedded | `Vec`, doubling from 16 under `kheap::admits` | `VaError::TableFull` → `ENOSPC` |
| capabilities per task | 256, never grown | doubling from the first 256; `allocate_high` keeps the top | `CapError::NoSpace` |
| VMO object table / IPC endpoint table | 1024 / 384 | grow under `kheap::admits`; 1024 endpoints per owner stay as the DoS valve | `ENOMEM` / `NoSpace` |
| user mapping window | 768 MiB to 0x8000_0000 | the Sv39 user half to 0x40_0000_0000 | — |
| service heap | `heap-*` feature, 384 KiB … 16 MiB | one 64 MiB `.bss`, lazily committed | the allocation fails by name (`alloc-fail svc=`) |
| app payload | 512 KiB VMO | a 16 MiB lazy object (`payload_vmo::VMO_LEN`), written pages only | the header's length past `MAX_LEN` is malformed |
| abilitymgr instances | 32 | `BTreeMap`; 64 per app | `abilitymgr: launch refused (app= instances= limit=)` |
| memd bands / GPU charge rows | 32 / 8 | `Vec`; each band entry names its owner | `memd: band dropped (pid=; no heap)` |
| gpud surfaces per owner | the GPU's ceiling only | the ceiling less the compositor's eighth per owner | `gpud: FAIL surface alloc (owner= past its budget` |

## Consequences

- **Positive**: the app count, the window count and an app's heap are bounded by memory, measured
  by memd, instead of by eleven unrelated constants; a reservation is free, so sparse address-space
  use (guard pages, large lazy arenas) costs nothing; the kernel heap's ceiling is no longer the
  first table to fail on the board.
- **Negative / accepted cost**: a sparse page map is a two-level structure (an indirection per
  page lookup on the fault path); grown tables churn the kernel heap where embedded arrays did not
  (measured per phase, the frame budget is the gate); every refusal gains a line.
- **Follow-ups**: the per-app limit from a policyd-granted manifest claim (deny-by-default: the
  band's default until granted) — TASK-0352 P3 names the default, the claim is a later phase;
  RFC-0085's window amendment (P9).

## Alternatives considered

- **Raise the constants** (as done for the capability table 128 → 256, the payload 256 K → 512 K):
  each raise is a new guess and a new ceiling; rejected.
- **A kernel heap sized by RAM at boot** (RAM/32, clamped): a bigger fixed table, still a
  ceiling, frames taken whether used or not; rejected in favour of growth from the pool, with
  the measurement (P0) deciding how much growth the heap needs once the page maps are sparse.
- **A separate kernel VA window for the heap, mapped on demand**: Sv39's kernel half is the
  direct map; there is no free kernel VA for a second window; rejected until Sv48.

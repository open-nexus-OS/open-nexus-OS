<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# ADR-0087: A frozen app's private memory is squeezed on its parent's order — the records live with the object, a touch brings the page back, and a frame goes only after every hart forgot it

- Status: Accepted (2026-10-28; the proof is both QEMU lanes and board cycle 75 of TASK-0333 6f-1 —
  `KSELFTEST: task squeeze ok (pages=4 zero=1 kept=1 … reject=stranger,running,shared,device)` on
  every lane and the board, `SELFTEST: exec child squeeze ok (…)` with the child's pattern whole
  after the thaw (45 µs a page on the board's core), and on the usb-visible lane the drill's chain
  `execd: squeeze` → `abilitymgr: squeeze` → `memd: squeeze` → `SELFTEST: mem squeeze ok
  (pages=450 net=333 ratio=25%)` on an app-host frozen by the lifecycle phase, thawed and working)
- Date: 2026-10-28
- Links:
  - Tasks: `tasks/TASK-0333-memory-the-efficient-one-and-the-app-lifecycle-layer-6.md` (6f-1)
  - RFCs: `docs/rfcs/RFC-0100-memory-object-model-v2-commit-on-touch-unified-gpu-memory-pressure.md`
    (§5 the backing stages — this decision is its pool), `docs/rfcs/RFC-0111-…` (the parent
    orders its process whole — a squeeze is one more order of that kind)
  - Related ADRs: `docs/adr/0077-…` (memory committed on first touch — the page a squeeze takes
    is a lazy object's page, and the fault that commits it is the one that brings it back),
    `docs/adr/0082-…` (a parent orders its process whole), `docs/adr/0086-…` (memd is the one
    memory authority — it asks for the squeeze as it asks for the kill)

## Context

RFC-0100 §5 orders the backing stages: purgeable pages dropped, clean file pages dropped,
anonymous dirty pages compressed into the kernel's pool, compressed pages written to swap, the
kill. 6f-0 measured the codec where it runs (board cycle 74): 47 % on the kernel image's machine
code, 49 µs to compress and 16 µs to inflate a page on the board's core. The census of the same
board says what a squeeze can reach: each app-host holds 7.0–8.8 MB resident, of which 6.68 MB
is the shared image's text and read-only data (mapped by every app-host, clean, never
compressed); the private rest — 0.3–2.2 MB — is the app's `.bss` (its heap: the allocator's
bump arena lives there, 8 MiB reserved) and its stack, both lazy objects since ADR-0077, plus
one eager page of small data. A kill frees exactly that private part; a squeeze can free most of
it and keep the app.

Three questions had to be decided before the pool exists: where a compressed page lives (and
who frees it), who may squeeze whom, and how a squeezed page comes back without a stale
translation ever reaching a frame that went back to the pool.

## Decision

- **A squeeze is the parent's order, on a parked child, over its private lazy pages.** Syscall
  `task_squeeze` (69) takes `(pid, from_va, out_ptr, out_len)`: the caller must be the child's
  parent (`ESRCH` otherwise — the authority of every process order, ADR-0082), and every task of
  the child's process must be frozen and off every hart (`EBUSY` otherwise — a deferred freeze
  that has not reached its hart yet is not parked). An object is squeezed only when it is the
  child's alone: lazy, not sealed, never told to a device, not unlocked for purging, mapped by
  exactly one region of the child's space and by no other space, and named by no capability
  outside the child's process (the rule is `vmo_lifetime::squeeze_allowed`, host-tested). The
  shared image, a surface another process maps, a device's buffer are never touched.
- **One call is one bounded batch.** At most 16 committed pages are examined and 4096 page
  entries scanned per call; the call answers the next address to pass (`0`: the walk reached the
  end) and writes the batch's record (`squeezed`, `zero`, `kept`, `taken`, `stored`). The parent
  loops; the kernel lock is never held for a whole process (16 pages ≈ 0.8 ms on the board).
  Per page: a page of zeroes is uncommitted (its frame goes back; the next touch commits a zeroed
  frame — a zero page is a fact, not a record); a page whose block is at most 7/8 of a page is
  recorded; any other page is kept as it is, mapped and untouched.
- **The records live with the object.** Each lazy object that holds a squeezed page owns a
  store: 64-byte slots, one occupancy word per store page, a record is a 2-byte length and the
  block (at most 56 slots, spanning at most two store pages), placed first-fit from the lowest
  store page with room so the holes a thaw leaves are reused. A store page whose last record
  went back returns its frame at once; the object's death or purge returns all of them. The
  page table of the object says, per page, absent / a frame / a record — one word, the record
  tagged, so no path can mistake a squeezed page for an untouched one.
- **A touch brings the page back, synchronously.** Every path that commits a lazy page — the
  user fault, the kernel's checked user range, `vmo_write`/`vmo_read`, a device's runs — inflates
  a squeezed one: a fresh frame (inside the object's DMA reach), the record copied out under the
  table lock, decoded outside it (16 µs measured), the decoded length checked to be exactly one
  page — a record that decodes to anything else fails the access as damaged and stays — then the
  entry switched and the record released.
- **A frame goes only after every hart forgot it.** A batch clears the PTEs of the pages it
  changed, runs one TLB shootdown with the kernel lock held (the form the memory paths already
  use; idle harts flush at their next dispatch), and only then returns the frames to the pool.
- **The order travels the kill's road.** memd asks at `critical` before any kill: the process
  longest in the suspended band that was not squeezed since its last band change; a band change
  makes it a candidate again; `emergency` kills at once. `OP_MEM_SQUEEZE {pid}` (8) to
  abilitymgr, taken from memd's kernel sender id alone and refused unless the instance is
  suspended; abilitymgr orders the parent, `OP_SQUEEZE {pid}` (7) to execd, taken from
  abilitymgr's sender id alone and refused unless the pid is a live child; execd runs the
  batches and answers the totals, which travel back up. The exchange is waited (no clock): one
  app's squeeze holds memd and abilitymgr for its duration, never the compositor or the GPU.
- **Out of this decision:** compacting a store (its holes are reused; an empty store page goes
  back), the clean pages of the shared image (dropped, never compressed — a later stage),
  keeping a record after its page came back (a write would have to be tracked), squeezing a
  service (only apps the lifecycle authority froze are asked for), the swap stage (6f-2).

## Consequences

- **Positive**: memory pressure has a step between "drop what can be rebuilt" and "end an app"
  that the user never sees — the app comes back where it was, its pages one fault each; a kill
  of a squeezed app still frees everything (the store dies with its objects, never shared across
  objects); the counts are in `mm_stats` v5 (`squeezed_pages`, `store_frames`, `store_bytes`,
  `inflated_pages`).
- **Negative / accepted cost**: a lazy object's page entry carries a tag bit, so a frame number
  must fit 31 bits (8 TiB of physical memory); a fault on a squeezed page costs an inflate
  (16 µs on the board) instead of a zeroing; the first touches of a thawed app are slower until
  its working set is back; memd and abilitymgr wait for one app's squeeze.
- **Follow-ups**: the swap stage moves cold records out of the pool (6f-2, the page-in protocol
  on the pressure endpoint); the `lowmem` lane gives the kernel-sourced path a real `critical`
  (6f-3); the board measures the squeeze of a real app once real pressure exists.

## Alternatives considered

- **One pool for every object** (a global compressed store). Rejected: records of different apps
  would share store pages, so a kill would free records but not the pages they share with
  another app's — the stage before the kill would make the kill free less.
- **The kernel squeezes on its own at a watermark.** Rejected: the kernel does not know which
  process the user will come back to; the bands are memd's (ADR-0086) and the order is the
  parent's (ADR-0082). The kernel executes, it does not choose.
- **One call for the whole process.** Rejected: 7 MB of private memory at 49 µs a page is
  ≈ 90 ms with the kernel lock held — every hart's syscalls would wait.
- **Defer the TLB flush to the next activation of the child's address space.** Rejected for now:
  a second flush mechanism beside the shootdown; one shootdown per 16 pages costs little and the
  existing path is the proven one.

---
schema: 1
title: "Memory without the kernel lock"
role: reference
subsystem: kernel
sources: [source/kernel/neuron/src/mm/populate.rs, source/kernel/neuron/src/core/trap/fault_entry.rs, source/kernel/neuron/src/mm/frames/magazine.rs, source/kernel/neuron/src/mm/space_regions.rs, source/kernel/neuron/src/mm/vmo/mapping.rs, source/kernel/neuron/src/cap/space_maps.rs, source/kernel/neuron/src/mm/hart_spaces.rs, source/kernel/neuron/src/core/smp/tlb.rs, source/kernel/neuron/src/syscall/api/vm_map.rs, source/kernel/neuron/src/syscall/api/vmo_runs.rs, source/kernel/neuron/src/syscall/api/vmo_copy.rs, source/kernel/neuron/src/syscall/api/pinned.rs, source/kernel/neuron/src/mm/reclaim.rs]
contracts: [ADR-0103, RFC-0102, RFC-0100]
verified: 2026-11-25
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Memory without the kernel lock

> **Purpose** — which lock a page fault, a mapping, an unmap, a device's runs and a copy through an object hold, what keeps an object alive under them, and which harts an unmap reaches.
> **Read first** — [README.md](README.md) (how a syscall enters), [hart-spaces.md](hart-spaces.md).
> **Contract** — ADR-0103; the ranks are RFC-0102's (address space 3, memory object 4, a frame magazine 8).

## Who holds what

| path | holds | never |
|---|---|---|
| a user page fault (`populate::fill_free`, asked first by `trap/fault_entry.rs`) | its own space for the look, **nothing** while it takes and zeroes the frame, its own space for the install — which happens only if the region is still what the look saw | a second space; the kernel lock (a violation, a squeezed or swapped page, an empty pool go to the locked path, which redoes the fault whole) |
| `vm_map`, `vmo_runs` | the object's capability for the call (`pinned::pin`: a second, counted one), then the caller's space | the kernel lock |
| `vmo_write`, `vmo_read` (`vmo_copy`) | the object's capability and its count for the copy's length; **no lock** while bytes move — the caller's buffer is translated (`mm::user_copy`), a page of it not ready is filled as its fault would (`Ipc::fill`); four pages a stretch, the shootdown mailbox read between | the kernel lock (only what the free fill refuses re-runs under it) |
| `vm_unmap` (`vm_ops::unmap_range`) | the space for the clear, **nothing** across the shootdown, the space for the forget | a lock across the wait for another hart |
| a squeeze batch (`squeeze::run`) | the child's space for the walk; shootdown and frees after it | the same |

Single frames come from a magazine per hart (sixteen, refilled under the pool's lock); a free
goes straight to the pool, so a double free is refused where it happens.

## A region and its object

- **Going.** An unmap marks its region (`VaSpace::begin_unmap`): still recorded, filled by no
  fault, refused to a second unmap, its address not reused before the shootdown landed.
- **Counted.** A memory object counts the address spaces that map it, and those that hold a
  writable region of it, in its header. A space holds one token per object (`cap::SpaceMaps`);
  every write to a region table goes through `mm/space_regions.rs` — count, then record;
  forget, then give back. The token is made in the hold of the object table that checks the
  object's state (`vmo::mapping`): an object unlocked for purging is mapped by nobody, a
  sealed one by no writer. The reclaim, the destroy, the seal, the purge unlock and the
  squeeze read the counts; no path asks every address space.
- **Born held, held for a call.** `vmo::create` and the adoptions answer the id with the
  object's first capability (`vmo::Born`); nothing mints one from an id. A syscall that names
  an object by a slot holds a second capability for the call, and its drop judges the object
  (`reclaim::judge`): an object nobody holds and nobody maps is garbage for any judge, in one
  hold of the table. The kernel keeps the boot image's capabilities for its whole life.
- **A copy counts.** `vmo_write` and `vmo_read` take the object's count through the mapping's
  door for their length — a write as a writer. A seal, an unlock for purging and a destroy
  are refused under a copy in flight; a sealed object refuses a write, an unlocked one both.
- **Named for a device.** `vmo_runs` tells a device the runs only of an object claimed for it
  in one hold of the table (`vmo::name_for_device`): not sealed, not unlocked, and neither
  from then on.

## One space, the harts that hold it

An unmap in one address space calls `shootdown_of(raw)` and reaches only the harts that may
hold a translation of that space; the recycling of the ASIDs keeps `shootdown_all`.

| | |
|---|---|
| the set | one word per space in `HartSpaces` (`mm/hart_spaces.rs`), a bit per CPU |
| a hart enters it | itself, before it can load a translation: in `enter_space` before its `satp` names the space, and before a copy through the space's tables without a lock (`Ipc::read`/`write`) |
| a hart leaves it | itself, after a full flush that finds it on another space — the responder's, when its mailbox names that one space (`tlb_space`; requests for two spaces pending at once name none), or the initiator's own |
| why none is missed | the unmap clears its entries, then reads the set; the hart sets its bit, then reads the tables — a full barrier on both sides, so one sees the other |
| a new space under an old handle | starts with an empty set: its ASID is fresh, or was recycled behind a flush of every hart |

## Proof and numbers

Markers: `KSELFTEST: tlb shootdown targeted ok (harts= ran= reached= left= spared=)` — a space
no hart read through reaches nobody, the one hart recorded is reached, acknowledges and leaves
the set; `KSELFTEST: exec image reject ok (…)` — the seal refuses a writable mapper, a sealed
object a writer; `KSELFTEST: vm map reject ok`; `KSELFTEST: vmo purge ok (… reject=…,mapped)`;
`KSELFTEST: vmo copy ok (reject=seal,unlock,destroy,sealed,unlocked)` — a copy in flight against
the three, and the two states against a copy.
Host: `cap/tests_maps.rs`, `va_space_tests.rs`, `mm/hart_spaces.rs`.

The boot-end fence's lines (twice: the boot window, the steady window):

| line | reads as |
|---|---|
| `KINIT: mm faults (free= locked= longest_free_us=)` | faults filled with the thread's own space held, those handed to the kernel lock, the longest fill without it |
| `KINIT: mm magazines (harts= held= served= refills= flushed=)` | frames served from the magazines against the refills; `flushed`: a dry pool took them back |
| `KINIT: tlb shootdowns (all= space= reached=)` | shootdowns of every hart, for one space, and the harts the latter reached |
| `KINIT: bkl mm calls (map=us/n unmap=us/n runs=us/n)` | what the three calls still took of the kernel lock: zero in a boot with no order pending |
| `KINIT: tlb slow ack hart= … sent= soft_traps= in_loop= activity=` | a shootdown's round (100 ms) ended without that hart's acknowledgement (the first few) |
| `KINIT: long call <us> cpu<n> nr=<syscall>` | a syscall served before the kernel lock ran above 10 ms (the first few) |
| `SELFTEST: fault scale (threads= pages= one_us= all_us= x10=)` | fresh pages touched by one thread, then by one per CPU; `x10` the rates' factor in tenths — the board's number (gate: ≥ 60 with eight), an emulation shares host cores |

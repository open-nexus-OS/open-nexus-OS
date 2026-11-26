<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# ADR-0103: A fault holds its own address space and nothing else — a shootdown reaches the harts that hold the space

- Status: Proposed (2026-11-24, TASK-0330 P3; decisions 8 and 9: 2026-11-25, P4)
- Date: 2026-11-24
- Links:
  - Tasks: `tasks/TASK-0330-smp-v3-eight-harts-fdt-topology-bkl-decomposition-per-asid-shootdown.md` (P3)
  - RFCs: `docs/rfcs/RFC-0102-smp-lock-decomposition-lock-classes-ranks-and-budgets.md` (rank 3 the address space, rank 4 the memory object, rank 8 a frame magazine), `docs/rfcs/RFC-0100-memory-object-model-v2-commit-on-touch-unified-gpu-memory-pressure.md` (an object's life)
  - Related ADRs: `docs/adr/0102-a-thread-blocks-in-two-steps-and-a-waker-never-waits-ipc-leaves-the-kernel-lock.md` (the entry before the lock, the copy without one), `docs/adr/0098-the-kernel-lock-is-split-in-the-order-the-desktop-in-use-holds-it.md` (the order), `docs/adr/0077-memory-is-committed-on-first-touch-the-kernel-touches-user-memory-only-through-a-checked-range.md`, `docs/adr/0082-a-process-is-its-address-space-the-parent-orders-it-whole-freeze-thaw-kill-and-a-frozen-receiver-refuses-the-park.md`

## Context

After ADR-0102 an IPC between two harts waits for no third, but every page fault, every
mapping and every `vmo_runs` still takes the kernel lock. On the board `vmo_runs` holds it
0.4–1.5 ms a call (234–795 ms in a 10 s window); a fault zeroes its frame under it (on a
multi-hart QEMU lane a host page fault inside that zeroing made every other hart wait
44–155 ms); and an unmap flushes the TLB of every hart, whoever ran the process.

RFC-0102 gives the ranks. It does not say where an address space lives so that a hart finds it
without the lock that owns the table, what a fault may do between looking at its region and
writing its page table entry, which harts a shootdown may leave out, or what keeps a memory
object alive under a mapping syscall that no longer excludes the close of its capability.

## Decision

**1. Every address space is a slot at a fixed address behind its own lock.**
`AddressSpaceManager` is a block table of `RankedLock<Option<Box<AddressSpace>>>` (class
address space, rank 3); every method takes `&self`, and `get` answers a guard. One rule
replaces the kernel lock for them: **a space is never held together with a second one, nor
asked for again while it is held**; nothing of rank 1 or 2 is taken under it. A walk over the
spaces (`each`) holds one at a time.

**2. A fault is look, commit, install — and only the two outer steps hold the space.**
`populate::fill_free`: with the space held, classify the fault and find the region and the
object's page; with **no lock held**, take a frame and zero it (`vmo::commit_fresh`); with the
space held again, install the entry **only if the region is still what the look saw and the
object still names a frame for that page**. Anything else — a violation, a squeezed or swapped
page, an empty pool, an order or a pressure level waiting — is the locked path's, which redoes
the fault whole. The trap asks this path first (`trap/fault_entry.rs`) and asserts that no
ranked lock outlives it.

**3. Single frames come from a magazine per hart.** `frame_pool::alloc(0)` takes from the
hart's magazine (sixteen frames, refilled under the pool's lock, lowest first so a one-hart
boot stays deterministic). A free goes straight to the pool: its refusals (a double free, a
foreign block) stay exact. A pool that would refuse first takes the magazines back.

**4. A region that is being unmapped is `going`.** An unmap is clear, shootdown, forget, and
the space is free between the steps. `VaSpace::begin_unmap` marks the region: it stays
recorded (its address is not reused before the shootdown landed), no fault fills it, and a
second unmap of it is refused. Without the mark a second thread's forget took whatever a third
had mapped at that address in between — its page table entries stayed, its region was gone,
and the object's frames could be freed under them.

**5. A shootdown for one space reaches the harts that may hold a translation of it.**
`HartSpaces` keeps a word per space, a bit per CPU. A hart sets its own bit before it can load
a translation — before its `satp` names the space (`enter_space`, interrupts off across bit,
`satp` and entry), and before a copy through the space's tables without a lock
(`Ipc::read`/`write`, whose only protection is the acknowledgement the unmap waits for). The
unmap clears its entries, then reads the set; the hart sets its bit, then reads the tables —
a full barrier on both sides, so one sees the other. A hart leaves a set only by its own full
flush while it runs on another space: the responder's, when its mailbox names that one space
(`tlb_space`; requests for two spaces pending at once name none), or the initiator's own. A
new space under an old handle starts empty: its ASID is fresh, or was recycled behind a flush
of every hart — which stays `shootdown_all`.

**6. `vm_map`, `vm_unmap` and `vmo_runs` run before the kernel lock, on a held capability.**
They are bodies of the entry ADR-0102 made (`ipc_ctx::body`). A mapping and a device's runs
hold the object's capability for the call (`pin_object`: a second, counted capability — only
for a memory object), so the close of the slot by a sibling thread frees nothing under them.
The device learns the runs only of an object **claimed** for it in one hold of the object
table (`vmo::name_for_device`: not sealed, not unlocked for purging, and neither from then
on) — a lazy object before its pages are committed and named, an eager one after its runs
passed the range and the reach. The answer is written through the caller's page table without
a lock; a page of it that is not ready asks for the kernel lock and the call re-executes.

**7. A memory object counts the address spaces that map it.** Its header carries two more
counts — spaces that map it, spaces that hold a writable region of it — kept by a type, not by
the paths: a space holds one `Mapped` per object it maps (`cap::SpaceMaps`: the regions that
name the object, how many are writable, the token), taken with its first region, turned
writable with its first writable one, given back with its last; a space that is torn down
drops them. Every write to a region table goes through `mm/space_regions.rs` — count, then
record; forget, then give back — and `AddressSpace` hands its region table out for writing to
that module only. The token is made in one hold of the object table together with the check
of the object's state (`mm/vmo/mapping.rs`): an object unlocked for purging is mapped by
nobody, a sealed one by no writer. The seal, the purge unlock and the destroy read the counts
in their own hold of the same table. So the walks over every address space are deleted
(`any_space_maps_vmo`, `…_writable`, `spaces_mapping_vmo`), and a check-then-act that the
kernel lock used to make atomic — seal against a writable mapping, unlock against a mapping,
destroy against a mapping — is atomic again without it.

**8. An object is born held, a syscall holds the object it names, and the judge takes no
lock but the table's (TASK-0330 P4).** `vmo::create` and the two adoptions answer the id
together with the object's first capability, minted in the table hold that enters the object
(`vmo::Born`); no capability is minted from an id any more. A creator keeps that capability
until a slot or a region names the object; the kernel keeps the boot image's for its whole
life. Every syscall that names a memory object by a slot holds a second capability to it for
the call (`syscall::api::pinned`), and that one's drop is where an orphan is judged. So an
object nobody holds and nobody maps is garbage whichever object its id names by then, and
any judge may take it: `vmo::destroy` reads both counts in the hold that takes the object.
The judge's detour through the kernel lock is deleted. The swap stage names an object by a
page's token, not a slot, and needs no pin: its commit and its page-in are one table hold
each that checks the page's own export counter, and the sealed record binds counter, object
and page.

**9. A copy through an object's pages is counted where a mapping is counted, and runs
without the kernel lock (TASK-0330 P4).** `vmo_write` and `vmo_read` are one body for both
entries (`syscall::api::vmo_copy`). The caller holds the object (8) and takes the object's
count for the copy's length through the mapping's door — a writer's for a write — in the
hold that refuses a sealed object a writer and an unlocked one anyone. A seal, an unlock for
purging and a destroy read that count in theirs, so none of them finds the object free of a
copy in flight; a squeeze takes only an object no capability outside its parked process
names. The caller's buffer is translated, never dereferenced (`mm::user_copy`); a page of it
that is not ready is filled as its fault would fill it (`populate::fill_free`), and only what
that refuses takes the kernel lock. The copy moves in stretches of four pages and reads the
hart's shootdown mailbox between them.

## Consequences

- On two and on eight harts under QEMU, of about 9 100 user page faults at boot 15 take the
  kernel lock (`KINIT: mm faults (free=9120 locked=16 …)`); `vm_map`, `vm_unmap` and
  `vmo_runs` take it zero times (`KINIT: bkl mm calls`); of 28 unmaps' shootdowns on eight
  harts 21 harts are reached instead of 196 (`KINIT: tlb shootdowns`).
- Threads of one process still share one exclusive lock twice a fault, and every fault takes
  the one object table's lock: `SELFTEST: fault scale` reads 1.6× with two threads under QEMU.
  The board's number against the gate (≥ 6× with eight) decides the next step — a shared hold
  for the look and the install, and a lock per object (rank 4).
- A lazy object's `vmo_runs` call refused after the claim (no frame, out of the device's reach)
  leaves the object claimed: it can no longer be unlocked for purging. Fail-closed.
- The locked forms stay for a caller the entry does not serve (an order pending, a pressure
  level waiting, a kernel-mode task): the syscall table's bodies and the phased unmap. They go
  with the kernel lock (TASK-0330 P4).
- With the copies off the lock, the memory syscalls' takes of the kernel lock per 20 s window
  fall from 1046 to 109 on two harts and from 696 to 121 on eight; their hold from 112 ms to
  47 ms and from 110 ms to 63–70 ms (`KINIT: bkl takes`, `KINIT: bkl held us`).
- A seal, an unlock for purging and a destroy are refused while a copy by another holder is
  in flight, where they used to wait for the kernel lock. A copy is a writer or a reader of
  the object for its length; the caller that raced it asks again.
- The boot image's objects are never reclaimed: the kernel holds their first capabilities.
- A space's lock is held across a whole eager mapping and a whole clear; a sibling thread's
  fault waits that long. `KINIT: long call … nr=` names a call above 10 ms.
- No space is held across a shootdown — RFC-0102's rule (no lock across a wait for another
  hart), applied to every path: the unmap (`vm_ops::unmap_range`, the syscall's body and the
  kernel's own callers), the squeeze (`squeeze::run` walks with the space held, shoots down
  and frees after), the selftest's table oracle. Measured: a hart parked before its scheduler
  loop acknowledged one round late (`KINIT: tlb slow ack …`), and the space held across that
  wait read `max_hold_us=105000` in the boot window; with the rule applied it reads 5 600–9 000
  (an eager mapping), 81–174 in the steady window.
- The soft-interrupt handler reads the shootdown mailbox again after it cleared the doorbell:
  a request raised between the trap's first read and the clear rang a doorbell the clear ate.

## Alternatives considered

- **One reader/writer lock over all address spaces.** Rejected: a fault of one process would
  still write a word every other process's fault reads.
- **Hold the space across the zeroing.** Rejected: it is the longest step of a fault and
  touches nothing of the space; the re-check at the install is what makes dropping it sound.
- **A magazine on the free side too.** Rejected for now: a free would be accepted before the
  pool judged it, and a double free would surface sixteen frees later on another hart.
- **Reach every hart that ever ran the space** (a set that only grows). Rejected: after a
  minute of placement by load that is every hart; the flush that empties a hart is the moment
  it can leave.
- **Keep the copy under the kernel lock and only shorten it.** Rejected: on eight harts the
  64 KiB chunks of one 4 MiB fill held the lock 10–13 ms each while seven harts spun for it
  (`KINIT: bkl long hold … by=6`), and in those runs the session stage opened past init's
  two-second witness (4 runs of 4; none of 6 without the long holds). Timed once, the pin
  cost 5 µs and its drop 7 µs; the bytes were the hold.
- **A lock per object for the copy.** Rejected: the count already answers the three
  questions a seal, an unlock and a destroy ask, in a hold of a few instructions; a lock held
  across 64 KiB of bytes would be the hold this decision removes.
- **Count regions, not spaces.** Rejected: the squeeze's rule ("mapped by no other space") and
  an image's `mappers=` are about spaces; a count of regions would need the walk back to
  tell two regions of one space from two spaces.
- **Let each path that records a region keep the count.** Rejected for the reason RFC-0102
  gives for capabilities: eleven sites record or forget a region; one that forgot the count
  would free frames under a mapping.

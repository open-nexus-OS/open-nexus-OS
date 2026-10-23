<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# ADR-0077: Memory is committed on first touch — the user page fault fills a lazy page, and the kernel touches user memory only through a checked range

- Status: Accepted
- Date: 2026-10-23
- Links:
  - Tasks: `tasks/TASK-0333-memory-the-efficient-one-and-the-app-lifecycle-layer-6.md` (6b-1)
  - RFCs: `docs/rfcs/RFC-0100-memory-object-model-v2-commit-on-touch-unified-gpu-memory-pressure.md` (§2),
    `docs/rfcs/RFC-0085-…` (kernel-owned VAs)
  - Related ADRs: `docs/adr/0073-an-object-dies-with-its-last-capability-a-death-is-an-event-on-an-endpoint.md`
    (the object's lifetime), `docs/adr/0054-map-errors-keep-their-identity-across-the-abi.md`
  - Measurement: `docs/board/measurements/2026-10-22-commit-on-touch/README.md`

## Context

Every page of this system is committed when it is reserved. A VMO's frames are taken and zeroed
at create, `vm_map` writes every PTE, and exec allocates, zeroes and maps every RW segment's
`.bss` and the whole stack. A service's static heap therefore costs its full size in frames and a
memset at every spawn whether a byte of it is used or not: app-host reserves a 4 MiB heap and a
1 MiB frame arena in every app process, and on board cycle 63 its apps touched 0.10–0.84 MB of the
heap. A contiguous object keeps the whole power-of-two buddy block it was cut from; gpud's layout
block held 6 443 frames (25.2 MiB) that nothing counted. The user page fault has one meaning,
kill.

Two facts shape the change. The kernel reads and writes user memory directly inside syscalls
(`SUM=1`) after a range check that never asks whether the page is mapped — an unmapped pointer in
range, or a read-only buffer handed to a receive, is a kernel page fault and a panic today, and
any page committed lazily would add a third such case. And exec's copy of an image runs with the
kernel lock dropped, so a fix-up inside a kernel fault would have to work both with and without
the lock.

## Decision

1. **A lazy kind.** A VMO created with `VMO_CREATE_LAZY` (bit 1 of `vmo_create`'s flags) is
   anonymous and zero-filled: no frame is taken at create; a page takes one 4 KiB frame the first
   time it is touched — by a user fault, by the kernel through a checked range, by `vmo_write`, or
   by a device asking its runs — and that frame is zeroed before anyone can see it. A page is
   committed once; it returns when the object dies. A contiguous object is never lazy (a display
   controller cannot fault).
2. **exec reserves, the touch commits.** Each RW segment's `.bss` beyond the page that holds its
   last file byte, and every stack, are backed by a lazy object the task owns (no capability
   names it; it dies with the address space at reap, ADR-0073's rule). Every static heap and the
   frame arena cost the pages touched.
3. **The user page fault fills a lazy page.** Inside a region over a lazy object, with the
   access's right (a store needs write; a lazy page is never executable), the fault commits the
   page if it is not yet committed, maps it (4 KiB, A/D set as every PTE today), fences this
   hart's TLB for that address and resumes the instruction. A fault on a page whose PTE is
   already valid with the right is spurious — another hart committed it, or this hart cached the
   miss — and is answered by the fence alone. Every other fault kills as before; a fault that
   finds no free frame kills with its reason named, never a panic.
4. **The kernel touches user memory only through a checked range.** Before a syscall reads or
   writes user memory it asks `user_range(ptr, len, access)`: every page must be mapped with the
   access's right — a lazy page is committed and mapped first — or the syscall returns `EFAULT`.
   An unmapped or read-only buffer is an error to the caller, no longer a kernel panic.
5. **A contiguous object keeps exactly its frames.** The tail of the power-of-two block it is cut
   from goes back to the pool at create.
6. **RSS is committed memory.** A region over a lazy object counts its committed pages; MmStats
   v3 adds the committed and the reserved bytes, so `rss` drops where the touch does.

## Consequences

- An app-host child resident in its touched pages instead of its reservations; the frame arena
  can be reserved for the largest frame without costing it (6b-2); gpud's 25 MiB tail is free.
- A first touch costs a trap, a zeroed frame and a PTE (measured as `us_per_fault`); lazy regions
  map 4 KiB pages only — no superpage promotion.
- Every syscall that touches user memory pays a page-table walk per page of its buffer (IPC
  messages are one to three pages), and a bad pointer is `EFAULT` everywhere.
- Copy-on-write, the zero page and the file-backed kind are not part of this decision (RFC-0100,
  6c); dirty tracking keeps A/D eager (6e/6f).

## Alternatives considered

- **Fix-ups for kernel-mode faults on user addresses (an exception table).** Rejected: the fault
  would arrive with the kernel lock held in most syscalls and dropped in exec's copy phase, and a
  missed call site would still be a silent class; one checked range is simpler, testable on the
  host, and turns the unchecked-pointer panic into `EFAULT` as a side effect.
- **Every anonymous VMO lazy by default.** Rejected: device consumers read an object's runs to
  program DMA; laziness is opt-in, and a device's runs commit what they name.
- **A shared zero page for first reads.** Deferred to copy-on-write (6c): without it a read of an
  untouched page commits a private zero page, which costs one frame per page read.

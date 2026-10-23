# RFC-0100: The memory object model v2 — a VMO dies with its last capability, pages committed on touch, one pool shared with the GPU, pressure as an event, compression before swap

- Status: Draft (seeded 2026-10-19 at layer 6's start; filled in 6b)
- Owners: @kernel-team @runtime @gpu
- Created: 2026-10-19
- Last Updated: 2026-10-23
- Links:
  - Tasks: `tasks/TASK-0333-memory-the-efficient-one-and-the-app-lifecycle-layer-6.md` (the layer, 6a–6f, execution + proof); `tasks/TASK-0286-…` (M1, Done: frames from the FDT, page-backed VMOs, the `contiguous-DMA` kind, `mm_stats`)
  - Related RFCs: RFC-0098 C4 (physical memory from the FDT; defers the object model here), RFC-0085 (kernel-owned VAs; the explicit `vmo_destroy` rule stays), RFC-0080 (`vmo_share_ro`), RFC-0087 ("exhaustion is an event"), RFC-0101 (the GPU driver: contexts with their own GPU address space, VMOs bound with a cache mode), RFC-0079 (EOF on the last sender's death)
  - ADRs (at their packages, numbers at writing): process control of a child; the app lifecycle contract; windowd never blocks on an app; one memory authority; the compressor before swap
  - Order: `tasks/IMPLEMENTATION-ORDER.md` ("Block 4 in layers", layer 6)
  - Measured: `docs/board/measurements/2026-10-10-mgs-baseline/README.md` ("Memory"); `docs/board/measurements/2026-10-20-memory-lifecycle-baseline/` (6a P0)

## Status at a Glance

- **Phase 0 (6a: object lifetime — what dies is freed)**: ✅ 2026-10-20 (board cycle 48; ADR-0073: every kernel object, a death as an event)
- **Phase 1 (6b: commit on touch, the layout block split, unified GPU memory; CoW moved to 6c)**: ⏳ 6b-1 ✅ 2026-10-23 (board cycle 64, ADR-0077: lazy objects, the fault fills, the checked user range, lazy `.bss` and stacks, the contiguous tail returned); 6b-2 (the layout block split) and 6b-3 (the allocator frees by default) open
- **Phase 2 (6c: the file-backed kind, the page cache, shared code)**: ⬜
- **Phase 3 (6e: accounting, pressure levels, purgeable VMOs and seals)**: ⬜
- **Phase 4 (6f: the backing stages — compressor, swap)**: ⬜

"Complete" means the contract below is defined and every phase's proof gates are green (the
`KSELFTEST`/`SELFTEST` lines named in TASK-0333 on `headless`, `visible` and `board-visible`).

## Scope boundaries (anti-drift)

- **This RFC owns**: the kinds of a VMO and their backing; when pages are committed and when they
  return; the lifetime rule (a VMO dies with its last capability); copy-on-write and the
  file-backed kind; the accounting fields a process is charged with, GPU memory included; the
  pressure levels, where they come from and how they are told; purgeable VMOs and seals; the
  order of the backing stages under pressure.
- **This RFC does NOT own**: the lifecycle of an app (the ADR of 6d and RFC-0065), the kill policy
  (`memd`, the one-authority ADR), the GPU driver's binding and queues (RFC-0101), the swap
  partition's layout on disk (the storage lane), the service allocator's internals (an
  implementation under `nexus-service-entry`, proven by its tests).

### Relationship to tasks (single execution truth)

TASK-0333 defines the stop conditions and proof commands of every phase; this RFC links to it and
is updated when a phase's contract is filled (6b writes §2–§4 in full).

## Context

Measured 2026-10-19 (`docs/board/measurements/2026-10-10-mgs-baseline/`, the tree): the board has
4 GiB and the system is resident in ~163 MiB, 63 % of it one contiguous block most of whose rows
are never touched; M1 gave the kernel a buddy frame allocator and page-backed VMOs, and nothing
since frees — a dead task's VMOs stay in the table with their frames, every service's heap is a
bump that never returns a byte, no page is committed late, no code page is shared, no pressure is
told to anyone. The operator's goal: a RAM management like a phone's, dynamic in what the machine
has, with the GPU in the same pool.

## Goals

1. **Nothing that dies keeps memory.** A VMO is destroyed when its last capability closes and no
   address space maps it; a task's exit closes its capabilities. (6a)
2. **Pages are committed where they are touched**, and a reserved range costs nothing until then;
   private data is copy-on-write. (6b)
3. **One pool with the GPU.** A GPU buffer is an ordinary page-backed VMO charged to its client,
   bound into the client's GPU address space; only scan-out planes a display controller reads by
   base and stride are contiguous and eager. (6b)
4. **Code is shared.** One file-backed VMO per bundle file, mapped read-only by every process that
   runs it; no process holds its own copy. (6c)
5. **Memory is accounted and pressure is an event.** Per process: private, shared, committed,
   reserved, purgeable, GPU; four levels derived from the machine's own size; told, never polled. (6e)
6. **Caches are purgeable; backing has stages.** Purgeable pages go first, then compression, then
   swap of compressed pages only, then the policy's kill. (6e, 6f)

## Non-goals

- A general-purpose allocator in services; a swap of clean file pages (they are dropped);
  policy in the kernel (bands, declarations, kill order live in `memd`); fixed sizes anywhere.

## §1 Object lifetime (6a)

- A VMO is a kernel object with a reference count = the capabilities that name it. The last
  capability's close destroys it when no address space maps it; a mapped VMO outlives its
  capabilities until the mapping is gone (the mapping holds a reference).
- A task's exit closes every capability in its table; its address space is destroyed at reap.
  The explicit `vmo_destroy` keeps RFC-0085's rule (sole owner, no mapping) — it is a fast path
  for the owner, not the only path to death.
- **The rule is every kernel object's (ADR-0073, 2026-10-20).** An endpoint, a timer, a waitset
  and a fence die with their last capability too: an exit frees what the task owned like a
  `cap_close` of each would, and an endpoint nothing names (no capability, nothing in flight)
  is destroyed and forgotten by every waitset, timer, interrupt line and exit binding that
  held its id; a dead endpoint's slot is reused only once nothing names it. A death is an
  event: the parent's bound endpoint (`child_exit_bind`, syscall 62) receives one
  `OP_CHILD_EXITED` frame per dead child; a receiver's EOF verdict never depends on its wait
  mode; a send into an endpoint no RECV capability names returns `PeerClosed`.
- Invariant and proof: `test_reject_reclaim_shared_vmo`, `test_reject_reclaim_mapped_vmo`, the
  `ipc_eof` truth tables; `KSELFTEST: vm reclaim (vmos= bytes= endpoints=)`,
  `KSELFTEST: task exit releases ok (…)`, `KSELFTEST: child exit notify ok (…)`,
  `KSELFTEST: ipc eof nonblocking ok`, `KSELFTEST: ipc peer closed send ok`;
  `SELFTEST: rss loop ok (cycles=10 leaked=0 kobjects=0)`; on the board, every window closed
  ends its process (`APPHOST: window closed - exiting`, `KINIT: vm reclaim (… by=reap)`,
  `abilitymgr: stop (inst= … live=)`).

## §2 Kinds and commit (6b-1 filled 2026-10-23, ADR-0077; the layout block 6b-2)

| Kind | Backing | Committed | Returns |
|---|---|---|---|
| anonymous (eager) | frames, zeroed at create | at create | at death |
| lazy (`VMO_CREATE_LAZY`, bit 1) | frames, one per 4 KiB page, zeroed before mapped | on first touch: a user fault, the kernel's checked range, `vmo_write`, a device's `vmo_runs` | at death (purge when purgeable, 6e) |
| contiguous-DMA | one run inside the master's reach (M1); the power-of-two block's tail returned at create | eagerly (a controller cannot fault); never lazy | at death |
| file (6c) | the page cache of a bundle file | on first touch, shared read-only | dropped under pressure, re-read |

- **exec** backs each RW segment's `.bss` beyond the page of its last file byte, and every stack,
  with a lazy object the task owns (no capability; it dies with the space at reap, §1).
- **The user page fault** inside a region over a lazy object, with the access's right (a store
  needs write; a lazy page is never executable): commit the page if it is not yet committed, map
  it (4 KiB, A/D set), fence this hart's TLB for the address, resume. A valid PTE with the right
  is a spurious fault (another hart committed it, or a cached miss) — fenced, resumed. Anything
  else kills as before; no free frame kills with the reason named, never a panic.
- **The kernel touches user memory only through `user_range(ptr, len, access)`**: every page
  mapped with the access's right — a lazy page committed and mapped first — or `EFAULT`.
- **RSS** of a region over a lazy object is its committed pages; MmStats v3 adds committed and
  reserved bytes.
- `vmo_read` of an uncommitted page reads zeros without committing it; `vmo_write` commits what it
  writes; a device's `vmo_runs` commits the range it asks for (its pages are then pinned until the
  object dies).

Copy-on-write (6c): a private mapping of a file or shared page breaks on the first write (the
fault copies one page). The user fault stays fatal for a true violation (no mapping, wrong rights).

The layout block (RFC-0101 §8, `nexus-display-proto::layout`): the scan-out planes are one
contiguous-DMA VMO; the atlas and the GPU's caches are anonymous commit-on-touch VMOs bound into
the client's GPU context — `vm_bind` commits what it binds (the GPU MMU has no populate path; a
fault there is a context reset), a purge touches only unbound pages. The row formulas do not change.

## §3 Accounting and pressure (6e, to be filled)

Per process: private, shared (counted once per object, attributed by first mapper), committed,
reserved, purgeable, GPU-bound (charged to the context's client). Pressure levels normal / warn /
critical / emergency from watermarks that are fractions of the boot-time free total per bank
(warn 25 %, critical 10 %, emergency 5 % free, floored at a page count) — the same code prints
different absolutes on QEMU 320M and the 4 GiB board. The kernel notifies an endpoint on a level
change (the `irq_bind` pattern); `memd` alone waits on it and fans out `OP_MEMORY_PRESSURE`.

## §4 Purgeable VMOs and seals (6e, to be filled)

A purgeable VMO's clean pages may be dropped by the kernel at warn or above; the owner learns it
on the next access (a status, never a fault that kills). Seals (TASK-0290): a sealed read-only
VMO refuses a write mapping deterministically.

## §5 Backing stages (6f, to be filled)

Under pressure, in this order: purgeable pages dropped → clean file pages dropped → anonymous
dirty pages compressed into the kernel's pool → compressed pages written to the `swap` partition
through blkd → `memd`'s kill, by band, journaled. Clean pages are never written to swap.

## Invariants (security)

- Identity = `sender_service_id`; a pressure subscription names its service by kernel identity.
- A purge never touches a GPU-bound or DMA-bound page.
- Bounded input: every size on the wire is checked against the kinds' limits before allocation.
- `test_reject_*` per phase (TASK-0333).

## Open questions (decided by measurement)

- QEMU's virtio backing-entry bound for a per-band attach of the atlas (6b P0).
- The PTE A/D bits on the board's core (clean/dirty per page in hardware or software) — 6b P0.
- The compressor's ratio and cost per page on RV64 (R11, 6f P0).

---
schema: 1
id: RFC-0100
title: "The memory object model v2 — a VMO dies with its last capability, pages committed on touch, one pool shared with the GPU, pressure as an event, compression before swap"
status: done
status_note: "2026-10-30 — phases 0–4 green: TASK-0333 6a, 6b, 6c, 6e, 6f; board cycles 48–77; seeded 2026-10-19 at layer 6's start, filled in 6b"
owners: ["@kernel-team", "@runtime", "@gpu"]
created: 2026-10-19
updated: 2026-10-28
subsystem: kernel
tasks: [TASK-0333, TASK-0286]
links:
  - label: Tasks
    target: "`tasks/TASK-0333-memory-the-efficient-one-and-the-app-lifecycle-layer-6.md` (the layer, 6a–6f, execution + proof); `tasks/TASK-0286-…` (M1, Done: frames from the FDT, page-backed VMOs, the `contiguous-DMA` kind, `mm_stats`)"
  - label: "Related RFCs"
    target: "RFC-0098 C4 (physical memory from the FDT; defers the object model here), RFC-0085 (kernel-owned VAs; the explicit `vmo_destroy` rule stays), RFC-0080 (`vmo_share_ro`), RFC-0087 (\"exhaustion is an event\"), RFC-0101 (the GPU driver: contexts with their own GPU address space, VMOs bound with a cache mode), RFC-0079 (EOF on the last sender's death)"
  - label: "ADRs (at their packages, numbers at writing)"
    target: "process control of a child; the app lifecycle contract; windowd never blocks on an app; one memory authority; the compressor before swap"
  - label: Order
    target: "`tasks/milestones/hardware-network.md` (\"Block 4 in layers\", layer 6)"
  - label: Measured
    target: "`docs/board/measurements/2026-10-10-mgs-baseline/README.md` (\"Memory\"); `docs/board/measurements/2026-10-20-memory-lifecycle-baseline/` (6a P0)"
---

# RFC-0100: The memory object model v2 — a VMO dies with its last capability, pages committed on touch, one pool shared with the GPU, pressure as an event, compression before swap

## Status at a Glance

- **Phase 0 (6a: object lifetime — what dies is freed)**: ✅ 2026-10-20 (board cycle 48; ADR-0073: every kernel object, a death as an event)
- **Phase 1 (6b: commit on touch, the layout block split, unified GPU memory; CoW moved to 6c)**: ✅ 2026-10-23 — 6b-1 ✅ 2026-10-23 (board cycle 64, ADR-0077: lazy objects, the fault fills, the checked user range, lazy `.bss` and stacks, the contiguous tail returned); 6b-2a ✅ 2026-10-23 (board cycle 65: the app arena reserved, the splash given back, the atlas counted); 6b-2b ✅ 2026-10-23 (board cycle 66, ADR-0078: one pool with the GPU — the ring contiguous, the surfaces lazy and entered as sampled, the GPU's ceiling two thirds of RAM); 6b-3 ✅ 2026-10-23 (board cycle 67: every service's heap frees, a double free refused, an object freed while mapped dies at its unmap)
- **Phase 2 (6c: sealed images, shared code)**: ⏳ 6c-0 ✅ 2026-10-23 (the OS build's own flags), 6c-1a ✅ 2026-10-25 (board cycle 69, ADR-0079: cache maintenance a non-coherent device's right), 6c-1b ✅ 2026-10-26 (board cycle 70, ADR-0080: `vmo_seal` + `exec_vmo` the one loader, text and rodata mapped from the sealed image's frames, the volume's images sealed before they are verified), 6c-2 ✅ 2026-10-26 (board cycle 71, ADR-0081: the boot image one container — kernel + bootfs under one digest, every core program a sealed object the kernel adopts, no program embeds another; the loader in every chain). **Phase 2 complete**
- **Phase 3 (6e: accounting, pressure levels, purgeable VMOs and seals, the restorable contract)**: ✅ 2026-10-27 (both QEMU lanes + board cycle 73; ADR-0085/0086 Accepted) — 6e-0 built 2026-10-27 (ADR-0085: `mm_stats` v4 with shared bytes, the level and its watermarks; the watermarks from each bank's boot-time free; `mem_pressure_bind` (67) — one sink, one frame per level change, the level at the bind; headless proof `KSELFTEST: mem pressure notify ok`, `mem watermarks ok`); 6e-1 built 2026-10-27 (ADR-0086, §6: `restorable = true` → `nexus.permission.STATE` → `svc.state`; the record `/state/app/@self/persist` by kernel identity; written on the compositor's `hidden` sighting and at close, restored before the initial effects; settings' navigation the first; board proof pending in the 6e cycle); 6e-2 built 2026-10-27 (ADR-0086, §3: memd the one authority — the bands from the lifecycle authority, the kill through it, proven by the death; both lanes); 6e-3 lane-proven 2026-10-27 (§4: purgeable VMOs — syscall 68 unlock/lock, the purge at warn before the frame leaves; the level told onward memd → gpud, memd → windowd → every app window → the host event `MemoryPressure`; `KSELFTEST: vmo purge ok` on both lanes, the whole fan-out chain at warn and critical on the visible lane); 6e-4 lane-proven 2026-10-27 (§3: GPU memory charged to its client, gpud → memd `OP_GPU_CHARGE`; the lent framebuffer object charged on both lanes, the context part is the board's — cycle 73 pending)
- **Phase 4 (6f: the backing stages — compressor, swap)**: ✅ 2026-10-30 — 6f-0 ✅ 2026-10-28 (board cycle 74; §5: the one codec `nexus-compress` — LZ4 block, bounded decoder, allocation-free; measured in the kernel on every lane and on the board: 47 % on the image's machine code, 49 µs to compress and 16 µs to inflate a page on the board's core — R11 answered: a frozen process is squeezed whole, a fault inflates synchronously); 6f-1 ✅ 2026-10-28 (board cycle 75; §5 "The pool", ADR-0087 Accepted: `task_squeeze` (69), the store per object, every commit path inflates, memd squeezes before it kills — an app-host's heap and stack 1.8 MB → 468 KB on the usb-visible lane, the probe child's squeeze 45 µs a page on the board's core); 6f-2 ✅ 2026-10-28 (board cycle 76; §5 "The swap stage", ADR-0088 Accepted: the records sealed under the boot's key (ChaCha20-Poly1305, the RNG service seeds it on the pager's request), `swap_export`/`_commit`/`_page_in` (71–73), a touch parks until the pager's answer, the `swap` partition memd's alone — on the usb-visible lane a frozen app-host's 450 squeezed pages out to the pager's disk and back at the thaw; on the board's harts `KSELFTEST: swap ok`, no key there — no entropy device); 6f-3 ✅ 2026-10-30 (test-all + board cycle 77; §5 "memd's journal and the `lowmem` lane": every kill memd ordered and saw carried out is journaled in the state store; on the `lowmem` lane the kernel's own `critical` squeezes, swaps out and kills a frozen app (`reason=oom`), the kill journaled, and the app relaunched comes back where it was; the lanes declared and deterministic, ADR-0089). **Phase 4 complete**

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
  capabilities until the mapping is gone (the mapping holds a reference) — and dies at that
  last unmap: `vm_unmap` applies the same rule to the object its region mapped (6b-3,
  `KSELFTEST: vm unmap reclaim ok (bytes=)`).
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
| sealed (6c-1b, ADR-0080) | an anonymous object sealed by `vmo_seal` — a bundle's `payload.elf` filled by bundlemgrd and hashed after the seal, an embedded image copied once from the binary; the page cache of an image, held by its spawner | at create (the image's bytes); mapped by `exec_vmo` into every process of the image — text and rodata the object's own frames, data copied | at death (the spawner's capability and the last process's reap); dropping clean pages under pressure is 6e/6f |

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

**A process is a sealed image (6c-1b, ADR-0080).** `exec_vmo` is the one loader: it takes a
sealed object whose `PT_LOAD`s each begin on their own page-aligned file page (the linker's
`-z separate-loadable-segments`; the kernel refuses anything else), maps every read-only or
executable segment from the object's frames — only this path maps a page executable, and only
from a sealed object — copies a writable segment's file pages into frames the process owns and
backs its `.bss` and stack with lazy objects. The region naming the object keeps it alive while
the process runs (§1). Copy-on-write is not built: the data segments measured 8–32 bytes, one
copied page beats a fault. The user fault stays fatal for a true violation (no mapping, wrong
rights).

**One pool with the GPU (6b-2b, ADR-0078).** No memory is set aside for the GPU. The layout
block (RFC-0101 §8, `nexus-display-proto::layout`) splits in two objects over one row numbering:
the scan-out ring (the three slots a display controller reads by base and stride) is one
contiguous-DMA object inside the controller's reach; the wallpaper, the retained plane and the
surface atlas are one **lazy object made for the GPU** — its frames from inside the GPU's reach,
its ring rows a hole never touched. The GPU's MMU has no populate path (a fault there is a context
reset), so a page is committed when the GPU is *told* it: the lazy object is bound into the
client's GPU context as one sparse range with nothing entered, and before a pass gpud enters the
pages of every rectangle the frame samples — computed from the descriptor the GPU reads — through
`vmo_runs`, which commits what it names. A page named to a device is pinned until the object dies;
a purge (§4) never takes one. windowd's `vmo_write`s and the GPU's entries name the same frames.

The share is a fraction of the machine, read at boot from `mm_stats` (the RAM the kernel took from
the device tree), never a constant: the GPU's **ceiling is two thirds of RAM** (a GPU buffer past
it is refused by name) and the surfaces' **reservation an eighth of RAM** in whole display planes,
at least the fixed layout, at most what the ceiling leaves — on the 4 GiB board 2.65 GiB and 64
planes (the atlas 59, before 8). The framebuffer grant's length carries the surface object's size;
windowd's atlas ends where the object ends. The row formulas do not change. The virtio display
keeps one eager object of the fixed layout until a per-band attach (6f): its attach lists, and so
names, every page.

## §3 Accounting and pressure (6e-0 filled 2026-10-27, ADR-0085)

**The record (`mm_stats` v4, syscall 61).** Per caller: `own_rss_bytes` (committed: every page of
an eager region, the touched pages of a lazy one), `own_shared_bytes` (the part of those over
objects another space maps too — a shared image's text, the atlas; private = rss − shared),
`own_dma_bytes`, `own_reserved_bytes` (lazy, untouched), `own_purgeable_bytes` (§4). The pool:
`total`, `free`, the three watermarks, `pressure_level`, `purged_pages`. GPU memory is charged
to its client by gpud's ledger, not by the kernel: the kernel sees gpud's pages. `KINIT: mm space
pid= sid= rss= dma= reserved= shared=` names the top spaces at the late fence.

**GPU memory charged to its client (6e-4 filled 2026-10-27).** Every byte gpud holds, it holds
FOR a client: the framebuffer object it made and lent the compositor (its whole span — the
scan-out planes and the atlas' reservation, ADR-0078), and on the board the buffers of the
client's GPU context. gpud keeps the figure per client (by kernel service id, two parts, eight
clients; a ninth refused by name) and tells the one memory authority each change as
`nexus_wire::memd::OP_GPU_CHARGE {client, bytes}` — an absolute figure that replaces the last,
zero frees the row; NONBLOCK, the next change tells the truth (`gpud: gpu memory charged (client=
bytes= told=)`). memd takes it from gpud's sender id alone (`test_reject_gpu_charge_from_anyone_
but_gpud`) into its own copy of the table (`memd: gpu charge (client= bytes= total=)`): the
authority's picture of a process now includes what the GPU holds for it. The join to a process's
pid comes when a client is an app (app surfaces of their own, layer 8); until then the one client
is the compositor. Proof on every display lane and the board: both lines for `client=windowd`.

**The levels.** Four: normal / warn / critical / emergency. The watermarks are set when the pool
is built: each bank's free frames contribute 25 % / 10 % / 5 % to three pool-wide thresholds,
floored at 1024 / 512 / 256 frames. `free ≤ watermark` enters a level at once; leaving it needs
free frames above the watermark by an eighth (hysteresis). The level is the pool's, not a bank's
(the buddies fill banks in tree order). Proof: `KSELFTEST: mem watermarks ok (total= free= warn=
crit= emerg= level=)` — ordered and below the total, on QEMU and the board alike.

**The event.** `SYSCALL_MEM_PRESSURE_BIND` (67): a SEND capability to an endpoint binds it as the
one sink; every level change writes one frame `OP_MEMORY_PRESSURE` (0x71): `[op, level, free:
u64, total: u64]` (18 bytes) and wakes the waiter; the bind delivers the level of the moment. The
first binder holds the sink until its endpoint dies; another process is refused (`EPERM`). The
pool observes with its lock dropped; delivery is at the syscall epilogue, the fault's fill, the
timer tick and the idle loop. Proof: `KSELFTEST: mem pressure notify ok (level= then=warn
reject=denied …)`. `memd` (ADR-0086) binds it and fans out `OP_MEMORY_PRESSURE` to gpud, windowd
and, through app-host, to apps as a DSL event (6e-3). The pressure a real pool run low produces is
the `lowmem` lane's (6f-3, §5: the harness takes the pool to the kernel's own `critical`); the
path it takes is proven here.

**The authority (6e-2 filled 2026-10-27, ADR-0086).** `memd` is the one listener: it binds the
sink to its declared endpoint pair at start (the kernel's first frame, `memd: pressure (level=
free= total=)`, is the live proof on every lane) and waits on that endpoint and its server
endpoint. It learns what each app process is to the user from the lifecycle authority as a DIFF
of BANDS (`nexus_wire::memd::OP_BAND`: foreground 0 > visible 1 > declared 2 > hidden 3 >
suspended 4, `gone` at the death report), taken from abilitymgr's kernel sender id alone. At
`critical` the kill floor is `hidden`, at `emergency` `visible`; the victim is the highest band
at or above the floor, the longest there first; one kill in flight. The order is
`OP_MEM_KILL {pid}` to abilitymgr (from memd's sender id alone; a foreground or starting instance
is refused), which orders the parent (execd `OP_KILL`); the kill is proven when the victim's
`gone` band comes back — `SELFTEST: mem pressure kill ok (band= reason=oom|drill)` — and a
kernel-sourced kill then reads the level again (one frame per level change). The proof harness
drives the chain on a QEMU lane with `OP_DRILL {level}` (its sender id alone, never on a board):
a simulated level over the real bands. `STATUS` answers the sink, the level and the bands:
`SELFTEST: memd bound ok (level= bands=)` on every lane.

## §4 Purgeable VMOs and seals (6e-3 filled 2026-10-27; seals built)

A purgeable VMO's clean pages may be dropped by the kernel at warn or above; the owner learns it
on the next access (a status, never a fault that kills). A page bound into a GPU context or told
to a device (`vmo_runs`) is never purged (ADR-0085).

**The purge, as built (6e-3).** An object is purgeable only while its owner says it is not
using it: syscall 68 `vmo_purgeable (slot, unlock)` by a holder of the MAP right. `unlock = 1`
hands the object to the kernel — refused (`InvalidArgs`) for an eager object (its frames are
its contents, nothing to rebuild), for one a device was ever told the runs of or that is bound
into a GPU context (device-named — the hardware may still write it), and for one any address
space maps (a mapped page cannot vanish under a running program; unmap first). Unlocked, the
object refuses `vm_map`, `vmo_write`, `vmo_read` and `vmo_runs` — the owner cannot touch what
it gave away. `unlock = 0` locks it again and answers **1 if the pages were purged** (the object
is now empty, lazily recommitted on the next touch: the owner rebuilds) or 0 (still intact).
The kernel purges every unlocked object — frees its frames to the pool, uncommits every page —
when the level it is about to tell reaches `warn`, before the frame leaves (`MM: pressure purge
level= pages=`); no authority need ask, no word crosses a boundary first. `MmStats`
`own_purgeable_bytes` is the committed bytes of the unlocked objects the caller's capabilities
name; `purged_pages` counts what the purge ever took. Host: `test_reject_purge_of_bound_vmo`,
`test_reject_purge_of_mapped_vmo`, `test_reject_purge_of_eager_vmo`; kernel selftest
`KSELFTEST: vmo purge ok (pages=4 purged=4 relocked=purged reject=bound,eager,mapped)` on every
lane. The first users (atlas bands, glass cache, blur cache) follow the ledger that says what each
costs whom (6e-4 and after); the drop of a host-GPU-resident texture is not a purge of pool
frames and is gpud's own matter.

**The level told onward (6e-3, ADR-0086).** memd pushes every level — the kernel's and a drill's
— as `OP_LEVEL {level, source, free, total}` on two declared legs, NONBLOCK: to gpud (named:
`gpud: mem pressure (level= source=)`; what it drops is 6e-4's with the ledger) and to windowd,
which tells every live app window `OP_SURFACE_PRESSURE {level}` on its event channel (`windowd:
mem pressure (level= source= windows=)`); app-host raises it into the program as the host event
`MemoryPressure` with the cases `Normal`/`Warn`/`Critical`/`Emergency` (`apphost: mem pressure
(level= handled=)`) — a program without the event does no work. Each consumer takes the push
from memd's kernel sender id alone. Proof: the harness's drill runs `warn` once a thawed app is on
screen (a frozen window cannot hear the level) and `critical` once an app is frozen again;
`SELFTEST: mem pressure fanout ok (level=warn sent=2)` says both legs took it, `apphost: mem
pressure (level=warn handled=)` that the window did.

**Seals (6c-1b, ADR-0080, built):**
`vmo_seal` is one-way, by a holder of the writable capability; refused while any space maps the
object writably, for a lazy or fixed object, and for one a device was ever told the runs of
(`vmo_runs`). Sealed, the object refuses `vmo_write`, a writable `vm_map` and `vmo_runs` for
the rest of its life — deterministically, by the object, whoever holds a capability.

## §5 Backing stages (6f-0 filled 2026-10-28: the codec and its cost; 6f-1 filled 2026-10-28: the pool, ADR-0087; 6f-2 filled 2026-10-28: the swap stage, ADR-0088; 6f-3 filled 2026-10-29: memd's journal and the `lowmem` lane)

Under pressure, in this order: purgeable pages dropped → clean file pages dropped → anonymous
dirty pages compressed into the kernel's pool → compressed pages written to the `swap` partition
through blkd → `memd`'s kill, by band, journaled. Clean pages are never written to swap.

**The codec (6f-0, built).** One block format on both sides of the stage boundary — the kernel's
pool and memd's swap pager — so a blob the kernel made is what memd writes and what the kernel
reads back after a page-in: the LZ4 block format (a token per sequence — a literal run, a 16-bit
backwards offset, a match of at least four bytes; the last sequence literals only), in
`source/libs/nexus-compress`: `no_std`, zero dependencies, `forbid(unsafe_code)`,
allocation-free (the encoder's one hash table, 4 KiB, is the caller's — the kernel keeps one per
hart, a service one per thread; cleared per call, so a page's blob never depends on what was
compressed before it). The decoder checks every length and offset against the blob and the page
it was given: a blob read back from the partition or handed over by a service is untrusted
bytes, a malformed one a typed error, never a read past the blob or a write past the page
(`test_reject_truncated_blob`, `test_reject_offset_beyond_output`,
`test_reject_output_over_the_page`, `test_reject_blob_ending_after_a_match`,
`test_reject_literal_run_over_the_blob`). The format has no length header: the caller records
the blob's length and the page's, and a blob that decodes to fewer bytes than a page is refused
by the caller. A page whose blob is not smaller than the page is kept as it is (`OutputFull`); a
zero page is a fact a pool records, not a blob.

**Measured (R11, 6f-0).** `KSELFTEST: mem compress ok (pages= zero= ratio=% smaller= us_per_page=
inflate_us_per_page= sample=image)` on every lane and on the board, in the boot's selftest phase:
the kernel image's content pages (machine code — the hardest content the codec meets; an app's
heap compresses better), zero pages counted and skipped. The numbers and their reading are in
`docs/board/measurements/2026-10-28-backing-stages/README.md`; what they decide: whether a
frozen process is squeezed whole on memd's order (compress cost per page), whether a fault on a
squeezed page inflates synchronously (inflate cost per page), and what the pool gains per page
(the ratio).

**The pool (6f-1, ADR-0087).** What a squeeze reaches, from the census of board cycle 74: an
app-host holds 7.0–8.8 MB resident, 6.68 MB of it the shared image (clean, never compressed);
the private 0.3–2.2 MB is its lazy `.bss` (the heap's bump arena) and its stack. The contract:

- *The order.* `SYSCALL_TASK_SQUEEZE` (69): `(pid, from_va, out_ptr, out_len)` → the next
  address to pass, `0` when the walk reached the end. Only the child's parent (`ESRCH`), only
  when every task of the child's process is frozen and off every hart (`EBUSY`), `from_va`
  page-aligned and the record buffer the caller's (`EINVAL`/`EFAULT`). Each call writes the
  batch record `SqueezeBatch` v1 (`version`, `fields`, then `squeezed`, `zero`, `kept`, `taken`
  — store frames taken — and `stored` — record bytes — as `u64`). One call is one batch: at most
  16 committed pages examined and 4096 page entries scanned, so the kernel lock is held for
  ≈ 0.8 ms on the board, never for a whole process.
- *What is taken.* An object of the child's alone (`vmo_lifetime::squeeze_allowed`): lazy, not
  sealed, never told to a device, not unlocked for purging, mapped by exactly one region of the
  child's space and by no other space, named by no capability outside the child's process —
  refused as `Kind`, `Sealed`, `DeviceBound`, `Unlocked`, `Shared`, `Named`. Per page: zeroes
  are uncommitted (`zero`; the next touch commits a zeroed frame), a block of at most 3 582 bytes
  (a record of at most 7/8 of a page) is recorded (`squeezed`), anything else stays mapped
  (`kept`).
- *Where it lives.* With the object: a store of 64-byte slots, one occupancy word per store
  page; a record is a 2-byte length and the block, at most 56 slots, spanning at most two store
  pages, placed first-fit from the lowest store page with room (a thaw's holes are reused); a
  store page whose last record went returns its frame at once, the object's death or purge all
  of them. The object's page word is absent / a frame / a record (tagged; a frame number fits 31
  bits). Store frames are taken before the object table's lock and returned after it.
- *The way back.* Every path that commits a lazy page — the user fault, the kernel's checked
  range, `vmo_write`/`vmo_read`, a device's runs — inflates a squeezed one: a fresh frame inside
  the object's reach, the record copied out under the table lock, decoded outside it, the length
  checked to be one page (anything else fails the access as damaged and keeps the record), the
  word switched, the record released (`inflated_pages`).
- *The TLB.* A batch clears the PTEs of the pages it changed, runs one shootdown with the kernel
  lock held, and only then returns their frames to the pool.
- *The authority.* memd at `critical`, before any kill: the process longest in the suspended
  band not squeezed since its last band change (a band change makes it a candidate again);
  `emergency` kills at once. `OP_MEM_SQUEEZE {pid}` to abilitymgr (memd's sender id alone; the
  instance must be suspended) → `OP_SQUEEZE {pid}` to execd (abilitymgr's sender id alone; a
  live child) → the batches → the totals back up (`execd: squeeze (…)`, `abilitymgr: squeeze
  (…)`, `memd: squeeze (…)`). A kernel-sourced squeeze reads the level again when it returns.
- *The record.* `mm_stats` v5 appends `squeezed_pages`, `store_frames`, `store_bytes`,
  `inflated_pages`.
- *Proof.* `KSELFTEST: task squeeze ok (…)` on every lane and the board (the refusals, the
  three outcomes, the PTEs, every page back byte-exact, the store empty again);
  `SELFTEST: exec child squeeze ok (…)` on every lane and the board (the launcher's probe child:
  frozen, squeezed, thawed — it checks its own memory before it answers); `SELFTEST: mem squeeze
  ok (…)` on the usb-visible lane (memd's drill at the first freeze, the app thawed after it).
- *Not in 6f-1:* compacting a store, the shared image's clean pages, a record kept after its page
  came back, squeezing a service.

**The swap stage (6f-2, ADR-0088).** The records of the pool leave RAM sealed, through memd and
blkd, and come back on touch. The contract:

- *The key.* `SYSCALL_SWAP_KEY` (70): `(key_ptr, 32)`, once per boot, from the RNG service's kernel
  service id alone (`EPERM` for anyone else, `EBUSY` for a second key). Without a key every export
  is refused (`EINVAL`): no entropy, no swap. The RNG service's entropy device is granted after
  it starts, so the key is seeded on demand: the pager asks the RNG service (`OP_SEED_SWAP_KEY`,
  rngd op 2, memd's kernel identity alone — `rngd: swap key seeded`, or `unavailable` without a
  device) when it first needs to swap; the key goes from rngd into the kernel, never through memd.
  `mm_stats` says whether the boot has its key.
- *The seal.* ChaCha20-Poly1305: the nonce a kernel-wide 64-bit counter (never reused within the
  boot), the associated data the page's identity — the object id and the page index; a sealed
  record is the record's bytes plus a 16-byte tag (3 600 bytes at most).
- *Export.* `SYSCALL_SWAP_EXPORT` (71): `(pid, from_va, buf_ptr, buf_len)` → the next address
  (`0`: done). Only the pressure sink's holder (`EPERM`); the target is any live process but the
  pager itself (`ESRCH`), its lazy regions walked like a squeeze (the same eligibility); at most 16
  records and 4 096 page words a call, while the buffer holds the largest entry. The buffer:
  `version: u32, count: u32`, then per record `token: u64` (object id << 32 | page index), `ctr:
  u64` (the counter it was sealed under), `len: u16`, `pad: u16`, the sealed bytes padded to 4.
  The page stays a record; the kernel remembers it as exported under its counter — a second
  export of a page not yet committed supersedes the first.
- *Commit.* `SYSCALL_SWAP_COMMIT` (72): `(token, ctr, slot)` — the page is still the record of
  that very export (`ESRCH` otherwise: a touch inflated it, a later export superseded it, the
  object is gone): it becomes swapped at `slot`, its record is released (the store page back when
  it empties).
- *Page-in.* A user fault on a swapped page parks the thread (`BlockReason::PageIn {token}`; the
  instruction re-executes); a syscall touching one parks its caller and re-executes (as
  `Reschedule`) — an IPC call already sent makes its reply buffer writable again before the reply
  leaves its queue. The first parked thread queues `OP_PAGE_IN (0x72) {token: u64, slot: u32}` on
  the pressure sink (one request per page, however many threads wait; queued until the sink takes
  it); `SYSCALL_SWAP_PAGE_IN` (73): `(token, slot, sealed_ptr, len)` from the sink's holder — the
  kernel opens it with the page's counter and identity, inflates it into a fresh frame, wakes
  every thread parked on the token. `ESRCH`: the page is not swapped in that slot (stale — its
  waiters are woken to retry); `EINVAL`: the record did not open or decode — refused, never
  entered, the threads parked on it killed (`ExitReason::Fault`, cause `0xF2`; `MM: page-in
  refused (vmo= page= killed=)`); `ENOMEM`: no frame now, the request stands (the pager tries
  again). No sink holder at the fault: the page cannot come back — the fault kills, a syscall is
  `EFAULT`; waiters of a pager that died are woken to meet that.
- *Drops.* An object destroyed or purged with swapped pages queues its slots on the sink as
  `OP_SWAP_DROP (0x73) {count: u8, slot: u32 × count}` (32 a frame): the pager frees them.
- *The record.* `mm_stats` v6 appends `swapped_pages`, `page_ins`, `parked_faults`, `swap_key`.
- *The disk.* `swap` appended to the layout (64 MiB, after `trace`; the smallest disk 448 MiB, was
  384); block selector 7 (`blkd: gpt ok (parts=8)`); blkd's gate grants it to memd alone, read and
  write; one 4 KiB slot per sealed record — its length (`u16`), then the bytes.
- *The pager.* memd, at `critical` after the squeeze: the longest-suspended squeezed app's records
  out (export → write → commit, `memd: swap out (pid= band= pages= slots=)`; the partition opened
  once, `memd: swap on (slots=)`); an `OP_PAGE_IN` served at once; a band leaving `suspended`
  pages the app's slots back before it asks (`memd: swap in (pid= pages= slots=)`); a drop frees
  the slots. Then the kill, as before. Without a key: `memd: swap off (no key)`, once, and the
  policy goes from the squeeze to the kill.
- *Proof.* `KSELFTEST: swap ok (…)` on every lane and the board (the boot proof's own key
  installed and removed around it: the refusals — a key from anyone but the RNG service, a second
  key, an export by a stranger and without a key, a stranger's commit, the superseded export's, a
  stale slot, a tampered and a replayed record — export, commit, the fault's and a syscall's
  verdict, one request for two waiters, the page-in byte-exact and the wake, a waiter killed by a
  record that does not open, the drop at the object's death); `SELFTEST: swap out ok (` and
  `SELFTEST: swap in ok (` on the usb-visible lane (the drill's second `critical` at the first
  freeze, then the thaw). The board has no entropy device: memd's first `critical` would say
  `memd: swap off (no key)`.

**memd's journal and the `lowmem` lane (6f-3, filled 2026-10-29).** Every kill memd ordered and
saw carried out (the victim's band came back `gone`) is recorded in the state store under memd's
own key `/state/memd/journal` (the declared route memd → statefsd, `REQUIRED_ROUTES`; policy
`statefs.read` + `statefs.write`). The record `NXMJ` v1: a 12-byte header — magic, version, a
count ≤ 16, two reserved bytes, the next sequence number — and 24-byte records: the sequence
number (it runs on across boots, so a reader sees which kills it has not seen), the victim's pid
in the boot that killed it, its band, the reason (`oom` — the kernel's level, `drill` — the
harness's), the level judged, the free and total frames of the last kernel frame, milliseconds
since boot; a ring of the newest sixteen. Counts and process identities only, never anything an
app held. The journal is read once, at a boot's first kill; every kill since is appended, the
ring written back and read back — the read-back is the proof: `SELFTEST: memd journal ok
(reason= seq= records=)`. A kill proven while the store cannot be read waits in memory and takes
its number when it can; a refused write is written with the next kill; a stored record that is
not exactly this layout (magic, version, count, length) is replaced by a fresh journal, said once
(`memd: journal reset`). The exchange is waited (statefsd's answer or its death, no clock) on
memd's CAP_MOVE inbox, which no other exchange holds at that moment. A kill the lifecycle
authority carried makes the instance *dying*: its window closing before the death report no
longer thaws a process the parent already reaped, and the freeze grace no longer runs for it.

The `lowmem` lane (`just ci-os-lowmem`, `[profile.lowmem]` extends usb-visible) is the real
pressure the drill stood in for. The injector launches settings and hides it; the grace freezes
it. The harness then takes memory as eager objects until free frames are halfway between the
critical and the emergency watermark — the kernel's own level is `critical`, with room both
ways: the squeeze and the swap-out give frames back without the level recovering past its
hysteresis, and a burst elsewhere does not reach `emergency`, where memd kills at once
(`SELFTEST: mem pressure held (level=critical free= taken=)`). memd's chain runs from the
kernel's frame: the squeeze, the swap-out (the RNG service seeds the key first), the kill
(`SELFTEST: mem pressure kill ok (band=suspended reason=oom)`), the journal (`reason=oom`), the
level told to gpud and windowd (`source=oom`). The harness gives its memory back once memd
ordered the kill and the level recovers to normal (`SELFTEST: mem pressure released
(level=normal free= taken=)`); the injector launches settings again and the new process
restores the record the hidden one saved (`SELFTEST: app restore ok`). The drill's line is
forbidden on the profile (`forbidden_when`, judged by verify-uart): every step is the kernel's
level. First green run (`lowmem--2026-10-29T10-20-12`): 31 978 frames taken to 5 791 free; 448
pages squeezed (net 331 frames, 25 %), 448 slots out, killed, 448 slots dropped; free 39 830
after the release (the harness's frames and the victim's back); the restore 21 ms warm.

## §6 The restorable contract (6e-1 filled 2026-10-27, ADR-0086)

**The word.** `restorable = true` in the manifest (a boolean; nxb-pack refuses the derived grant
in `caps`). `nx app compile --meta` writes `restorable=` into the sidecar; bundlemgrd derives the
one grant `nexus.permission.STATE` at publish (ADR-0084's authority); execd provisions the SDK
route `svc.state` → statefsd into the child's fixed slot 25 for that grant. The route is the
capability: an app without it has no slot.

**The record.** `/state/app/@self/persist` — statefsd rewrites `@self` to the sender's kernel
identity (`sender_service_id`, the `app:<bundle>` sid) before any rule runs; the owner's own
record needs no policy ask, another owner's record is foreign (the generic `statefs.*` grant,
which no app holds). Durable SMALL: the snapshot is bounded at 2048 bytes (`apphost: persist FAIL
(snapshot … over …)` when exceeded); documents are the query service's (`db-queries.md`).

**When.** The compositor pushes its sighting to the window itself (`OP_SURFACE_SEEN` 35: focused
/ visible / hidden — the sighting it states to the lifecycle authority, ADR-0083). On `hidden`
app-host writes the runtime's `@persist` snapshot (`NXPS` v1, name-keyed) inside the grace,
before a freeze or a kill; at the window's close once more, then it exits. Never per change,
never on a clock (privacy: no keystroke timing in the journal, counts in the trace). On the next
launch a program with a persisted field asks once at mount, before the initial effects; the
runtime restores per entry, tolerant of a changed program. Proof (usb-visible lane): `apphost:
persist saved (fields= bytes= reason=hidden|close)` → `SELFTEST: app restore ok (fields= bytes=
warm_us=)`; the keyboard's close is Super+Ctrl+W (`windowd: wm chord (zone=close …)`).

## Invariants (security)

- Identity = `sender_service_id`; a pressure subscription names its service by kernel identity.
- An app's record is owned by the sender's kernel identity (`@self`); a key naming another owner is
  foreign and needs the generic grant (`test_reject_app_key_of_another_owner`).
- A purge never touches a GPU-bound or DMA-bound page.
- Bounded input: every size on the wire is checked against the kinds' limits before allocation.
- `test_reject_*` per phase (TASK-0333).

## Open questions (decided by measurement)

- QEMU's virtio backing-entry bound for a per-band attach of the atlas (moved to 6f, `lowmem`:
  6b-2b keeps the virtio display's one eager object). Not answered by 6f-3: the `lowmem` lane
  takes the pool low through the kill chain and never re-attaches the atlas — still open, with
  the GPU items of layer 8.
- The PTE A/D bits on the board's core (clean/dirty per page in hardware or software) — 6b P0.
- ~~The compressor's ratio and cost per page on RV64 (R11, 6f P0).~~ Answered 2026-10-28 (board
  cycle 74): 47 % on the kernel image's machine code (the ceiling for the pool's gain), 49 µs to
  compress and 16 µs to inflate a page on the board's core (QEMU TCG: 159 / 56 µs) — the pool
  squeezes a frozen process whole, the fault handler inflates synchronously
  (`docs/board/measurements/2026-10-28-backing-stages/README.md`).
